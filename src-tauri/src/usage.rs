use crate::transcript::{Provider, StreamEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

const AUDIO_PER_MINUTE: f64 = 0.017;
const INPUT_PER_MILLION: f64 = 0.10;
const CACHED_PER_MILLION: f64 = 0.01;
const OUTPUT_PER_MILLION: f64 = 0.50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub(crate) input: u64,
    pub(crate) cached: u64,
    pub(crate) output: u64,
}

impl Tokens {
    pub fn from_response(response: &Value) -> Option<Self> {
        let usage = &response["usage"];
        let input = usage["input_tokens"].as_u64()?;
        let cached = usage["input_tokens_details"]["cached_tokens"].as_u64()?;
        let output = usage["output_tokens"].as_u64()?;
        (cached <= input).then_some(Self {
            input,
            cached,
            output,
        })
    }

    pub(crate) fn add(self, other: Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_add(other.input)?,
            cached: self.cached.checked_add(other.cached)?,
            output: self.output.checked_add(other.output)?,
        })
    }

    pub(crate) fn estimate(self) -> f64 {
        ((self.input - self.cached) as f64 * INPUT_PER_MILLION
            + self.cached as f64 * CACHED_PER_MILLION
            + self.output as f64 * OUTPUT_PER_MILLION)
            / 1_000_000.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    session_id: String,
    provider: Provider,
    audio_ms: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    estimated_usd: Option<f64>,
    transcription_usd: Option<f64>,
    translation_usd: Option<f64>,
    #[serde(default)]
    analysis_usd: Option<f64>,
    pending_translation_requests: u32,
    incomplete: bool,
}

struct State {
    samples: u64,
    tokens: Tokens,
    analysis: Option<Tokens>,
    analysis_requested: bool,
    pending: u32,
    incomplete: bool,
    last_update: Instant,
}

impl State {
    fn snapshot(&self, session_id: &str, provider: Provider) -> Snapshot {
        let rate = u64::from(match provider {
            Provider::Openai => crate::openai::SAMPLE_RATE,
            Provider::Soniox => crate::soniox::SAMPLE_RATE,
        });
        let supported = provider == Provider::Openai;
        let transcription_usd = self.samples as f64 / rate as f64 / 60.0 * AUDIO_PER_MINUTE;
        let translation_usd = self.tokens.estimate();
        let mut snapshot = Snapshot {
            session_id: session_id.into(),
            provider,
            audio_ms: self.samples.saturating_mul(1000) / rate,
            input_tokens: self.tokens.input,
            cached_input_tokens: self.tokens.cached,
            output_tokens: self.tokens.output,
            estimated_usd: supported.then_some(transcription_usd + translation_usd),
            transcription_usd: supported.then_some(transcription_usd),
            translation_usd: supported.then_some(translation_usd),
            analysis_usd: None,
            pending_translation_requests: self.pending,
            incomplete: self.incomplete,
        };
        if self.analysis_requested {
            snapshot.add_analysis(self.analysis);
        }
        snapshot
    }

    fn record_analysis(&mut self, tokens: Option<Tokens>) {
        self.analysis_requested = true;
        match tokens.and_then(|tokens| self.analysis.unwrap_or_default().add(tokens)) {
            Some(total) => self.analysis = Some(total),
            None => self.incomplete = true,
        }
    }

    fn record(&mut self, event: &StreamEvent) -> bool {
        match event {
            StreamEvent::AudioSent(samples) => {
                if let Some(samples) = self.samples.checked_add(*samples as u64) {
                    self.samples = samples;
                } else {
                    self.incomplete = true;
                }
                self.last_update.elapsed() >= Duration::from_secs(1)
            }
            StreamEvent::TranslationStarted => {
                self.pending += 1;
                true
            }
            StreamEvent::TranslationUsage(usage) => {
                self.pending = self.pending.saturating_sub(1);
                match usage.and_then(|tokens| self.tokens.add(tokens)) {
                    Some(tokens) => self.tokens = tokens,
                    None => self.incomplete = true,
                }
                true
            }
            _ => false,
        }
    }
}

impl Snapshot {
    pub fn for_record_analysis(id: i64, provider: Option<Provider>) -> Self {
        Self {
            session_id: format!("record-{id}"),
            provider: provider.unwrap_or(Provider::Openai),
            audio_ms: 0,
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            estimated_usd: None,
            transcription_usd: None,
            translation_usd: None,
            analysis_usd: None,
            pending_translation_requests: 0,
            incomplete: false,
        }
    }

    pub fn add_analysis(&mut self, tokens: Option<Tokens>) {
        self.incomplete |= self.provider == Provider::Soniox;
        let previous = Tokens {
            input: self.input_tokens,
            cached: self.cached_input_tokens,
            output: self.output_tokens,
        };
        let Some((tokens, total)) =
            tokens.and_then(|tokens| previous.add(tokens).map(|total| (tokens, total)))
        else {
            self.incomplete = true;
            return;
        };
        self.input_tokens = total.input;
        self.cached_input_tokens = total.cached;
        self.output_tokens = total.output;
        let amount = tokens.estimate();
        self.analysis_usd = Some(self.analysis_usd.unwrap_or(0.0) + amount);
        self.estimated_usd = Some(self.estimated_usd.unwrap_or(0.0) + amount);
    }
}

