use crate::{
    language::Language,
    record_ai::{self, Input},
    records::{RecordDiagram, RecordInsights, TimelinePoint},
    transcript::{StreamEvent, TranslationStatus},
    usage::Tokens,
};
use reqwest::header::HeaderValue;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_PENDING_CHARS: usize = 16_000;
const UPDATE_INTERVAL: Duration = Duration::from_secs(30);

type Publish = dyn Fn(&SummaryView) + Send + Sync;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryContent {
    pub title: String,
    pub notes: String,
    pub timeline: Vec<TimelinePoint>,
    pub diagram: Option<RecordDiagram>,
    pub source_times: Vec<Option<u64>>,
    pub covered_through_ms: Option<u64>,
    pub truncated: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryView {
    session_id: String,
    phase: &'static str,
    revision: u64,
    content: Option<SummaryContent>,
    error: Option<String>,
}

struct Excerpt {
    sequence: u64,
    text: String,
    start_ms: Option<u64>,
}

struct State {
    pending: VecDeque<Excerpt>,
    pending_chars: usize,
    sequence: u64,
    latest_ms: Option<u64>,
    last_attempt: Instant,
    failures: u32,
    truncated: bool,
    view: SummaryView,
}

struct Batch {
    sequence: u64,
    covered_through_ms: Option<u64>,
    excerpts: Vec<(String, Option<u64>)>,
    truncated: bool,
}

impl State {
    fn observe(&mut self, event: &StreamEvent) {
        let StreamEvent::Transcript(event) = event else {
            return;
        };
        if event.session_id != self.view.session_id {
            return;
        }
        let tokens: Vec<_> = event
            .tokens
            .iter()
            .filter(|token| {
                token.is_final
                    && token.translation_status != TranslationStatus::Translation
                    && !matches!(token.text.as_str(), "<end>" | "<fin>")
            })
            .collect();
        let text: String = tokens.iter().map(|token| token.text.as_str()).collect();
        if text.trim().is_empty() {
            return;
        }
        let count = text.chars().count();
        // Keep the most recent material when a single unusually large frame arrives.
        let text = if count > MAX_PENDING_CHARS {
            self.truncated = true;
            text.chars().skip(count - MAX_PENDING_CHARS).collect()
        } else {
            text
        };
        self.sequence += 1;
        self.latest_ms = self
            .latest_ms
            .into_iter()
            .chain(
                tokens
                    .iter()
                    .filter_map(|token| token.end_ms.or(token.start_ms)),
            )
            .max();
        self.pending_chars += text.chars().count();
        self.pending.push_back(Excerpt {
            sequence: self.sequence,
            text,
            start_ms: (count <= MAX_PENDING_CHARS)
                .then(|| tokens.first().and_then(|token| token.start_ms))
                .flatten(),
        });
        while self.pending_chars > MAX_PENDING_CHARS || self.pending.len() > 256 {
            if let Some(old) = self.pending.pop_front() {
                self.pending_chars -= old.text.chars().count();
                self.truncated = true;
            }
        }
    }

    fn begin(&mut self, now: Instant) -> Option<Batch> {
        let elapsed = now.saturating_duration_since(self.last_attempt);
        let interval = UPDATE_INTERVAL * (1 << self.failures.min(3));
        if self.view.phase == "updating"
            || self.pending_chars < 20
            || elapsed < interval
            || (self.pending_chars < 120 && elapsed < Duration::from_secs(60))
        {
            return None;
        }
        let mut excerpts = Vec::new();
        if let Some(previous) = &self.view.content {
            excerpts.extend(previous.timeline.iter().map(|point| {
                (
                    format!(
                        "Earlier machine-summarized meeting point: {}. {} {}",
                        point.title,
                        point.summary,
                        point.details.join(" ")
                    ),
                    point.start_ms,
                )
            }));
        }
        excerpts.extend(
            self.pending
                .iter()
                .map(|source| (source.text.clone(), source.start_ms)),
        );
        self.last_attempt = now;
        self.view.phase = "updating";
        self.view.error = None;
        Some(Batch {
            sequence: self.sequence,
            covered_through_ms: self.latest_ms,
            excerpts,
            truncated: self.truncated,
        })
    }

    fn complete(
        &mut self,
        batch: &Batch,
        result: Result<RecordInsights, String>,
        source_times: Vec<Option<u64>>,
    ) {
        match result {
            Ok(insights) => {
                while self
                    .pending
                    .front()
                    .is_some_and(|source| source.sequence <= batch.sequence)
                {
                    if let Some(source) = self.pending.pop_front() {
                        self.pending_chars -= source.text.chars().count();
                    }
                }
                self.view.content = Some(SummaryContent {
                    title: insights.title,
                    notes: insights.notes.unwrap_or_default(),
                    timeline: insights.timeline,
                    diagram: insights.diagram,
                    source_times,
                    covered_through_ms: batch.covered_through_ms,
                    truncated: batch.truncated,
                });
                self.view.revision += 1;
                self.view.phase = "ready";
                self.failures = 0;
            }
            Err(error) => {
                self.view.phase = "error";
                self.view.error = Some(error);
                self.failures = self.failures.saturating_add(1);
            }
        }
    }
}

#[derive(Clone)]
pub struct LiveSummary {
    state: Arc<Mutex<State>>,
    publish: Arc<Publish>,
}

impl LiveSummary {
    pub fn new(session_id: &str, publish: impl Fn(&SummaryView) + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                pending: VecDeque::new(),
                pending_chars: 0,
                sequence: 0,
                latest_ms: None,
                last_attempt: Instant::now(),
                failures: 0,
                truncated: false,
                view: SummaryView {
                    session_id: session_id.into(),
                    phase: "waiting",
                    revision: 0,
                    content: None,
                    error: None,
                },
            })),
            publish: Arc::new(publish),
        }
    }

    pub fn observe(&self, event: &StreamEvent) {
        if let Ok(mut state) = self.state.lock() {
            state.observe(event);
        }
    }

    pub fn content(&self) -> Option<SummaryContent> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.view.content.clone())
    }

    pub fn unavailable(&self, error: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.view.phase = "unavailable";
            state.view.error = Some(error.into());
        }
        self.emit();
    }

    fn emit(&self) {
        let view = self.state.lock().ok().map(|state| state.view.clone());
        if let Some(view) = view {
            (self.publish)(&view);
        }
    }

    pub async fn run(
        &self,
        endpoint: &str,
        auth: HeaderValue,
        language: Language,
        stopped: Arc<AtomicBool>,
        account: impl Fn(Option<Tokens>),
    ) {
        self.emit();
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;
            if stopped.load(Ordering::Acquire) {
                break;
            }
            let batch = self
                .state
                .lock()
                .ok()
                .and_then(|mut state| state.begin(Instant::now()));
            let Some(batch) = batch else { continue };
            self.emit();
            let input = Input::from_excerpts(&batch.excerpts, batch.truncated);
            let (result, source_times) = match input {
                Ok(input) => {
                    let times = input.source_times();
                    let (result, usage) =
                        record_ai::summarize_live(endpoint, auth.clone(), input, language).await;
                    account(usage);
                    (result, times)
                }
                Err(error) => (Err(error), Vec::new()),
            };
            if let Ok(mut state) = self.state.lock() {
                state.complete(&batch, result, source_times);
            }
            self.emit();
        }
        if let Ok(mut state) = self.state.lock() {
            state.view.phase = "stopped";
        }
        self.emit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(text: &str, final_text: bool, lane: &str, session: &str, ms: u64) -> StreamEvent {
        StreamEvent::Transcript(
            serde_json::from_value(json!({
                "session_id":session, "finished":false,
                "tokens":[{"text":text,"is_final":final_text,"translation_status":lane,
                    "start_ms":ms,"end_ms":ms + 1000}]
            }))
            .unwrap(),
        )
    }

    fn insights(label: &str) -> RecordInsights {
        RecordInsights {
            title: label.into(),
            notes: Some(format!("{label} [S1]")),
            timeline: vec![TimelinePoint {
                start_ms: Some(1000),
                title: label.into(),
                summary: "Supported point".into(),
                details: vec![],
            }],
            diagram: None,
        }
    }

    #[test]
    fn only_confirmed_originals_from_the_current_session_are_summarized() {
        let worker = LiveSummary::new("meeting", |_| {});
        for item in [
            event("draft", false, "original", "meeting", 0),
            event("译文", true, "translation", "meeting", 0),
            event("other", true, "original", "other", 0),
            event("<end>", true, "original", "meeting", 0),
        ] {
            worker.observe(&item);
        }
        let mut state = worker.state.lock().unwrap();
        assert!(state.pending.is_empty());
        state.observe(&event(
            &"original ".repeat(20),
            true,
            "original",
            "meeting",
            1000,
        ));
        let start = state.last_attempt;
        assert!(state.begin(start + Duration::from_secs(29)).is_none());
        let batch = state.begin(start + UPDATE_INTERVAL).unwrap();
        assert_eq!(batch.excerpts.len(), 1);
        assert!(batch.excerpts[0].0.starts_with("original"));
        assert_eq!(batch.covered_through_ms, Some(2000));
        assert!(state.begin(start + Duration::from_secs(120)).is_none());
    }

    #[test]
    fn updates_merge_previous_points_and_preserve_speech_arriving_during_a_request() {
        let worker = LiveSummary::new("meeting", |_| {});
        let mut state = worker.state.lock().unwrap();
        state.observe(&event(
            &"First. ".repeat(30),
            true,
            "original",
            "meeting",
            1000,
        ));
        let now = state.last_attempt + UPDATE_INTERVAL;
        let first = state.begin(now).unwrap();
        state.observe(&event(
            &"New. ".repeat(40),
            true,
            "original",
            "meeting",
            3000,
        ));
        state.complete(&first, Ok(insights("First point")), vec![Some(1000)]);
        assert_eq!(state.view.revision, 1);
        assert_eq!(state.pending.len(), 1);
        let next = state.begin(now + UPDATE_INTERVAL).unwrap();
        assert_eq!(next.excerpts.len(), 2);
        assert!(next.excerpts[0].0.contains("Earlier machine-summarized"));
        assert!(next.excerpts[0].0.contains("First point"));
        assert!(next.excerpts[1].0.starts_with("New."));
        state.complete(
            &next,
            Ok(insights("Combined")),
            vec![Some(1000), Some(3000)],
        );
        assert!(state.pending.is_empty());
        assert!(state.begin(now + UPDATE_INTERVAL * 10).is_none());
    }

    #[test]
    fn errors_keep_the_last_summary_and_back_off_before_retrying() {
        let worker = LiveSummary::new("meeting", |_| {});
        let mut state = worker.state.lock().unwrap();
        state.observe(&event(
            &"First ".repeat(30),
            true,
            "original",
            "meeting",
            1000,
        ));
        let now = state.last_attempt + UPDATE_INTERVAL;
        let first = state.begin(now).unwrap();
        state.complete(&first, Ok(insights("Keep this")), vec![Some(1000)]);
        state.observe(&event(
            &"More ".repeat(30),
            true,
            "original",
            "meeting",
            2000,
        ));
        let next_time = now + UPDATE_INTERVAL;
        let next = state.begin(next_time).unwrap();
        state.complete(&next, Err("rate limited".into()), Vec::new());
        assert_eq!(state.view.phase, "error");
        assert_eq!(state.view.content.as_ref().unwrap().title, "Keep this");
        assert!(!state.pending.is_empty());
        assert!(state.begin(next_time + UPDATE_INTERVAL).is_none());
        assert!(state.begin(next_time + UPDATE_INTERVAL * 2).is_some());
    }

    #[test]
    fn short_meetings_wait_for_context_and_high_volume_input_stays_bounded() {
        let worker = LiveSummary::new("meeting", |_| {});
        let mut state = worker.state.lock().unwrap();
        state.observe(&event(
            "A short but meaningful point.",
            true,
            "original",
            "meeting",
            9000,
        ));
        let now = state.last_attempt;
        assert!(state.begin(now + UPDATE_INTERVAL).is_none());
        assert!(state.begin(now + Duration::from_secs(60)).is_some());
        state.view.phase = "ready";
        state.observe(&event(
            &"原文".repeat(MAX_PENDING_CHARS),
            true,
            "original",
            "meeting",
            1000,
        ));
        assert!(state.pending_chars <= MAX_PENDING_CHARS);
        assert!(state.truncated);
        assert_eq!(state.latest_ms, Some(10_000));
        assert_eq!(state.pending.back().unwrap().start_ms, None);
        for _ in 0..300 {
            state.observe(&event("x", true, "original", "meeting", 11_000));
        }
        assert!(state.pending.len() <= 256);
        assert!(state.pending_chars <= MAX_PENDING_CHARS);
    }

    #[tokio::test]
    async fn worker_sends_provisional_source_backed_requests_and_accounts_before_publishing() {
        use std::{
            io::{BufRead, BufReader, Read, Write},
            net::TcpListener,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("POST /v1/responses "));
            let mut length = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let generated = json!({"title":"AST construction", "notes":"Parser builds the AST [S1]",
                "timeline":[{"sourceId":1,"title":"Parser","summary":"Builds the AST","details":[]}],
                "diagram":{"nodes":[],"edges":[]}});
            let body = json!({"status":"completed", "output":[{"type":"message","role":"assistant",
                "content":[{"type":"output_text","text":generated.to_string()}]}],
                "usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":0},"output_tokens":30}}).to_string();
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        });
        let stopped = Arc::new(AtomicBool::new(false));
        let published = Arc::new(Mutex::new(Vec::new()));
        let sink = published.clone();
        let stop_after_result = stopped.clone();
        let worker = LiveSummary::new("meeting", move |view| {
            if view.phase == "ready" || view.phase == "error" {
                stop_after_result.store(true, Ordering::Release);
            }
            sink.lock().unwrap().push(view.clone());
        });
        worker.observe(&event(
            &"The parser builds an AST. ".repeat(8),
            true,
            "original",
            "meeting",
            1000,
        ));
        worker.state.lock().unwrap().last_attempt = Instant::now() - Duration::from_secs(60);
        let usage = Mutex::new(Vec::new());
        worker
            .run(
                &endpoint,
                HeaderValue::from_static("Bearer fixture"),
                Language::Zh,
                stopped,
                |tokens| usage.lock().unwrap().push(tokens),
            )
            .await;
        let request = server.join().unwrap();
        let input: serde_json::Value =
            serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
        assert_eq!(input["meeting_in_progress"], true);
        assert_eq!(input["target_language"], "Simplified Chinese");
        assert_eq!(input["sources"][0]["timestamp"], "00:01");
        assert!(
            input["sources"][0]["text"]
                .as_str()
                .unwrap()
                .contains("parser")
        );
        assert_eq!(usage.lock().unwrap()[0].unwrap().input, 100);
        let published = published.lock().unwrap();
        assert_eq!(published.last().unwrap().phase, "stopped");
        let result = published.iter().find(|view| view.phase == "ready").unwrap();
        assert_eq!(result.revision, 1);
        assert_eq!(
            result.content.as_ref().unwrap().timeline[0].start_ms,
            Some(1000)
        );
    }

    #[tokio::test]
    async fn stopped_workers_never_make_requests_or_charge_usage() {
        let updates = Arc::new(Mutex::new(Vec::new()));
        let sink = updates.clone();
        let worker = LiveSummary::new("meeting", move |view| {
            sink.lock().unwrap().push(view.clone())
        });
        worker.observe(&event(&"Text ".repeat(50), true, "original", "meeting", 0));
        worker
            .run(
                "http://127.0.0.1:1",
                HeaderValue::from_static("Bearer fixture"),
                Language::Zh,
                Arc::new(AtomicBool::new(true)),
                |_| panic!("no request should be made"),
            )
            .await;
        assert_eq!(updates.lock().unwrap().last().unwrap().phase, "stopped");
    }
}
