use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    future::Future,
    sync::Mutex,
};
use tokio::sync::oneshot;

pub const MAX_KNOWLEDGE_SOURCES: usize = 192;
const MAX_CACHE_KEYWORDS: usize = 24;
const MAX_ASR_KEYWORDS: usize = 100;
const MAX_HINTS: usize = 6;
const MAX_HINT_CHARS: usize = 2_000;
const INVALID_CACHE: &str = "生成的知识缓存格式不完整，请保留原有缓存。";
const CANCELLED: &str = "知识缓存生成已取消。已发出的请求仍可能计费。";

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSource {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeEntry {
    pub term: String,
    #[serde(default)]
    pub coverage_area: String,
    pub aliases: Vec<String>,
    pub meaning: String,
    pub translation: String,
    pub example: String,
    pub source_urls: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCache {
    pub domain: String,
    pub focus: String,
    pub updated_at: String,
    pub enabled: bool,
    pub sources: Vec<KnowledgeSource>,
    pub entries: Vec<KnowledgeEntry>,
    pub usage: KnowledgeUsage,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub web_search_calls: u32,
    pub estimated_usd: Option<f64>,
    pub incomplete: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeRequest {
    pub domain: String,
    pub focus: String,
    #[serde(default)]
    pub source_urls: Vec<String>,
}

impl KnowledgeRequest {
    pub fn validate(&mut self) -> Result<(), String> {
        self.domain = self.domain.trim().into();
        self.focus = self.focus.trim().into();
        if !field(&self.domain, 120, true) || !field(&self.focus, 600, false) {
            return Err("请填写领域（最多 120 字）和关注范围（最多 600 字）。".into());
        }
        if self.source_urls.len() > 5 {
            return Err("最多可填写 5 个公开 HTTPS 参考链接。".into());
        }
        let mut seen = HashSet::new();
        for url in &mut self.source_urls {
            *url = url.trim().into();
            if !validate_public_url(url) {
                return Err("参考链接需要是公开的 HTTPS 地址。".into());
            }
        }
        self.source_urls.retain(|url| seen.insert(url.clone()));
        Ok(())
    }
}

pub fn validate_public_url(value: &str) -> bool {
    if !value.starts_with("https://")
        || value.len() > 2_048
        || value.contains('\\')
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let Some(host) = url.host_str().map(|host| host.trim_end_matches('.')) else {
        return false;
    };
    let Some((_, suffix)) = host.rsplit_once('.') else {
        return false;
    };
    // Accept public-looking DNS names only. The app never fetches these URLs itself.
    !host.ends_with(".home.arpa")
        && !matches!(
            suffix,
            "localhost"
                | "local"
                | "localdomain"
                | "lan"
                | "internal"
                | "test"
                | "invalid"
                | "example"
                | "onion"
                | "arpa"
        )
        && suffix.bytes().any(|byte| byte.is_ascii_alphabetic())
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn field(value: &str, max: usize, required: bool) -> bool {
    (!required || !value.trim().is_empty())
        && value.chars().count() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

impl KnowledgeCache {
    pub fn validate_generated(&self) -> Result<(), String> {
        if !field(&self.domain, 120, true)
            || !field(&self.focus, 600, false)
            || !(1..=MAX_KNOWLEDGE_SOURCES).contains(&self.sources.len())
            || self.entries.is_empty()
            || self.usage.cached_input_tokens > self.usage.input_tokens
            || self
                .usage
                .estimated_usd
                .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
        {
            return Err(INVALID_CACHE.into());
        }
        let mut sources = HashSet::new();
        for source in &self.sources {
            if !validate_public_url(&source.url)
                || !field(&source.title, 180, true)
                || !sources.insert(source.url.as_str())
            {
                return Err(INVALID_CACHE.into());
            }
        }
        let mut seen = HashSet::new();
        for entry in &self.entries {
            if !valid_spoken_term(&entry.term)
                || !seen.insert(entry.term.trim().to_lowercase())
                || entry.aliases.len() > 8
                || !entry.aliases.iter().all(|alias| valid_spoken_term(alias))
                || !field(&entry.meaning, 300, true)
                || !field(&entry.translation, 100, true)
                || !field(&entry.example, 300, false)
                || !(1..=12).contains(&entry.source_urls.len())
                || !entry
                    .source_urls
                    .iter()
                    .all(|url| sources.contains(url.as_str()))
            {
                return Err(INVALID_CACHE.into());
            }
        }
        Ok(())
    }

    pub fn source_url(&self, url: &str) -> Option<reqwest::Url> {
        if !validate_public_url(url) || !self.sources.iter().any(|source| source.url == url) {
            return None;
        }
        reqwest::Url::parse(url).ok()
    }

    pub fn retrieve(
        &self,
        source: &str,
        context: &VecDeque<String>,
        manual: &[String],
    ) -> Vec<KnowledgeHint<'_>> {
        if !self.enabled {
            return Vec::new();
        }
        let source = spoken_text(source);
        let manual: HashSet<_> = manual
            .iter()
            .map(|term| term.trim().to_lowercase())
            .collect();
        let context: Vec<_> = context
            .iter()
            .rev()
            .take(3)
            .map(|text| spoken_text(text))
            .collect();
        let mut ranked: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                if !valid_spoken_term(&entry.term) {
                    return None;
                }
                if std::iter::once(&entry.term)
                    .chain(&entry.aliases)
                    .any(|term| manual.contains(&term.trim().to_lowercase()))
                {
                    return None;
                }
                let score = std::iter::once(&entry.term)
                    .chain(&entry.aliases)
                    .filter(|term| valid_spoken_term(term))
                    .map(|term| {
                        let term = term.trim().to_lowercase();
                        let weight = term.chars().count();
                        if contains_term(&source, &term) {
                            1000 + weight
                        } else {
                            context
                                .iter()
                                .position(|text| contains_term(text, &term))
                                .map_or(0, |index| 500 - index * 100 + weight)
                        }
                    })
                    .max()
                    .unwrap_or(0);
                (score > 0).then_some((score, entry))
            })
            .collect();
        ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        let mut remaining = MAX_HINT_CHARS - 2;
        ranked
            .into_iter()
            .filter_map(|(_, entry)| {
                let hint = KnowledgeHint {
                    term: &entry.term,
                    meaning: &entry.meaning,
                    translation: &entry.translation,
                };
                let size = serde_json::to_string(&hint).ok()?.chars().count() + 1;
                remaining = remaining.checked_sub(size)?;
                Some(hint)
            })
            .take(MAX_HINTS)
            .collect()
    }
}

#[derive(Serialize)]
pub struct KnowledgeHint<'a> {
    term: &'a str,
    meaning: &'a str,
    translation: &'a str,
}

fn has_code_syntax(term: &str) -> bool {
    let slash_acronym = term.contains('/')
        && term.split('/').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"+#-".contains(&byte)
                })
        });
    (term.contains('/') && !slash_acronym)
        || term.contains(['\t', '_', '`', '\\', '=', '[', ']', '{', '}', ';', '$', '%'])
        || term.contains("::")
        || term.char_indices().any(|(index, character)| {
            character == '(' && index > 0 && !term[..index].ends_with(char::is_whitespace)
        })
        || term.split_whitespace().any(|word| {
            word.trim_start_matches(['(', '"', '\''])
                .starts_with(['-', '−', '@'])
        })
}

