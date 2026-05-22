use crate::models::downloads::{model_path, windows_descriptor_for};
use anyhow::{anyhow, Context};
use llama_cpp::{
    standard_sampler::{SamplerStage, StandardSampler},
    LlamaModel, LlamaParams, SessionParams,
};
use parrot_language::DictationLanguageMetadata;
use parrot_models::{ModelDescriptor, SamplerConfig};
use parrot_prompts::{assemble_cleanup_prompt, CleanupPrompt, CleanupPromptInput};
use parrot_protocol::{AppSettings, ModelRole, NativeCorePaths};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Default)]
pub struct LlamaCleanupPipeline {
    models: Arc<Mutex<HashMap<PathBuf, Arc<LlamaModel>>>>,
}

impl LlamaCleanupPipeline {
    pub fn warm(&self, settings: &AppSettings, cleanup_models_dir: &str) -> anyhow::Result<()> {
        if !settings.cleanup_enabled {
            return Ok(());
        }

        let Some((_descriptor, path)) = descriptor_and_path(settings, cleanup_models_dir)? else {
            return Ok(());
        };

        ensure_gguf_file(&path)?;
        let _ = self.model_for(&path)?;
        Ok(())
    }

    pub fn cleanup(
        &self,
        raw: &str,
        settings: &AppSettings,
        language: &DictationLanguageMetadata,
        cleanup_models_dir: &str,
        cleanup_prompt: &str,
        debug_cleanup_failures: bool,
    ) -> anyhow::Result<String> {
        self.cleanup_with_cancel(
            raw,
            settings,
            language,
            cleanup_models_dir,
            cleanup_prompt,
            debug_cleanup_failures,
            &Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn cleanup_with_cancel(
        &self,
        raw: &str,
        settings: &AppSettings,
        language: &DictationLanguageMetadata,
        cleanup_models_dir: &str,
        cleanup_prompt: &str,
        debug_cleanup_failures: bool,
        cancel_flag: &Arc<AtomicBool>,
    ) -> anyhow::Result<String> {
        if is_cancelled(cancel_flag) {
            return Err(anyhow!("Recording cancelled."));
        }

        if !settings.cleanup_enabled {
            return Ok(raw.trim().to_string());
        }

        match self.cleanup_inner(
            raw,
            settings,
            language,
            cleanup_models_dir,
            cleanup_prompt,
            cancel_flag,
        ) {
            Ok(cleaned) => Ok(cleaned),
            Err(error) if !debug_cleanup_failures && !is_cancelled(cancel_flag) => {
                eprintln!("Windows llama.cpp cleanup failed; using raw transcript: {error}");
                Ok(raw.trim().to_string())
            }
            Err(error) => Err(error),
        }
    }

    pub fn clear_cache(&self) {
        self.models
            .lock()
            .expect("llama.cpp cleanup model cache poisoned")
            .clear();
    }

    fn cleanup_inner(
        &self,
        raw: &str,
        settings: &AppSettings,
        language: &DictationLanguageMetadata,
        cleanup_models_dir: &str,
        cleanup_prompt: &str,
        cancel_flag: &Arc<AtomicBool>,
    ) -> anyhow::Result<String> {
        if is_cancelled(cancel_flag) {
            return Err(anyhow!("Recording cancelled."));
        }

        let Some((descriptor, path)) = descriptor_and_path(settings, cleanup_models_dir)? else {
            return Ok(raw.trim().to_string());
        };

        if !path.exists() {
            return Ok(raw.trim().to_string());
        }

        ensure_gguf_file(&path)?;

        let cleanup_rules = if settings.cleanup_prompt.trim().is_empty() {
            cleanup_prompt.to_string()
        } else {
            settings.cleanup_prompt.clone()
        };

        let prompt_format = descriptor
            .prompt_format
            .ok_or_else(|| anyhow!("cleanup model is missing prompt format"))?;

        let prompt = assemble_cleanup_prompt(&CleanupPromptInput {
            cleanup_rules,
            dictionary_entries: settings.dictionary_entries.clone(),
            raw_transcript: raw.to_string(),
            language: language.clone(),
            prompt_format,
            default_output_tokens: descriptor.output_tokens.unwrap_or(512),
        });

        let model = self.model_for(&path)?;
        let output = complete_with_model(&descriptor, model.as_ref(), &prompt, cancel_flag)?;
        let cleaned = parrot_cleanup::sanitize(&output);

        if cleaned.trim().is_empty() {
            return Err(anyhow!("Cleanup model produced an empty response."));
        }

        Ok(cleaned)
    }

    fn model_for(&self, path: &Path) -> anyhow::Result<Arc<LlamaModel>> {
        let key = path.to_path_buf();

        if let Some(model) = self
            .models
            .lock()
            .expect("llama.cpp cleanup model cache poisoned")
            .get(&key)
            .cloned()
        {
            return Ok(model);
        }

        let params = LlamaParams {
            n_gpu_layers: cleanup_gpu_layer_count(),
            ..LlamaParams::default()
        };

        let model = Arc::new(
            LlamaModel::load_from_file(path, params)
                .with_context(|| format!("failed to load cleanup model {}", path.display()))?,
        );

        let mut models = self
            .models
            .lock()
            .expect("llama.cpp cleanup model cache poisoned");
        let cached = models.entry(key).or_insert_with(|| model.clone()).clone();

        Ok(cached)
    }
}

fn complete_with_model(
    descriptor: &ModelDescriptor,
    model: &LlamaModel,
    prompt: &CleanupPrompt,
    cancel_flag: &Arc<AtomicBool>,
) -> anyhow::Result<String> {
    if is_cancelled(cancel_flag) {
        return Err(anyhow!("Recording cancelled."));
    }

    let context_tokens = descriptor.context_tokens.unwrap_or(2048).max(1) as u32;
    let max_output_tokens = prompt.max_output_tokens.max(1) as usize;

    let prompt_tokens = model
        .tokenize_bytes(prompt.full_prompt.as_bytes(), true, true)
        .context("failed to tokenize cleanup prompt")?;

    if prompt_tokens.len() + max_output_tokens >= context_tokens as usize {
        return Err(anyhow!(
            "Cleanup prompt is too long for the {context_tokens}-token context."
        ));
    }

    let threads = default_thread_count() as u32;
    let session_params = SessionParams {
        n_ctx: context_tokens,
        n_batch: context_tokens,
        n_threads: threads,
        n_threads_batch: threads,
        ..SessionParams::default()
    };

    let mut session = model
        .create_session(session_params)
        .context("failed to create llama.cpp cleanup session")?;

    session
        .advance_context_with_tokens(&prompt_tokens)
        .context("llama.cpp failed to evaluate cleanup prompt")?;

    if is_cancelled(cancel_flag) {
        return Err(anyhow!("Recording cancelled."));
    }

    let sampler = sampler_for(descriptor.sampler.as_ref());
    let handle = session
        .start_completing_with(sampler, max_output_tokens)
        .context("llama.cpp failed to start cleanup completion")?;

    let mut output = String::new();

    for piece in handle.into_strings() {
        if is_cancelled(cancel_flag) {
            return Err(anyhow!("Recording cancelled."));
        }
        output.push_str(&piece);
    }

    Ok(output)
}

fn sampler_for(config: Option<&SamplerConfig>) -> StandardSampler {
    let Some(config) = config else {
        return StandardSampler::new_greedy();
    };

    let mut stages = Vec::new();

    if config.repeat_penalty != 1.0
        || config.frequency_penalty != 0.0
        || config.presence_penalty != 0.0
    {
        stages.push(SamplerStage::RepetitionPenalty {
            repetition_penalty: config.repeat_penalty.max(0.0),
            frequency_penalty: config.frequency_penalty,
            presence_penalty: config.presence_penalty,
            last_n: 64,
        });
    }

    if config.top_k > 0 {
        stages.push(SamplerStage::TopK(config.top_k));
    }

    if config.top_p > 0.0 && config.top_p < 1.0 {
        stages.push(SamplerStage::TopP(config.top_p.clamp(0.0, 1.0)));
    }

    if config.min_p > 0.0 {
        stages.push(SamplerStage::MinP(config.min_p.clamp(0.0, 1.0)));
    }

    stages.push(SamplerStage::Temperature(
        config.temperature.clamp(0.001, 2.0),
    ));

    StandardSampler::new_softmax(stages, 1)
}

fn descriptor_and_path(
    settings: &AppSettings,
    cleanup_models_dir: &str,
) -> anyhow::Result<Option<(ModelDescriptor, PathBuf)>> {
    let Some(descriptor) = windows_descriptor_for(&settings.cleanup_model_id) else {
        return Ok(None);
    };

    if descriptor.role != ModelRole::Cleanup {
        return Ok(None);
    }

    let paths = NativeCorePaths {
        app_data_dir: String::new(),
        models_dir: String::new(),
        speech_models_dir: String::new(),
        cleanup_models_dir: cleanup_models_dir.to_string(),
        resources_dir: String::new(),
        shared_resources_dir: String::new(),
        temp_dir: String::new(),
    };

    let path = model_path(&descriptor, &paths)?;
    Ok(Some((descriptor, path)))
}

fn ensure_model_file(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(anyhow!("Model download required: {}", path.display()))
    }
}

fn ensure_gguf_file(path: &Path) -> anyhow::Result<()> {
    ensure_model_file(path)?;

    let mut file = File::open(path)
        .with_context(|| format!("failed to open cleanup model {}", path.display()))?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic)
        .with_context(|| format!("failed to read cleanup model {}", path.display()))?;

