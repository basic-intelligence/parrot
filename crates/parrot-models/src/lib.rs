use anyhow::{anyhow, Context};
use parrot_language::{speech_model_slot, DictationLanguageSettings, SpeechModelSlot};
use parrot_protocol::{ModelRole, ModelStatus};
use serde::{Deserialize, Serialize};

pub const MODELS_JSON: &str = include_str!("../../../native-core/shared/models.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogManifest {
    pub default_cleanup_public_id: String,
    pub models: Vec<ModelDescriptor>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Architecture {
    AppleSilicon,
    Intel,
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelRuntime {
    Whisperkit,
    Whispercpp,
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CleanupPromptFormat {
    Qwen3Chatml,
    Gemma4Turns,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SamplerConfig {
    pub top_k: i32,
    pub top_p: f32,
    pub min_p: f32,
    pub temperature: f32,
    pub repeat_penalty: f32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub public_id: String,
    pub concrete_id: String,
    pub role: ModelRole,
    pub runtime: ModelRuntime,
    #[serde(default)]
    pub speech_slot: Option<SpeechModelSlot>,
    pub platforms: Vec<Platform>,
    pub architectures: Vec<Architecture>,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    pub display_name: String,
    pub subtitle: String,
    pub expected_bytes: i64,
    #[serde(default)]
    pub sha256: Option<String>,
    pub license: String,
    #[serde(default)]
    pub prompt_format: Option<CleanupPromptFormat>,
    #[serde(default)]
    pub sampler: Option<SamplerConfig>,
    #[serde(default)]
    pub context_tokens: Option<i32>,
    #[serde(default)]
    pub output_tokens: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelDownloadStatus {
    Downloaded,
    Downloading,
    Error,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelFileState {
    pub local_bytes: i64,
    pub downloading: bool,
    pub progress_bytes: i64,
    pub progress_total_bytes: i64,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn catalog() -> ModelCatalogManifest {
    serde_json::from_str(MODELS_JSON).expect("shared model catalog must be valid JSON")
}

pub fn validate_catalog(manifest: &ModelCatalogManifest) -> anyhow::Result<()> {
    if manifest.default_cleanup_public_id.trim().is_empty() {
        return Err(anyhow!("default cleanup public ID is empty"));
    }

    if cleanup_model_for(&manifest.default_cleanup_public_id).is_none() {
        return Err(anyhow!(
            "default cleanup model `{}` does not exist",
            manifest.default_cleanup_public_id
        ));
    }

    for model in &manifest.models {
        if model.public_id.trim().is_empty() {
            return Err(anyhow!("model `{}` has empty public ID", model.concrete_id));
        }
        if model.expected_bytes <= 0 {
            return Err(anyhow!(
                "model `{}` expected bytes must be positive",
                model.public_id
            ));
        }
        if model.role == ModelRole::Cleanup {
            if model.prompt_format.is_none() {
                return Err(anyhow!(
                    "cleanup model `{}` is missing prompt format",
                    model.public_id
                ));
            }
            if model.sampler.is_none() {
                return Err(anyhow!(
                    "cleanup model `{}` is missing sampler config",
                    model.public_id
                ));
            }
        }
        if matches!(
            model.runtime,
            ModelRuntime::Whispercpp | ModelRuntime::LlamaCpp
        ) {
            if model.repo_id.as_deref().unwrap_or_default().is_empty()
                || model.file_name.as_deref().unwrap_or_default().is_empty()
            {
                return Err(anyhow!(
                    "download model `{}` is missing repo or filename",
                    model.public_id
                ));
            }
        }
    }

    Ok(())
}

pub fn speech_model_for(
    slot: SpeechModelSlot,
    platform: Platform,
    architecture: Architecture,
) -> Option<ModelDescriptor> {
    catalog().models.into_iter().find(|model| {
        model.role == ModelRole::Speech
            && model.speech_slot == Some(slot)
            && model.platforms.contains(&platform)
            && model.architectures.contains(&architecture)
    })
}

pub fn cleanup_model_for(public_id: &str) -> Option<ModelDescriptor> {
    catalog()
        .models
        .into_iter()
        .find(|model| model.role == ModelRole::Cleanup && model.public_id == public_id)
}

pub fn cleanup_models() -> Vec<ModelDescriptor> {
    catalog()
        .models
        .into_iter()
        .filter(|model| model.role == ModelRole::Cleanup)
        .collect()
}

pub fn model_by_public_id(id: &str) -> Option<ModelDescriptor> {
    catalog()
        .models
        .into_iter()
        .find(|model| model.public_id == id)
}

pub fn required_models(
    settings: &DictationLanguageSettings,
    platform: Platform,
    architecture: Architecture,
) -> Vec<ModelDescriptor> {
    let speech = speech_model_for(speech_model_slot(settings), platform, architecture);
    let cleanup = cleanup_model_for(&settings.cleanup_model_id)
        .or_else(|| cleanup_model_for(&catalog().default_cleanup_public_id));

    speech.into_iter().chain(cleanup).collect()
}

pub fn model_status(
    descriptor: &ModelDescriptor,
    state: ModelFileState,
    required: bool,
) -> ModelStatus {
    let downloaded = !state.downloading && state.local_bytes > 0 && state.error.is_none();
    let progress_total_bytes = state
        .progress_total_bytes
        .max(state.local_bytes)
        .max(descriptor.expected_bytes)
        .max(1);

    ModelStatus {
        id: descriptor.public_id.clone(),
        role: descriptor.role.clone(),
        display_name: descriptor.display_name.clone(),
        subtitle: descriptor.subtitle.clone(),
        expected_bytes: descriptor.expected_bytes,
        local_bytes: state.local_bytes,
        progress_bytes: if downloaded {
            state.local_bytes
        } else {
            state.progress_bytes
        },
        progress_total_bytes,
        downloaded,
        downloading: state.downloading,
        required,
        error: state.error,
    }
}

pub fn cleanup_model_file_name(public_id: &str) -> Option<String> {
    cleanup_model_for(public_id).and_then(|model| model.file_name)
}

pub fn cleanup_temp_file_name(public_id: &str) -> Option<String> {
    cleanup_model_file_name(public_id).map(|file_name| format!("{file_name}.download"))
}

pub fn legacy_cleanup_file_names(public_id: &str) -> Vec<&'static str> {
    match public_id {
        "cleanup" => vec![
            "Qwen3-1.7B-Q5_K_M.gguf",
            "Qwen3-1.7B-Q5_K_M.gguf.download",
            "Qwen3-1.7B-Q8_0.gguf",
            "Qwen3-1.7B-Q8_0.gguf.download",
        ],
        _ => Vec::new(),
    }
}

pub fn validate_gguf_magic(bytes: &[u8]) -> anyhow::Result<()> {
    bytes
        .get(..4)
        .filter(|magic| *magic == b"GGUF")
        .map(|_| ())
        .context("not a valid GGUF file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::DictationLanguageMode;

    #[test]
    fn manifest_is_valid() {
        let manifest = catalog();
        validate_catalog(&manifest).unwrap();
    }

    #[test]
    fn cleanup_models_have_prompt_formats() {
        for model in cleanup_models() {
            assert!(model.prompt_format.is_some(), "{}", model.public_id);
        }
    }

    #[test]
    fn download_models_have_repo_and_file_names() {
        for model in catalog().models {
            if matches!(
                model.runtime,
                ModelRuntime::Whispercpp | ModelRuntime::LlamaCpp
            ) {
                assert!(model.repo_id.is_some(), "{}", model.public_id);
                assert!(model.file_name.is_some(), "{}", model.public_id);
            }
        }
    }

    #[test]
    fn apple_silicon_and_intel_speech_routing_still_work() {
        let apple = speech_model_for(
            SpeechModelSlot::Speech,
            Platform::Macos,
            Architecture::AppleSilicon,
        )
        .unwrap();
        let intel = speech_model_for(
            SpeechModelSlot::Speech,
            Platform::Macos,
            Architecture::Intel,
        )
        .unwrap();

        assert_eq!(apple.public_id, "speech");
        assert_eq!(apple.concrete_id, "whisperkit-openai-whisper-small-en");
        assert_eq!(intel.public_id, "speech");
        assert_eq!(intel.file_name.as_deref(), Some("ggml-small.en-q5_1.bin"));
    }

    #[test]
    fn required_models_follow_language_settings() {
        let settings = DictationLanguageSettings {
            dictation_language_mode: DictationLanguageMode::Specific,
            dictation_language_code: Some("es".into()),
            cleanup_model_id: "cleanup-gemma-4-e2b".into(),
        };

        let required = required_models(&settings, Platform::Macos, Architecture::AppleSilicon);
        let ids = required
            .iter()
            .map(|model| model.public_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["speech-multilingual", "cleanup-gemma-4-e2b"]);
    }

    #[test]
    fn gguf_magic_validation_is_future_ready() {
        assert!(validate_gguf_magic(b"GGUFrest").is_ok());
        assert!(validate_gguf_magic(b"NOPE").is_err());
    }
}
