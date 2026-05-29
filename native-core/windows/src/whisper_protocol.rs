use parrot_language::{DictationLanguageMetadata, DictationLanguageSettings};
use serde::{Deserialize, Serialize};

pub const METHOD_WARM: &str = "warm";
pub const METHOD_TRANSCRIBE: &str = "transcribe";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    pub text: String,
    pub language: DictationLanguageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperHelperRequest {
    pub id: String,
    pub method: String,
    pub model_path: String,
    #[serde(default)]
    pub audio_path: Option<String>,
    #[serde(default)]
    pub language_settings: Option<DictationLanguageSettings>,
}

#[allow(dead_code)]
impl WhisperHelperRequest {
    pub fn warm(id: String, model_path: String) -> Self {
        Self {
            id,
            method: METHOD_WARM.into(),
            model_path,
            audio_path: None,
            language_settings: None,
        }
    }

    pub fn transcribe(
        id: String,
        model_path: String,
        audio_path: String,
        language_settings: DictationLanguageSettings,
    ) -> Self {
        Self {
            id,
            method: METHOD_TRANSCRIBE.into(),
            model_path,
            audio_path: Some(audio_path),
            language_settings: Some(language_settings),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperHelperResponse {
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Option<Transcription>,
    #[serde(default)]
    pub error: Option<String>,
}

#[allow(dead_code)]
impl WhisperHelperResponse {
    pub fn success(id: impl Into<String>, result: Option<Transcription>) -> Self {
        Self {
            id: id.into(),
            ok: true,
            result,
            error: None,
        }
    }

    pub fn error(id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }
}
