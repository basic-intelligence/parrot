use crate::model_downloads::model_path;
use anyhow::{anyhow, Context};
use llama_cpp_2::{
    context::{params::LlamaContextParams, LlamaContext},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
    token::LlamaToken,
    TokenToStringError,
};
use llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_DISABLED;
use parrot_models::{ModelDescriptor, SamplerConfig};
use parrot_prompts::CleanupPrompt;
use parrot_protocol::NativeCorePaths;
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

const MAX_CLEANUP_BATCH_TOKENS: u32 = 512;
const LINUX_UNSUPPORTED_CLEANUP_MODELS: &[&str] = &["llama-qwen3-5-2b-q8-0"];

#[derive(Clone, Default)]
pub struct LlamaCleanupPipeline {
    models: Arc<Mutex<HashMap<PathBuf, Arc<LoadedCleanupModel>>>>,
}

struct LoadedCleanupModel {
    model: LlamaModel,
    _backend: Arc<LlamaBackend>,
}

impl LlamaCleanupPipeline {
    pub fn warm_descriptor(
        &self,
        descriptor: &ModelDescriptor,
        paths: &NativeCorePaths,
    ) -> anyhow::Result<()> {
        if linux_unsupported_cleanup_model(descriptor).is_some() {
            return Ok(());
        }

        let path = model_path(descriptor, paths)?;
        ensure_gguf_file(&path)?;
        let _ = self.model_for(&path)?;
        Ok(())
    }

    pub fn cleanup_descriptor(
        &self,
        descriptor: &ModelDescriptor,
        paths: &NativeCorePaths,
        prompt: &CleanupPrompt,
    ) -> anyhow::Result<String> {
        if let Some(message) = linux_unsupported_cleanup_model(descriptor) {
            return Err(anyhow!(message));
        }

        let path = model_path(descriptor, paths)?;
        ensure_gguf_file(&path)?;
        let loaded = self.model_for(&path)?;
        complete_with_model(descriptor, &loaded, prompt)
    }