pub(crate) fn valid_spoken_term(term: &str) -> bool {
    let term = term.trim();
    // Automatic vocabulary is for speech. Keep names such as C++, Node.js and
    // TableGen, but do not feed source syntax or command switches into ASR.
    field(term, 80, true) && crate::openai::valid_keyword(term) && !has_code_syntax(term)
}

fn spoken_text(text: &str) -> String {
    // An acronym embedded in a switch such as --ast-dump is not a spoken match.
    text.split_whitespace()
        .filter(|word| {
            !has_code_syntax(word.trim_matches([
                '.', ',', '!', '?', ';', '"', '\'', '“', '”', '‘', '’', '。', '，', '！', '？',
                '、',
            ]))
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) fn contains_term(text: &str, term: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    !term.is_empty()
        && text.match_indices(term).any(|(index, matched)| {
            !text[..index].chars().next_back().is_some_and(word)
                && !text[index + matched.len()..]
                    .chars()
                    .next()
                    .is_some_and(word)
        })
}

fn topic_words(text: &str) -> HashSet<&str> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .filter(|word| {
            !matches!(
                *word,
                "the" | "of" | "and" | "for" | "in" | "to" | "with" | "llvm" | "clang"
            )
        })
        .collect()
}

fn spoken_acronym(term: &str) -> bool {
    (2..=12).contains(&term.len())
        && term
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && term.bytes().any(|byte| byte.is_ascii_uppercase())
}

fn hotword_key(term: &str) -> String {
    let trimmed = term.trim();
    let base = trimmed
        .strip_suffix(')')
        .and_then(|name| name.rsplit_once('('))
        .filter(|(base, suffix)| {
            let suffix = suffix.trim();
            spoken_acronym(suffix)
                && base
                    .split(|c: char| !c.is_alphanumeric())
                    .filter_map(|word| word.chars().next())
                    .map(|initial| initial.to_ascii_uppercase())
                    .collect::<String>()
                    .ends_with(suffix)
        })
        .map_or(trimmed, |(base, _)| base);
    base.to_lowercase()
        .chars()
        .filter(|character| {
            !character.is_whitespace() && !matches!(character, '-' | '‐' | '‑' | '–')
        })
        .collect()
}

impl KnowledgeCache {
    pub(crate) fn retain_spoken_terms(&mut self) {
        self.entries.retain(|entry| valid_spoken_term(&entry.term));
        for entry in &mut self.entries {
            entry.aliases.retain(|alias| valid_spoken_term(alias));
        }
        if self.entries.is_empty() {
            self.enabled = false;
        }
    }

    pub(crate) fn normalize_translations(&mut self) {
        let llvm = self.focus.to_ascii_lowercase().contains("llvm");
        for entry in &mut self.entries {
            let key = hotword_key(&entry.term);
            if key == "profileguidedoptimization"
                || (key == "pgo"
                    && (llvm
                        || entry
                            .aliases
                            .iter()
                            .any(|alias| hotword_key(alias) == "profileguidedoptimization")))
            {
                entry.translation = "剖析引导优化".into();
            }
        }
    }
}

pub fn merge_asr_terms(
    manual: &[String],
    cache: Option<&KnowledgeCache>,
    meeting_topic: &str,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut merged: Vec<_> = manual
        .iter()
        .filter_map(|term| {
            let key = hotword_key(term);
            seen.insert(key).then(|| term.clone())
        })
        .take(MAX_ASR_KEYWORDS)
        .collect();
    if let Some(cache) = cache.filter(|cache| cache.enabled) {
        let cap = MAX_CACHE_KEYWORDS.min(MAX_ASR_KEYWORDS - merged.len());
        let topic = meeting_topic.to_lowercase();
        let words = topic_words(&topic);
        let score = |entry: &KnowledgeEntry| {
            if words.is_empty() {
                return 0;
            }
            let area = entry.coverage_area.to_lowercase();
            let label = std::iter::once(entry.term.as_str())
                .chain(entry.aliases.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let description = format!("{} {}", entry.meaning, entry.translation).to_lowercase();
            words
                .iter()
                .map(|word| {
                    if contains_term(&label, word) {
                        8
                    } else if contains_term(&area, word)
                        || (!word.is_ascii() && area.contains(word))
                    {
                        4
                    } else if contains_term(&description, word)
                        || (!word.is_ascii() && description.contains(word))
                    {
                        1
                    } else {
                        0
                    }
                })
                .sum::<usize>()
        };
        let mut ranked: Vec<_> = cache
            .entries
            .iter()
            .filter(|entry| valid_spoken_term(&entry.term))
            .map(|entry| (score(entry), entry))
            .collect();
        ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        let preferred = ranked.iter().flat_map(|(score, entry)| {
            let acronym = (*score > 0)
                .then(|| {
                    entry
                        .aliases
                        .iter()
                        .find(|alias| spoken_acronym(alias.trim()))
                })
                .flatten();
            std::iter::once(&entry.term).chain(acronym)
        });
        merged.extend(
            preferred
                .chain(ranked.iter().flat_map(|(_, entry)| &entry.aliases))
                .filter_map(|term| {
                    let term = term.trim();
                    (valid_spoken_term(term) && seen.insert(hotword_key(term)))
                        .then(|| term.to_owned())
                })
                .take(cap),
        );
    }
    merged
}

#[derive(Default)]
pub struct Generation(Mutex<Option<Option<oneshot::Sender<()>>>>);

pub struct Ticket<'a> {
    generation: &'a Generation,
    cancelled: oneshot::Receiver<()>,
}

impl Generation {
    pub fn begin(&self) -> Result<Ticket<'_>, String> {
        let mut state = self.0.lock().map_err(|_| INVALID_CACHE)?;
        if state.is_some() {
            return Err("知识缓存正在生成。".into());
        }
        let (stop, cancelled) = oneshot::channel();
        *state = Some(Some(stop));
        Ok(Ticket {
            generation: self,
            cancelled,
        })
    }

    pub fn cancel(&self) {
        if let Ok(mut state) = self.0.lock()
            && let Some(stop) = state.as_mut().and_then(Option::take)
        {
            let _ = stop.send(());
        }
    }

    pub fn edit<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let state = self.0.lock().map_err(|_| INVALID_CACHE)?;
        if state.is_some() {
            return Err("请先取消正在生成的知识缓存。".into());
        }
        operation()
    }
}

