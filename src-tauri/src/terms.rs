use serde::Serialize;
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

const MAX_CANDIDATES: usize = 50;
const MAX_ACTIVE: usize = 20;
const RECENT_SEGMENTS: usize = 256;
const ACTIVATION_THRESHOLD: u32 = 2;

#[derive(Serialize)]
pub struct Snapshot<'a> {
    session_id: &'a str,
    candidates: Vec<&'a Candidate>,
}

#[derive(Serialize)]
struct Candidate {
    term: String,
    occurrences: u32,
    active: bool,
    #[serde(skip)]
    dismissed: bool,
}

#[derive(Default)]
struct Learned {
    entries: Vec<Candidate>,
    seen: VecDeque<String>,
}

type Publish = dyn Fn(&Snapshot<'_>) + Send + Sync;

pub struct SessionTerms {
    pub session_id: String,
    manual: HashSet<String>,
    learned: Mutex<Learned>,
    publish: Box<Publish>,
}

impl SessionTerms {
    pub fn new(
        session_id: String,
        manual: &[String],
        publish: impl Fn(&Snapshot<'_>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            session_id,
            manual: manual
                .iter()
                .map(|term| term.trim().to_ascii_lowercase())
                .collect(),
            learned: Mutex::new(Learned::default()),
            publish: Box::new(publish),
        }
    }

    pub fn publish_initial(&self) {
        if let Ok(learned) = self.learned.lock() {
            self.emit(&learned);
        }
    }

    pub fn observe(&self, segment_id: &str, text: &str) {
        let Ok(mut learned) = self.learned.lock() else {
            return;
        };
        if learned.seen.iter().any(|id| id == segment_id) {
            return;
        }
        if learned.seen.len() == RECENT_SEGMENTS {
            learned.seen.pop_front();
        }
        learned.seen.push_back(segment_id.into());
        let mut changed = false;
        let mut counted = HashSet::new();
        for term in text.split_whitespace().filter_map(technical_term) {
            let key = term.to_ascii_lowercase();
            if self.manual.contains(&key) || !counted.insert(key) {
                continue;
            }
            if let Some(entry) = learned
                .entries
                .iter_mut()
                .find(|entry| entry.term.eq_ignore_ascii_case(term))
            {
                if !entry.dismissed {
                    entry.occurrences = entry.occurrences.saturating_add(1);
                    changed = true;
                }
            } else if learned.entries.len() < MAX_CANDIDATES {
                learned.entries.push(Candidate {
                    term: term.into(),
                    occurrences: 1,
                    active: false,
                    dismissed: false,
                });
                changed = true;
            }
        }
        if changed {
            activate(&mut learned.entries);
            self.emit(&learned);
        }
    }

    pub fn active(&self) -> Vec<String> {
        self.learned
            .lock()
            .map(|learned| {
                learned
                    .entries
                    .iter()
                    .filter(|entry| entry.active)
                    .map(|entry| entry.term.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn dismiss(&self, term: &str) -> Result<(), String> {
        let mut learned = self.learned.lock().map_err(|_| "无法读取会话术语。")?;
        let Some(entry) = learned
            .entries
            .iter_mut()
            .find(|entry| entry.term.eq_ignore_ascii_case(term))
        else {
            return Ok(());
        };
        if !entry.dismissed {
            entry.dismissed = true;
            entry.active = false;
            activate(&mut learned.entries);
            self.emit(&learned);
        }
        Ok(())
    }

    fn emit(&self, learned: &Learned) {
        (self.publish)(&Snapshot {
            session_id: &self.session_id,
            candidates: learned
                .entries
                .iter()
                .filter(|entry| !entry.dismissed)
                .collect(),
        });
    }
}

fn activate(entries: &mut [Candidate]) {
    let slots = MAX_ACTIVE.saturating_sub(entries.iter().filter(|entry| entry.active).count());
    for entry in entries
        .iter_mut()
        .filter(|entry| {
            !entry.active && !entry.dismissed && entry.occurrences >= ACTIVATION_THRESHOLD
        })
        .take(slots)
    {
        entry.active = true;
    }
}

fn technical_term(word: &str) -> Option<&str> {
    let term = word.trim_matches([
        '.', ',', '!', '?', ';', ':', '"', '\'', '“', '”', '‘', '’', '(', ')', '[', ']', '{', '}',
        '。', '，', '！', '？', '、',
    ]);
    if !(2..=32).contains(&term.len())
        || !term.as_bytes()[0].is_ascii_alphabetic()
        || !term
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-:+#".contains(&byte))
        || !crate::openai::valid_keyword(term)
    {
        return None;
    }
    let acronym = term.len() <= 12
        && term
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_uppercase()))
        && !matches!(
            term,
            "OK" | "TV" | "AM" | "PM" | "US" | "UK" | "USA" | "EU" | "ID" | "NO" | "YES"
        );
    let camel_case = term.len() >= 4
        && term.bytes().skip(1).any(|byte| byte.is_ascii_uppercase())
        && term.bytes().any(|byte| byte.is_ascii_lowercase())
        && term.bytes().all(|byte| byte.is_ascii_alphanumeric());
    let code_form = ["::", "_"].iter().any(|separator| {
        let mut parts = term.split(separator);
        let mut count = 0;
        let valid = parts.all(|part| {
            count += 1;
            part.starts_with(|c: char| c.is_ascii_alphabetic())
                && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
        count > 1 && valid
    }) || matches!(term, "C++" | "C#" | "F#");
    (acronym || camel_case || code_form).then_some(term)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::sync::Arc;

    #[test]
    fn extracts_conservative_technical_forms_without_general_words_or_addresses() {
        for (input, expected) in [
            ("LLVM,", Some("LLVM")),
            ("TableGen", Some("TableGen")),
            ("C++", Some("C++")),
            ("(RISC-V)", Some("RISC-V")),
            ("std::vector", Some("std::vector")),
            ("Hello", None),
            ("Clang", None),
            ("Sema", None),
            ("and", None),
            ("OK", None),
            ("A---", None),
            ("123", None),
            ("https://LLVM.dev", None),
            ("LLVM@example.com", None),
        ] {
            assert_eq!(technical_term(input), expected, "{input}");
        }
    }

    #[test]
    fn distinct_segments_activate_terms_manual_and_dismissed_terms_stay_excluded() {
        let snapshots = Arc::new(Mutex::new(Vec::<Value>::new()));
        let events = snapshots.clone();
        let terms = SessionTerms::new("current".into(), &["llvm".into()], move |snapshot| {
            events
                .lock()
                .unwrap()
                .push(serde_json::to_value(snapshot).unwrap());
        });
        terms.publish_initial();
        terms.observe("a", "LLVM C++ C++ TableGen");
        terms.observe("a", "C++");
        assert!(terms.active().is_empty());
        terms.observe("b", "LLVM C++");
        assert_eq!(terms.active(), ["C++"]);
        assert_eq!(
            snapshots.lock().unwrap().last().unwrap(),
            &json!({
                "session_id":"current",
                "candidates":[{"term":"C++","occurrences":2,"active":true},
                {"term":"TableGen","occurrences":1,"active":false}]
            })
        );
        terms.dismiss("c++").unwrap();
        terms.observe("c", "LLVM C++ TableGen");
        assert_eq!(terms.active(), ["TableGen"]);
        assert_eq!(
            snapshots.lock().unwrap().last().unwrap()["candidates"],
            json!([
                {"term":"TableGen","occurrences":2,"active":true}
            ])
        );
        let next = SessionTerms::new("next".into(), &[], |_| {});
        next.observe("a", "C++");
        assert!(next.active().is_empty());
    }

    #[test]
    fn bounds_candidates_and_active_hints() {
        let terms = SessionTerms::new("bounded".into(), &[], |_| {});
        let source = (0..MAX_CANDIDATES + 15)
            .map(|index| format!("CodeTerm{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        terms.observe("a", &source);
        terms.observe("b", &source);
        assert_eq!(terms.learned.lock().unwrap().entries.len(), MAX_CANDIDATES);
        assert_eq!(terms.active().len(), MAX_ACTIVE);
        terms.dismiss("CodeTerm0").unwrap();
        let active = terms.active();
        assert_eq!(active.len(), MAX_ACTIVE);
        assert!(!active.iter().any(|term| term == "CodeTerm0"));
    }
}