    parrot_models::validate_gguf_magic(&magic)
        .with_context(|| format!("{} is not a valid GGUF file", path.display()))?;

    Ok(())
}

fn default_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().clamp(1, 16))
        .unwrap_or(4)
}

fn is_cancelled(cancel_flag: &Arc<AtomicBool>) -> bool {
    cancel_flag.load(Ordering::SeqCst)
}

fn cleanup_gpu_layer_count() -> u32 {
    #[cfg(feature = "cuda")]
    {
        99
    }

    #[cfg(not(feature = "cuda"))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn language() -> DictationLanguageMetadata {
        DictationLanguageMetadata {
            mode: "selected".into(),
            code: Some("en".into()),
            locale: None,
            name: Some("English".into()),
        }
    }

    #[test]
    fn cleanup_disabled_returns_raw() {
        let pipeline = LlamaCleanupPipeline::default();
        let mut settings = AppSettings::default();
        settings.cleanup_enabled = false;

        let result = pipeline
            .cleanup(
                " hello world ",
                &settings,
                &language(),
                "unused",
                "Clean it.",
                true,
            )
            .unwrap();

        assert_eq!(result, "hello world");
    }

    #[test]
    fn missing_cleanup_model_falls_back_to_raw() {
        let temp = TempDir::new().unwrap();
        let pipeline = LlamaCleanupPipeline::default();
        let settings = AppSettings::default();

        let result = pipeline
            .cleanup(
                "hello world",
                &settings,
                &language(),
                temp.path().to_str().unwrap(),
                "Clean it.",
                false,
            )
            .unwrap();

        assert_eq!(result, "hello world");
    }

    #[test]
    fn corrupt_cleanup_model_falls_back_to_raw_when_debug_is_disabled() {
        let temp = TempDir::new().unwrap();
        let pipeline = LlamaCleanupPipeline::default();
        let settings = AppSettings::default();
        let cleanup_models_dir = temp.path().to_str().unwrap();
        let (_descriptor, path) = descriptor_and_path(&settings, cleanup_models_dir)
            .unwrap()
            .unwrap();
        std::fs::write(&path, b"NOPE").unwrap();

        let result = pipeline
            .cleanup(
                "hello world",
                &settings,
                &language(),
                cleanup_models_dir,
                "Clean it.",
                false,
            )
            .unwrap();

        assert_eq!(result, "hello world");
    }

    #[test]
    fn corrupt_cleanup_model_surfaces_error_when_debug_is_enabled() {
        let temp = TempDir::new().unwrap();
        let pipeline = LlamaCleanupPipeline::default();
        let settings = AppSettings::default();
        let cleanup_models_dir = temp.path().to_str().unwrap();
        let (_descriptor, path) = descriptor_and_path(&settings, cleanup_models_dir)
            .unwrap()
            .unwrap();
        std::fs::write(&path, b"NOPE").unwrap();

        let error = pipeline
            .cleanup(
                "hello world",
                &settings,
                &language(),
                cleanup_models_dir,
                "Clean it.",
                true,
            )
            .unwrap_err();

        assert!(error.to_string().contains("valid GGUF"));
    }

    #[test]
    fn cancellation_is_not_treated_as_cleanup_fallback() {
        let temp = TempDir::new().unwrap();
        let pipeline = LlamaCleanupPipeline::default();
        let settings = AppSettings::default();
        let cancel_flag = Arc::new(AtomicBool::new(true));

        let error = pipeline
            .cleanup_with_cancel(
                "hello world",
                &settings,
                &language(),
                temp.path().to_str().unwrap(),
                "Clean it.",
                false,
                &cancel_flag,
            )
            .unwrap_err();

        assert!(error.to_string().contains("Recording cancelled"));
    }

    #[test]
    fn default_thread_count_stays_in_supported_range() {
        let count = default_thread_count();
        assert!((1..=16).contains(&count));
    }
}