#[derive(Clone)]
pub struct Meter {
    app: AppHandle,
    provider: Provider,
    session_id: Arc<str>,
    state: Arc<Mutex<State>>,
}

impl Meter {
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.state
            .lock()
            .ok()
            .map(|state| state.snapshot(&self.session_id, self.provider))
    }

    pub fn new(app: AppHandle, provider: Provider, session_id: &str) -> Self {
        let meter = Self {
            app,
            provider,
            session_id: Arc::from(session_id),
            state: Arc::new(Mutex::new(State {
                samples: 0,
                tokens: Tokens::default(),
                analysis: None,
                analysis_requested: false,
                pending: 0,
                incomplete: false,
                last_update: Instant::now(),
            })),
        };
        if let Ok(mut state) = meter.state.lock() {
            meter.emit(&mut state);
        }
        meter
    }

    pub fn record(&self, event: &StreamEvent) {
        if let Ok(mut state) = self.state.lock()
            && state.record(event)
        {
            self.emit(&mut state);
        }
    }

    pub fn record_analysis(&self, tokens: Option<Tokens>) {
        if let Ok(mut state) = self.state.lock() {
            state.record_analysis(tokens);
            self.emit(&mut state);
        }
    }

    pub fn finish(&self, incomplete: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.incomplete |= incomplete || state.pending > 0;
            state.pending = 0;
            self.emit(&mut state);
        }
    }

    fn emit(&self, state: &mut State) {
        let snapshot = state.snapshot(&self.session_id, self.provider);
        let _ = self.app.emit("session-usage", &snapshot);
        state.last_update = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> State {
        State {
            samples: 0,
            tokens: Tokens::default(),
            analysis: None,
            analysis_requested: false,
            pending: 0,
            incomplete: false,
            last_update: Instant::now(),
        }
    }

    #[test]
    fn sums_submitted_samples_and_prices_cached_input_once() {
        let mut state = state();
        for _ in 0..24 {
            state.record(&StreamEvent::AudioSent(60_000));
        }
        state.record(&StreamEvent::TranslationStarted);
        assert_eq!(
            state
                .snapshot("test", Provider::Openai)
                .pending_translation_requests,
            1
        );
        let tokens = Tokens::from_response(&json!({
            "usage": {"input_tokens": 1_000_000, "input_tokens_details": {"cached_tokens": 500_000}, "output_tokens": 500_000}
        })).unwrap();
        state.record(&StreamEvent::TranslationUsage(Some(tokens)));
        let snapshot = state.snapshot("test", Provider::Openai);
        assert_eq!(snapshot.audio_ms, 60_000);
        assert_eq!(snapshot.transcription_usd, Some(0.017));
        assert!((snapshot.translation_usd.unwrap() - 0.305).abs() < 1e-12);
        assert!((snapshot.estimated_usd.unwrap() - 0.322).abs() < 1e-12);
        assert_eq!(snapshot.pending_translation_requests, 0);
        assert!(!snapshot.incomplete);
    }

    #[test]
    fn live_summary_usage_is_separate_from_translation_and_survives_snapshot_updates() {
        let mut state = state();
        let tokens = Tokens {
            input: 1000,
            cached: 100,
            output: 200,
        };
        state.record(&StreamEvent::TranslationStarted);
        state.record(&StreamEvent::TranslationUsage(Some(tokens)));
        let before = state.snapshot("meeting", Provider::Openai);
        state.record_analysis(Some(tokens));
        state.record_analysis(Some(tokens));
        state.record(&StreamEvent::AudioSent(24_000));
        let after = state.snapshot("meeting", Provider::Openai);
        assert_eq!(after.translation_usd, before.translation_usd);
        assert_eq!(after.analysis_usd, Some(tokens.estimate() * 2.0));
        assert_eq!(after.input_tokens, 3000);
        assert_eq!(after.output_tokens, 600);
        assert_eq!(after.pending_translation_requests, 0);
        assert!(!after.incomplete);
        assert!(
            (after.estimated_usd.unwrap()
                - after.transcription_usd.unwrap()
                - after.translation_usd.unwrap()
                - after.analysis_usd.unwrap())
            .abs()
                < 1e-12
        );
        state.record_analysis(None);
        let partial = state.snapshot("meeting", Provider::Openai);
        assert!(partial.incomplete);
        assert_eq!(partial.analysis_usd, after.analysis_usd);
        let soniox = state.snapshot("meeting", Provider::Soniox);
        assert!(soniox.incomplete);
        assert_eq!(soniox.estimated_usd, soniox.analysis_usd);
    }

    #[test]
    fn invalid_missing_or_overflowing_usage_stays_incomplete() {
        let mut state = state();
        let invalid = Tokens::from_response(&json!({
            "usage": {"input_tokens": 8, "input_tokens_details": {"cached_tokens": 9}, "output_tokens": 2}
        }));
        assert!(invalid.is_none());
        state.record(&StreamEvent::TranslationStarted);
        state.record(&StreamEvent::TranslationUsage(invalid));
        assert!(state.snapshot("test", Provider::Openai).incomplete);
        assert!(
            state
                .snapshot("test", Provider::Soniox)
                .estimated_usd
                .is_none()
        );
        let full = Tokens {
            input: u64::MAX,
            cached: 0,
            output: 0,
        };
        assert!(
            full.add(Tokens {
                input: 1,
                cached: 0,
                output: 0
            })
            .is_none()
        );
    }
}
