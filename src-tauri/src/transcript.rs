use serde::{Deserialize, Serialize};

/// App-facing event. Each batch contains newly confirmed tokens and the latest
/// draft snapshot. All timings are positions in the streamed audio.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TranscriptEvent {
    pub session_id: String,
    pub tokens: Vec<TranscriptToken>,
    pub finished: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TranscriptToken {
    pub text: String,
    pub is_final: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    pub translation_status: TranslationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation_source: Option<Provider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_segment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationStatus {
    #[default]
    None,
    Original,
    Translation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Soniox,
    #[default]
    Openai,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Soniox => "Soniox",
            Self::Openai => "OpenAI",
        }
    }
}

pub enum StreamEvent {
    Transcript(TranscriptEvent),
    AudioLevel(f32),
    TranslationWarning(String),
    AudioSent(usize),
    TranslationStarted,
    TranslationUsage(Option<crate::usage::Tokens>),
}