impl Ticket<'_> {
    pub async fn wait<T>(
        &mut self,
        future: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        tokio::select! {
            biased;
            _ = &mut self.cancelled => Err(CANCELLED.into()),
            result = future => result,
        }
    }

    pub fn commit<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let state = self.generation.0.lock().map_err(|_| INVALID_CACHE)?;
        if !state.as_ref().is_some_and(Option::is_some) {
            return Err(CANCELLED.into());
        }
        operation()
    }
}

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.generation.0.lock() {
            *state = None;
        }
    }
}

#[cfg(test)]
pub(crate) fn sample_cache(terms: &[&str]) -> KnowledgeCache {
    const SOURCE: &str = "https://llvm.org/docs/";
    KnowledgeCache {
        domain: "Compiler engineering".into(),
        focus: "LLVM".into(),
        updated_at: String::new(),
        enabled: true,
        sources: vec![KnowledgeSource {
            url: SOURCE.into(),
            title: "LLVM documentation".into(),
        }],
        entries: terms
            .iter()
            .map(|term| KnowledgeEntry {
                term: (*term).into(),
                coverage_area: String::new(),
                aliases: Vec::new(),
                meaning: format!("Definition of {term}"),
                translation: (*term).into(),
                example: String::new(),
                source_urls: vec![SOURCE.into()],
            })
            .collect(),
        usage: KnowledgeUsage::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_vocabulary_rejects_code_syntax_but_preserves_established_names() {
        for term in [
            "--help",
            "(-O2)",
            "\"--help\"",
            "-O2",
            "-fno-rtti",
            "clang -Xclang",
            "__attribute__",
            "__builtin_expect",
            "LLVM_VERSION_MAJOR",
            "get_value",
            "llvm::Module",
            "getName()",
            "foo(int)",
            "std::vector<T>",
            "-std=c++20",
            "foo=bar",
            "include/clang.h",
            "`AST`",
            "$PATH",
            "@llvm.memcpy",
            "value[0]",
        ] {
            assert!(!valid_spoken_term(term), "{term}");
        }
        for term in [
            "LLVM",
            "AST",
            "Sema",
            "TableGen",
            "ASTContext",
            "C++",
            "C#",
            "F#",
            "RISC-V",
            "Objective-C",
            "Node.js",
            ".NET",
            "TCP/IP",
            "CI/CD",
            "HTTP/2",
            "profile-guided optimization",
            "abstract syntax tree (AST)",
            "静态单赋值",
        ] {
            assert!(valid_spoken_term(term), "{term}");
        }
    }

    #[test]
    fn code_entries_and_aliases_do_not_enter_translation_hints_or_asr() {
        let mut cache = sample_cache(&["__attribute__", "-O2", "AST", "Sema"]);
        cache.entries[2].aliases = vec!["--ast-dump".into(), "syntax tree".into()];
        let context = VecDeque::from(["__attribute__ -O2 --ast-dump".into()]);
        assert!(
            cache
                .retrieve("__attribute__ -O2 --ast-dump", &context, &[])
                .is_empty()
        );
        assert_eq!(
            cache.retrieve("syntax tree", &VecDeque::new(), &[])[0].term,
            "AST"
        );
        assert_eq!(cache.retrieve("AST;", &VecDeque::new(), &[])[0].term, "AST");
        assert_eq!(
            cache.retrieve("Sema,", &VecDeque::new(), &[])[0].term,
            "Sema"
        );
        assert_eq!(
            merge_asr_terms(&[], Some(&cache), ""),
            ["AST", "Sema", "syntax tree"]
        );
        // Explicit user vocabulary remains available even when it is code-shaped.
        let merged = merge_asr_terms(&["__custom__".into()], Some(&cache), "");
        assert_eq!(merged[0], "__custom__");
        cache.retain_spoken_terms();
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.entries[0].aliases, ["syntax tree"]);
        cache.validate_generated().unwrap();
    }

    #[test]
    fn only_public_https_references_and_stored_source_links_are_allowed() {
        let cache = sample_cache(&["AST"]);
        assert!(cache.source_url("https://llvm.org/docs/").is_some());
        assert!(cache.source_url("https://clang.llvm.org/").is_none());
        for url in [
            "http://llvm.org/docs/",
            "https://localhost/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://169.254.169.254/",
            "https://router.home.arpa/",
            "https://home.arpa/",
            "https://somehost.localdomain/",
            "https://name@llvm.org/",
            "https://llvm.org:8080/",
            "https:llvm.org",
            "https://llvm.org\\@localhost/",
        ] {
            assert!(!validate_public_url(url), "{url}");
        }
        let mut request = KnowledgeRequest {
            domain: " Compiler engineering ".into(),
            focus: String::new(),
            source_urls: vec![
                " https://llvm.org/docs/ ".into(),
                "https://llvm.org/docs/".into(),
            ],
        };
        request.validate().unwrap();
        assert_eq!(request.domain, "Compiler engineering");
        assert_eq!(request.source_urls, ["https://llvm.org/docs/"]);
        request.source_urls.push("https://localhost/".into());
        assert!(request.validate().is_err());
    }

    #[test]
    fn generated_cache_has_no_business_entry_count_cap() {
        let terms: Vec<_> = (0..150).map(|index| format!("Term{index}")).collect();
        let cache = sample_cache(&terms.iter().map(String::as_str).collect::<Vec<_>>());
        cache.validate_generated().unwrap();
        assert_eq!(cache.entries.len(), 150);
    }

    #[test]
    fn meeting_hotwords_keep_spoken_acronym_without_repeating_full_name() {
        let mut cache = sample_cache(&[
            "Profile-Guided Optimization",
            "Profile Guided Optimization",
            "Profile Guided Optimization (PGO)",
            "Loop Optimization",
        ]);
        for entry in &mut cache.entries[..3] {
            entry.aliases = vec!["PGO".into()];
            entry.translation = "配置文件引导优化".into();
        }
        cache.normalize_translations();
        assert!(
            cache.entries[..3]
                .iter()
                .all(|entry| entry.translation == "剖析引导优化")
        );
        assert_eq!(
            merge_asr_terms(&[], Some(&cache), "LLVM profile-guided optimization"),
            ["Profile-Guided Optimization", "PGO", "Loop Optimization"],
        );
    }

    #[test]
    fn meeting_topic_prioritizes_relevant_terms_outside_the_default_keyword_budget() {
        let names: Vec<_> = (0..30).map(|i| format!("IR term {i}")).collect();
        let mut names: Vec<_> = names.iter().map(String::as_str).collect();
        names.extend(["Sema", "QualType"]);
        let mut cache = sample_cache(&names);
        for entry in &mut cache.entries[30..] {
            entry.coverage_area = "Clang frontend and semantic analysis".into();
        }
        let manual = vec!["LLVM".to_owned()];
        let baseline = merge_asr_terms(&manual, Some(&cache), "");
        let selected = merge_asr_terms(&manual, Some(&cache), "Clang frontend");
        assert!(!baseline.contains(&"Sema".into()));
        assert_eq!(&selected[..3], ["LLVM", "Sema", "QualType"]);
        assert_eq!(baseline.len(), selected.len());
    }

    #[test]
    fn retrieval_uses_boundaries_current_text_aliases_and_manual_priority() {
        let mut cache = sample_cache(&["IR", "AST", "Sema", "C++"]);
        cache.entries[1].aliases.push("syntax tree".into());
        let context = VecDeque::from(["An AST follows.".into()]);
        let hints = cache.retrieve(
            "First check the syntax tree in C++.",
            &context,
            &["AST".into()],
        );
        let terms: Vec<_> = hints.iter().map(|hint| hint.term).collect();
        assert_eq!(terms, ["C++"]);
        assert!(
            cache
                .retrieve("This is unrelated.", &VecDeque::new(), &[])
                .is_empty()
        );
        let hints = cache.retrieve("Check Sema.", &context, &[]);
        assert_eq!(
            hints.iter().map(|hint| hint.term).collect::<Vec<_>>(),
            ["Sema", "AST"]
        );
        assert_eq!(
            cache.retrieve("syntax tree", &VecDeque::new(), &[])[0].term,
            "AST"
        );
        cache.enabled = false;
        assert!(cache.retrieve("AST", &context, &[]).is_empty());
        assert_eq!(
            merge_asr_terms(&["Manual".into()], Some(&cache), ""),
            ["Manual"]
        );
    }

    #[test]
    fn retrieval_and_asr_vocabulary_are_bounded() {
        let terms: Vec<_> = (0..32).map(|index| format!("Term{index}")).collect();
        let mut cache = sample_cache(&terms.iter().map(String::as_str).collect::<Vec<_>>());
        cache.entries[0].aliases.push("Important alias".into());
        let merged = merge_asr_terms(&["Manual".into(), "term0".into()], Some(&cache), "");
        assert_eq!(merged.len(), 26);
        assert_eq!(&merged[..4], ["Manual", "term0", "Term1", "Term2"]);
        assert!(!merged.iter().any(|term| term == "Important alias"));
        let manual: Vec<_> = (0..98).map(|index| format!("Manual{index}")).collect();
        assert_eq!(merge_asr_terms(&manual, Some(&cache), "").len(), 100);
        let source = terms.join(" ");
        let context = VecDeque::new();
        assert_eq!(cache.retrieve(&source, &context, &[]).len(), MAX_HINTS);
        for entry in &mut cache.entries {
            entry.meaning = "\\".repeat(300);
        }
        let hints = cache.retrieve(&source, &context, &[]);
        assert!(hints.len() < MAX_HINTS);
        assert!(serde_json::to_string(&hints).unwrap().chars().count() <= MAX_HINT_CHARS);
    }

    #[tokio::test]
    async fn cancelled_generation_cannot_commit_and_holds_slot_until_exit() {
        let generation = Generation::default();
        let mut ticket = generation.begin().unwrap();
        assert!(generation.begin().is_err());
        assert!(generation.edit(|| Ok(())).is_err());
        generation.cancel();
        assert_eq!(
            ticket
                .wait(std::future::pending::<Result<(), String>>())
                .await
                .unwrap_err(),
            CANCELLED
        );
        assert!(
            ticket
                .commit(|| -> Result<(), String> { panic!("cancelled ticket wrote cache") })
                .is_err()
        );
        assert!(generation.begin().is_err());
        drop(ticket);
        let ticket = generation.begin().unwrap();
        assert_eq!(ticket.commit(|| Ok("fresh")).unwrap(), "fresh");
    }
}
