use crate::model_downloads::model_path;
use anyhow::{anyhow, Context};
use parrot_audio::RecordedAudio;
use parrot_core_service::TranscriptionOutput;
use parrot_models::ModelDescriptor;
use parrot_protocol::NativeCorePaths;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

#[derive(Clone, Default)]
pub struct WhisperCppPipeline {
    contexts: Arc<Mutex<HashMap<PathBuf, Arc<WhisperContext>>>>,
}

impl WhisperCppPipeline {
    pub fn warm_descriptor(
        &self,
        descriptor: &ModelDescriptor,
        paths: &NativeCorePaths,
    ) -> anyhow::Result<()> {
        let path = model_path(descriptor, paths)?;
        ensure_model_file(&path)?;
        let _ = self.context_for(&path)?;
        Ok(())
    }

    pub fn transcribe_descriptor(
        &self,
        descriptor: &ModelDescriptor,
        audio: &RecordedAudio,
        paths: &NativeCorePaths,
        language_code: Option<&str>,
        detect_language: bool,
    ) -> anyhow::Result<TranscriptionOutput> {
        if audio.samples.is_empty() {
            return Err(anyhow!("No speech detected."));
        }
        if audio.sample_rate_hz != 16_000 || audio.channels != 1 {
            return Err(anyhow!(
                "Whisper.cpp expects mono 16 kHz audio; got {} Hz with {} channel(s).",
                audio.sample_rate_hz,
                audio.channels
            ));
        }

        let path = model_path(descriptor, paths)?;
        ensure_model_file(&path)?;

        let context = self.context_for(&path)?;
        let mut state = context
            .create_state()
            .context("failed to create Whisper.cpp state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(default_thread_count());
        params.set_language(language_code);
        params.set_detect_language(detect_language);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, &audio.samples)
            .context("Whisper.cpp transcription failed")?;

        let text = state
            .as_iter()
            .map(|segment| segment.to_string())
            .collect::<String>()
            .trim()
            .to_string();
        if text.is_empty() {
            return Err(anyhow!("Transcription was empty."));
        }

        let detected_language_code = if detect_language {
            get_lang_str(state.full_lang_id_from_state()).map(str::to_string)
        } else {
            None
        };

        Ok(TranscriptionOutput {
            text,
            detected_language_code,
        })
    }

    fn context_for(&self, path: &Path) -> anyhow::Result<Arc<WhisperContext>> {
        let key = path.to_path_buf();
        let mut contexts = self
            .contexts
            .lock()
            .expect("whisper context cache poisoned");
        if let Some(context) = contexts.get(&key).cloned() {
            return Ok(context);
        }

        let context = Arc::new(
            WhisperContext::new_with_params(path, WhisperContextParameters::default())
                .with_context(|| format!("failed to load Whisper.cpp model {}", path.display()))?,
        );
        contexts.insert(key, context.clone());
        Ok(context)
    }
}

fn ensure_model_file(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(anyhow!("Model download required: {}", path.display()))
    }
}

fn default_thread_count() -> i32 {
    std::thread::available_parallelism()
        .map(|count| count.get().clamp(1, 16) as i32)
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_thread_count_stays_in_supported_range() {
        let count = default_thread_count();
        assert!((1..=16).contains(&count));
    }
}
