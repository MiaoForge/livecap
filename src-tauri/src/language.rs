use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Zh,
    Ja,
    Ko,
    Fr,
    De,
    Es,
    Pt,
    It,
    Ru,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Fr => "fr",
            Self::De => "de",
            Self::Es => "es",
            Self::Pt => "pt",
            Self::It => "it",
            Self::Ru => "ru",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Zh => "Simplified Chinese",
            Self::Ja => "Japanese",
            Self::Ko => "Korean",
            Self::Fr => "French",
            Self::De => "German",
            Self::Es => "Spanish",
            Self::Pt => "Portuguese",
            Self::It => "Italian",
            Self::Ru => "Russian",
        }
    }

    pub fn supports_knowledge(self) -> bool {
        self == Self::Zh
    }
}