    pub fn clear_cache(&self) {
        self.models
            .lock()
            .expect("llama.cpp cleanup model cache poisoned")
            .clear();
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
        let params = LlamaModelParams::default().with_n_gpu_layers(0);

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

fn linux_unsupported_cleanup_model(descriptor: &ModelDescriptor) -> Option<String> {
    if !LINUX_UNSUPPORTED_CLEANUP_MODELS.contains(&descriptor.concrete_id.as_str()) {
        return None;
    }

    Some(format!(
        "{} is currently disabled on Linux because llama.cpp aborts while initializing this GGUF.",
        descriptor.display_name
    ))
}

fn complete_with_model(
    descriptor: &ModelDescriptor,
    loaded: &LoadedCleanupModel,
    prompt: &CleanupPrompt,
) -> anyhow::Result<String> {
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
    let batch_tokens = cleanup_batch_tokens(context_tokens);
    let context_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(context_tokens))
        .with_n_batch(batch_tokens)
        .with_n_ubatch(batch_tokens)
        .with_flash_attention_policy(LLAMA_FLASH_ATTN_TYPE_DISABLED)
        .with_n_threads(threads)
        .with_n_threads_batch(threads);

    let mut context = model
        .new_context(backend, context_params)
        .context("failed to create llama.cpp cleanup session")?;

    let mut batch = LlamaBatch::new(batch_tokens as usize, 1);
    decode_prompt_tokens(
        &mut context,
        &mut batch,
        &prompt_tokens,
        batch_tokens as usize,
    )?;

    let mut sampler = sampler_for(descriptor.sampler.as_ref());
    sampler.accept_many(prompt_tokens.iter());

    let mut output = Vec::new();
    let mut position = prompt_tokens.len() as i32;

    for _ in 0..max_output_tokens {
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

fn decode_prompt_tokens(
    context: &mut LlamaContext<'_>,
    batch: &mut LlamaBatch<'_>,
    prompt_tokens: &[LlamaToken],
    batch_tokens: usize,
) -> anyhow::Result<()> {
    if prompt_tokens.is_empty() {
        return Ok(());
    }

    for (chunk_index, chunk) in prompt_tokens.chunks(batch_tokens.max(1)).enumerate() {
        let chunk_start = chunk_index * batch_tokens.max(1);
        batch.clear();

        for (offset, token) in chunk.iter().enumerate() {
            let position = chunk_start + offset;
            let logits = position + 1 == prompt_tokens.len();
            batch
                .add(*token, position as i32, &[0], logits)
                .context("failed to batch cleanup prompt")?;
        }

        context
            .decode(batch)
            .context("llama.cpp failed to evaluate cleanup prompt")?;
    }

    Ok(())
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

fn cleanup_batch_tokens(context_tokens: u32) -> u32 {
    context_tokens.clamp(1, MAX_CLEANUP_BATCH_TOKENS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_downloads::linux_descriptor_for;
    use parrot_prompts::{assemble_cleanup_prompt, CleanupPromptInput};
    use tempfile::TempDir;

    fn paths(temp: &TempDir) -> NativeCorePaths {
        NativeCorePaths {
            app_data_dir: temp.path().join("app-data").display().to_string(),
            models_dir: temp.path().join("models").display().to_string(),
            speech_models_dir: temp.path().join("models/speech").display().to_string(),
            cleanup_models_dir: temp.path().join("models/cleanup").display().to_string(),
            resources_dir: temp.path().join("resources").display().to_string(),
            shared_resources_dir: temp.path().join("resources/shared").display().to_string(),
            temp_dir: temp.path().join("temp").display().to_string(),
        }
    }

    fn prompt(descriptor: &ModelDescriptor) -> CleanupPrompt {
        assemble_cleanup_prompt(&CleanupPromptInput {
            cleanup_rules: "Clean it.".into(),
            dictionary_entries: Vec::new(),
            raw_transcript: "hello world".into(),
            language: parrot_language::DictationLanguageMetadata {
                mode: "selected".into(),
                code: Some("en".into()),
                locale: None,
                name: Some("English".into()),
            },
            prompt_format: descriptor.prompt_format.unwrap(),
            default_output_tokens: descriptor.output_tokens.unwrap_or(512),
        })
    }

    #[test]
    fn missing_cleanup_model_errors_consistently() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        let pipeline = LlamaCleanupPipeline::default();
        let descriptor = linux_descriptor_for("cleanup-gemma-4-e2b").unwrap();

        let error = pipeline
            .cleanup_descriptor(&descriptor, &paths, &prompt(&descriptor))
            .unwrap_err();

        assert!(error.to_string().contains("Model download required"));
    }

    #[test]
    fn corrupt_cleanup_model_surfaces_gguf_error() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        let pipeline = LlamaCleanupPipeline::default();
        let descriptor = linux_descriptor_for("cleanup-gemma-4-e2b").unwrap();
        let path = model_path(&descriptor, &paths).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"NOPE").unwrap();

        let error = pipeline
            .cleanup_descriptor(&descriptor, &paths, &prompt(&descriptor))
            .unwrap_err();

        assert!(error.to_string().contains("valid GGUF"));
    }

    #[test]
    fn default_thread_count_stays_in_supported_range() {
        let count = default_thread_count();
        assert!((1..=16).contains(&count));
    }

    #[test]
    fn cleanup_batch_caps_context_sized_batches() {
        assert_eq!(cleanup_batch_tokens(0), 1);
        assert_eq!(cleanup_batch_tokens(128), 128);
        assert_eq!(cleanup_batch_tokens(2048), MAX_CLEANUP_BATCH_TOKENS);
    }

    #[test]
    #[ignore = "requires PARROT_CLEANUP_SMOKE_MODEL to point at a downloaded GGUF cleanup model"]
    fn downloaded_cleanup_model_does_not_abort_release_cleanup() {
        let model_path = std::env::var("PARROT_CLEANUP_SMOKE_MODEL")
            .expect("PARROT_CLEANUP_SMOKE_MODEL must point at a downloaded GGUF cleanup model");
        let model_path = PathBuf::from(model_path);
        let cleanup_models_dir = model_path
            .parent()
            .expect("smoke model path must have a parent directory");
        let temp = TempDir::new().unwrap();
        let mut paths = paths(&temp);
        paths.cleanup_models_dir = cleanup_models_dir.display().to_string();

        let pipeline = LlamaCleanupPipeline::default();
        let descriptor = linux_descriptor_for("cleanup").unwrap();
        let mut prompt = prompt(&descriptor);
        prompt.max_output_tokens = 1;

        match pipeline.cleanup_descriptor(&descriptor, &paths, &prompt) {
            Ok(_) => {}
            Err(error) => {
                assert!(
                    error.to_string().contains("disabled on Linux"),
                    "unexpected cleanup smoke error: {error}"
                );
            }
        }
    }
}
