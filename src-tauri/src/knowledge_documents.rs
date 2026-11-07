use reqwest::Client;
use scraper::{ElementRef, Html, Selector};

const MAX_DOCUMENT_BYTES: usize = 5 * 1024 * 1024;
const BATCH_BYTES: usize = 36_000;

pub async fn read(client: &Client, url: &str) -> Result<(String, Vec<String>), String> {
    let fail = || format!("无法读取官方文档正文（{url}）。现有资料保留。");
    // This function is used only with the preset's built-in HTTPS URLs.
    // The client disables redirects and does not attach API credentials to GETs.
    let mut response = client.get(url).send().await.map_err(|_| fail())?;
    if !response.status().is_success() {
        return Err(fail());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| fail())? {
        if chunk.len() > MAX_DOCUMENT_BYTES - body.len() {
            return Err(format!("官方文档超过读取上限（{url}）。现有资料保留。"));
        }
        body.extend_from_slice(&chunk);
    }
    let html = std::str::from_utf8(&body).map_err(|_| fail())?;
    parse(html).ok_or_else(fail)
}

fn parse(html: &str) -> Option<(String, Vec<String>)> {
    let document = Html::parse_document(html);
    let main = Selector::parse("[role=main], main").unwrap();
    let blocks = Selector::parse("h1,h2,h3,h4,h5,h6,p,dt,pre,li,th,td").unwrap();
    let main = document.select(&main).next()?;
    let main = main
        .select(&Selector::parse("[role=main]").unwrap())
        .next()
        .unwrap_or(main);
    let title = Selector::parse("h1").unwrap();
    let title = main.select(&title).next().map(clean_text)?;
    let mut batches = Vec::new();
    let mut batch = String::new();
    let mut heading = String::new();

    for block in main.select(&blocks) {
        if block
            .ancestors()
            .take_while(|node| node.id() != main.id())
            .any(|node| ElementRef::wrap(node).is_some_and(|parent| blocks.matches(&parent)))
        {
            continue;
        }
        let text = clean_text(block);
        if text.is_empty() {
            continue;
        }
        if matches!(
            block.value().name(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            if !batch.is_empty() && batch.len() + text.len() + 4 >= BATCH_BYTES {
                batches.push(std::mem::take(&mut batch));
            }
            heading = format!("## {}\n", text.chars().take(300).collect::<String>());
            batch.push_str(&heading);
            continue;
        }
        for word in text.split_whitespace() {
            // A long code literal may itself exceed one batch. Preserve its pieces.
            let mut rest = word;
            while !rest.is_empty() {
                if batch.len() + rest.len() < BATCH_BYTES {
                    batch.push_str(rest);
                    batch.push(' ');
                    break;
                }
                if BATCH_BYTES - batch.len() <= 4
                    || (batch.len() > heading.len() && rest.len() < BATCH_BYTES - heading.len())
                {
                    batches.push(std::mem::take(&mut batch));
                    batch.push_str(&heading);
                    continue;
                }
                let mut end = (BATCH_BYTES - batch.len() - 1).min(rest.len());
                while !rest.is_char_boundary(end) {
                    end -= 1;
                }
                batch.push_str(&rest[..end]);
                rest = &rest[end..];
                batches.push(std::mem::take(&mut batch));
                batch.push_str(&heading);
            }
        }
        if batch.len() < BATCH_BYTES {
            batch.push('\n');
        }
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    (!title.is_empty() && !batches.is_empty()).then_some((title, batches))
}

fn clean_text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['#', '¶'])
        .trim()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_all_sections_without_navigation_or_duplicate_list_text() {
        let long = "例子 ".repeat(BATCH_BYTES);
        let html = format!(
            "<nav>sidebar</nav><main><article role='main'><h1>LLVM</h1>
             <h2>PGO</h2><p>Profile-guided optimization {long}</p>
             <h2>LLVM IR</h2><ul><li><p>Freeze instruction</p></li></ul></article></main>"
        );
        let (title, batches) = parse(&html).unwrap();
        let text = batches.join(" ");
        assert_eq!(title, "LLVM");
        assert!(!text.contains("sidebar"));
        assert!(batches.iter().all(|batch| batch.len() <= BATCH_BYTES));
        assert_eq!(text.matches("例子").count(), BATCH_BYTES);
        assert_eq!(text.matches("Freeze instruction").count(), 1);
    }
}
