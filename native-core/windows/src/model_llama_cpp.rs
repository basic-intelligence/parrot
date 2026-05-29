use crate::models::downloads::{model_path, windows_descriptor_for};
use anyhow::{anyhow, Context};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
    token::LlamaToken,
    TokenToStringError,
};
use parrot_language::DictationLanguageMetadata;
use parrot_models::{ModelDescriptor, SamplerConfig};
use parrot_prompts::{assemble_cleanup_prompt, CleanupPrompt, CleanupPromptInput};
use parrot_protocol::{AppSettings, ModelRole, NativeCorePaths};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};

#[derive(Clone, Default)]
pub struct LlamaCleanupPipeline {
    models: Arc<Mutex<HashMap<PathBuf, Arc<LoadedCleanupModel>>>>,
}

struct LoadedCleanupModel {
    model: LlamaModel,
    _backend: Arc<LlamaBackend>,
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

        let loaded = self.model_for(&path)?;
        let output = complete_with_model(&descriptor, &loaded, &prompt, cancel_flag)?;
        let cleaned = parrot_cleanup::sanitize(&output);

        if cleaned.trim().is_empty() {
            return Err(anyhow!("Cleanup model produced an empty response."));
        }

        Ok(cleaned)
    }

    fn model_for(&self, path: &Path) -> anyhow::Result<Arc<LoadedCleanupModel>> {
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

        let backend = llama_backend()?;
        let params = LlamaModelParams::default().with_n_gpu_layers(cleanup_gpu_layer_count());

        let model = Arc::new(LoadedCleanupModel {
            model: LlamaModel::load_from_file(backend.as_ref(), path, &params)
                .with_context(|| format!("failed to load cleanup model {}", path.display()))?,
            _backend: backend,
        });

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
    loaded: &LoadedCleanupModel,
    prompt: &CleanupPrompt,
    cancel_flag: &Arc<AtomicBool>,
) -> anyhow::Result<String> {
    if is_cancelled(cancel_flag) {
        return Err(anyhow!("Recording cancelled."));
    }

    let model = &loaded.model;
    let backend = loaded._backend.as_ref();
    let context_tokens = descriptor.context_tokens.unwrap_or(2048).max(1) as u32;
    let max_output_tokens = prompt.max_output_tokens.max(1) as usize;

    let prompt_tokens = model
        .str_to_token(&prompt.full_prompt, AddBos::Always)
        .context("failed to tokenize cleanup prompt")?;

    if prompt_tokens.len() + max_output_tokens >= context_tokens as usize {
        return Err(anyhow!(
            "Cleanup prompt is too long for the {context_tokens}-token context."
        ));
    }

    let threads = default_thread_count() as i32;
    let context_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(context_tokens))
        .with_n_batch(context_tokens)
        .with_n_ubatch(context_tokens)
        .with_n_threads(threads)
        .with_n_threads_batch(threads);

    let mut context = model
        .new_context(backend, context_params)
        .context("failed to create llama.cpp cleanup session")?;

    let mut batch = LlamaBatch::new(context_tokens as usize, 1);
    batch
        .add_sequence(&prompt_tokens, 0, false)
        .context("failed to batch cleanup prompt")?;
    context
        .decode(&mut batch)
        .context("llama.cpp failed to evaluate cleanup prompt")?;

    if is_cancelled(cancel_flag) {
        return Err(anyhow!("Recording cancelled."));
    }

    let mut sampler = sampler_for(descriptor.sampler.as_ref());
    sampler.accept_many(prompt_tokens.iter());

    let mut output = Vec::new();
    let mut position = prompt_tokens.len() as i32;

    for _ in 0..max_output_tokens {
        if is_cancelled(cancel_flag) {
            return Err(anyhow!("Recording cancelled."));
        }

        let token = sampler.sample(&context, -1);
        if model.is_eog_token(token) {
            break;
        }

        sampler.accept(token);
        output.extend(token_piece_bytes(model, token)?);

        batch.clear();
        batch
            .add(token, position, &[0], true)
            .context("failed to batch cleanup token")?;
        context
            .decode(&mut batch)
            .context("llama.cpp failed while generating cleanup output")?;
        position += 1;
    }

    Ok(String::from_utf8(output)
        .unwrap_or_else(|error| String::from_utf8_lossy(&error.into_bytes()).into_owned()))
}

fn sampler_for(config: Option<&SamplerConfig>) -> LlamaSampler {
    let Some(config) = config else {
        return LlamaSampler::greedy();
    };

    let mut stages = Vec::new();

    if config.repeat_penalty != 1.0
        || config.frequency_penalty != 0.0
        || config.presence_penalty != 0.0
    {
        stages.push(LlamaSampler::penalties(
            64,
            config.repeat_penalty.max(0.0),
            config.frequency_penalty,
            config.presence_penalty,
        ));
    }

    if config.top_k > 0 {
        stages.push(LlamaSampler::top_k(config.top_k));
    }

    if config.top_p > 0.0 && config.top_p < 1.0 {
        stages.push(LlamaSampler::top_p(config.top_p.clamp(0.0, 1.0), 1));
    }

    if config.min_p > 0.0 {
        stages.push(LlamaSampler::min_p(config.min_p.clamp(0.0, 1.0), 1));
    }

    stages.push(LlamaSampler::temp(config.temperature.clamp(0.001, 2.0)));
    stages.push(LlamaSampler::dist(1));

    LlamaSampler::chain_simple(stages)
}

fn token_piece_bytes(model: &LlamaModel, token: LlamaToken) -> anyhow::Result<Vec<u8>> {
    match model.token_to_piece_bytes(token, 16, false, None) {
        Ok(bytes) => Ok(bytes),
        Err(TokenToStringError::InsufficientBufferSpace(size)) if size < 0 => {
            let size = size
                .checked_neg()
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(64);
            model
                .token_to_piece_bytes(token, size, false, None)
                .context("failed to decode cleanup token")
        }
        Err(error) => Err(error).context("failed to decode cleanup token"),
    }
}

fn llama_backend() -> anyhow::Result<Arc<LlamaBackend>> {
    static BACKEND: OnceLock<Result<Arc<LlamaBackend>, String>> = OnceLock::new();

    match BACKEND.get_or_init(|| {
        LlamaBackend::init()
            .map(Arc::new)
            .map_err(|e| e.to_string())
    }) {
        Ok(backend) => Ok(Arc::clone(backend)),
        Err(error) => Err(anyhow!(error.clone())),
    }
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
    #[cfg(feature = "cuda-core")]
    {
        99
    }

    #[cfg(not(feature = "cuda-core"))]
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
