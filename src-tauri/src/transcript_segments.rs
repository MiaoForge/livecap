//! Semantic segments are independent of the provider's audio commit batches.
use crate::transcript::{StreamEvent, TranscriptEvent, TranscriptToken};

const MAX_CHARS: usize = 1600;
const MAX_DURATION_MS: u64 = 60_000;
const PARAGRAPH_GAP_MS: u64 = 1200;

#[derive(Default)]
pub struct SentenceSegments {
    session_id: String,
    pending: Option<TranscriptToken>,
    next_id: u64,
    revision: u32,
}

impl SentenceSegments {
    fn identify(&mut self, token: &mut TranscriptToken, final_text: bool) {
        self.revision += 1;
        token.source_segment_id = Some(format!("sentence-{}", self.next_id));
        token.revision = Some(self.revision);
        token.is_final = final_text;
        if final_text {
            self.next_id += 1;
            self.revision = 0;
        }
    }

    pub fn push(&mut self, event: StreamEvent) -> Vec<StreamEvent> {
        if matches!(event, StreamEvent::TranscriptBoundary) {
            return self.finish();
        }
        let StreamEvent::Transcript(mut event) = event else {
            return vec![event];
        };
        self.session_id.clone_from(&event.session_id);
        let mut output = Vec::new();
        for mut token in event.tokens.drain(..) {
            if token.is_final
                && let Some(previous) = &self.pending
            {
                let gap = token
                    .start_ms
                    .zip(previous.end_ms)
                    .is_some_and(|(start, end)| start.saturating_sub(end) >= PARAGRAPH_GAP_MS);
                if gap || previous.speaker != token.speaker {
                    output.extend(self.finish());
                }
            }
            let final_text = token.is_final;
            if let Some(previous) = &self.pending {
                token.text = join(&previous.text, &token.text);
                token.start_ms = previous.start_ms;
            }
            if !final_text {
                self.identify(&mut token, false);
                output.push(self.event(token));
                continue;
            }
            let text = token.text.trim().to_owned();
            if text.is_empty() {
                self.identify(&mut token, true);
                output.push(self.event(token));
                continue;
            }
            let start = token.start_ms;
            let end = token.end_ms;
            let total = text.chars().count().max(1);
            let mut offset = 0;
            self.pending = None;
            // Keep complete sentences from one batch together to avoid a
            // separate translation request for every short sentence.
            let complete_bytes: usize = parts(&text)
                .iter()
                .take_while(|(_, complete)| *complete)
                .map(|(part, _)| part.len())
                .sum();
            let mut passages = Vec::new();
            if complete_bytes > 0 {
                passages.push((&text[..complete_bytes], true));
            }
            if !text[complete_bytes..].trim().is_empty() {
                passages.push((&text[complete_bytes..], false));
            }
            for (part, complete) in passages {
                let count = part.chars().count();
                let mut segment = token.clone();
                segment.text = part.trim().to_owned();
                segment.start_ms = interpolate(start, end, offset, total);
                offset += count;
                segment.end_ms = interpolate(start, end, offset, total);
                let bounded = segment.text.chars().count() >= MAX_CHARS
                    || segment
                        .start_ms
                        .zip(segment.end_ms)
                        .is_some_and(|(start, end)| end.saturating_sub(start) >= MAX_DURATION_MS);
                if complete || bounded {
                    self.identify(&mut segment, true);
                    output.push(self.event(segment));
                } else {
                    self.pending = Some(segment.clone());
                    self.identify(&mut segment, false);
                    output.push(self.event(segment));
                }
            }
        }
        output
    }

    pub fn finish(&mut self) -> Vec<StreamEvent> {
        let Some(mut token) = self.pending.take() else {
            return Vec::new();
        };
        self.identify(&mut token, true);
        vec![self.event(token)]
    }

    fn event(&self, token: TranscriptToken) -> StreamEvent {
        StreamEvent::Transcript(TranscriptEvent {
            session_id: self.session_id.clone(),
            tokens: vec![token],
            finished: false,
        })
    }
}

fn interpolate(start: Option<u64>, end: Option<u64>, offset: usize, total: usize) -> Option<u64> {
    start
        .zip(end)
        .map(|(start, end)| start + end.saturating_sub(start) * offset as u64 / total as u64)
}

fn join(left: &str, right: &str) -> String {
    let left = left.trim_end();
    let right = right.trim_start();
    let space = left
        .chars()
        .last()
        .zip(right.chars().next())
        .is_some_and(|(a, b)| a.is_ascii() && b.is_ascii_alphanumeric() && a != '-');
    format!("{left}{}{right}", if space { " " } else { "" })
}

// Conservative punctuation boundaries; timing within a provider batch is estimated.
fn parts(text: &str) -> Vec<(&str, bool)> {
    let mut result = Vec::new();
    let mut from = 0;
    for (index, c) in text.char_indices() {
        if index < from {
            continue;
        }
        let mut end = index + c.len_utf8();
        if !matches!(c, '.' | '!' | '?' | '。' | '！' | '？') {
            continue;
        }
        let word = text[from..end].split_whitespace().last().unwrap_or("");
        if c == '.'
            && (word.ends_with("..")
                || text[end..].starts_with('.')
                || matches!(
                    word,
                    "Mr." | "Mrs." | "Ms." | "Dr." | "Prof." | "e.g." | "i.e." | "U.S."
                )
                || (word.len() == 2 && word.as_bytes()[0].is_ascii_uppercase()))
        {
            continue;
        }
        for closing in text[end..].chars() {
            if matches!(
                closing,
                '"' | '\'' | '”' | '’' | ')' | '）' | '」' | '』' | '!' | '?'
            ) {
                end += closing.len_utf8();
            } else {
                break;
            }
        }
        if matches!(c, '。' | '！' | '？')
            || end == text.len()
            || text[end..].starts_with(char::is_whitespace)
        {
            result.push((&text[from..end], true));
            from = end;
        }
    }
    if from < text.len() && !text[from..].trim().is_empty() {
        result.push((&text[from..], false));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::TranslationStatus;

    fn batch(text: &str, start: u64, end: u64, is_final: bool) -> StreamEvent {
        StreamEvent::Transcript(TranscriptEvent {
            session_id: "test".into(),
            finished: false,
            tokens: vec![TranscriptToken {
                text: text.into(),
                start_ms: Some(start),
                end_ms: Some(end),
                is_final,
                speaker: None,
                language: None,
                source_language: None,
                translation_status: TranslationStatus::Original,
                translation_source: None,
                source_segment_id: Some(format!("provider-{start}")),
                revision: Some(1),
            }],
        })
    }
    fn tokens(events: Vec<StreamEvent>) -> Vec<TranscriptToken> {
        events
            .into_iter()
            .flat_map(|event| match event {
                StreamEvent::Transcript(event) => event.tokens,
                _ => vec![],
            })
            .collect()
    }

    #[test]
    fn carries_unfinished_sentence_across_audio_batches_before_final_translation() {
        let mut segments = SentenceSegments::default();
        let first = tokens(segments.push(batch("Welcome. Professor Zhang will", 0, 15000, true)));
        assert_eq!(first.len(), 2);
        assert!(first[0].is_final);
        assert!(!first[1].is_final);
        let draft = tokens(segments.push(batch("explain binary", 15000, 20000, false)));
        assert_eq!(draft[0].text, "Professor Zhang will explain binary");
        let next = tokens(segments.push(batch(
            "explain binary analysis. Next we",
            15000,
            30000,
            true,
        )));
        assert_eq!(
            next[0].text,
            "Professor Zhang will explain binary analysis."
        );
        assert!(next[0].is_final);
        assert_eq!(next[0].source_segment_id, first[1].source_segment_id);
        assert!(next[0].revision > draft[0].revision);
        assert!(!next[1].is_final);
        assert_eq!(next[1].text, "Next we");
        let tail = tokens(segments.finish());
        assert_eq!(tail[0].text, "Next we");
        assert!(tail[0].is_final);
        assert!(segments.finish().is_empty());
        assert!(first[0].end_ms <= next[0].start_ms);
        assert!(next[0].end_ms <= tail[0].start_ms);
        assert_eq!(tail[0].end_ms, Some(30000));
    }

    #[test]
    fn preserves_abbreviations_decimals_quotes_and_chinese_boundaries() {
        assert_eq!(
            parts("Dr. Zhang uses 3.14. \"Why?\" 下一句。继续"),
            vec![
                ("Dr. Zhang uses 3.14.", true),
                (" \"Why?\"", true),
                (" 下一句。", true),
                ("继续", false)
            ]
        );
        assert_eq!(parts("Ask Prof."), vec![("Ask Prof.", false)]);
        assert_eq!(parts("We can..."), vec![("We can...", false)]);
        assert_eq!(join("if true,", "continue"), "if true, continue");
        assert_eq!(join("我们", "继续"), "我们继续");
    }

    #[test]
    fn explicit_pause_flushes_the_confirmed_tail_without_waiting_for_more_speech() {
        let mut segments = SentenceSegments::default();
        segments.push(batch("unfinished thought", 0, 15000, true));
        let tail = tokens(segments.push(StreamEvent::TranscriptBoundary));
        assert_eq!(tail[0].text, "unfinished thought");
        assert!(tail[0].is_final);
        let next = tokens(segments.push(batch("a new thought", 15000, 30000, true)));
        assert_eq!(next[0].text, "a new thought");
        assert_ne!(next[0].source_segment_id, tail[0].source_segment_id);
    }

    #[test]
    fn complete_sentences_from_one_batch_share_a_translation_passage() {
        let mut segments = SentenceSegments::default();
        let output = tokens(segments.push(batch(
            "First sentence. Second sentence. Next we",
            0,
            15000,
            true,
        )));
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].text, "First sentence. Second sentence.");
        assert!(output[0].is_final);
        assert_eq!(output[1].text, "Next we");
        assert!(!output[1].is_final);
    }

    #[test]
    fn empty_completion_settles_a_rejected_draft_without_losing_carried_text() {
        let mut segments = SentenceSegments::default();
        let draft = tokens(segments.push(batch("unreliable draft", 0, 15000, false)));
        let final_text = tokens(segments.push(batch("", 0, 15000, true)));
        assert!(final_text[0].is_final);
        assert_eq!(final_text[0].text, "");
        assert_eq!(final_text[0].source_segment_id, draft[0].source_segment_id);
        assert!(final_text[0].revision > draft[0].revision);
        segments.push(batch("confirmed prefix", 15000, 30000, true));
        segments.push(batch("", 30000, 45000, true));
        assert_eq!(tokens(segments.finish())[0].text, "confirmed prefix");
    }

    #[test]
    fn gaps_and_speaker_changes_do_not_join_unrelated_speech() {
        for speaker in [false, true] {
            let mut segments = SentenceSegments::default();
            segments.push(batch("unfinished thought", 0, 15000, true));
            let mut next = batch(
                "different topic",
                if speaker { 15000 } else { 17000 },
                30000,
                true,
            );
            if speaker && let StreamEvent::Transcript(event) = &mut next {
                event.tokens[0].speaker = Some("2".into());
            }
            let output = tokens(segments.push(next));
            assert_eq!(output[0].text, "unfinished thought");
            assert!(output[0].is_final);
            assert_eq!(output[1].text, "different topic");
        }
    }

    #[test]
    fn unpunctuated_speech_has_bounded_wait_and_retains_final_tail() {
        let mut segments = SentenceSegments::default();
        for i in 0..4 {
            let output =
                tokens(segments.push(batch("continuous speech", i * 15000, (i + 1) * 15000, true)));
            assert_eq!(output[0].is_final, i == 3);
        }
        assert!(segments.finish().is_empty());
        let output = tokens(segments.push(batch(&"x".repeat(MAX_CHARS), 60000, 75000, true)));
        assert!(output[0].is_final);
    }
}
