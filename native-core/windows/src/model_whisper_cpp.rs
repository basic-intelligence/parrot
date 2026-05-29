use crate::models::downloads::{model_path, windows_descriptor_for};
use anyhow::{anyhow, Context};
use parrot_language::{
    decode_language_code, detected_language_metadata, selected_language_metadata,
    DictationLanguageMetadata, DictationLanguageSettings, SpeechModelSlot,
};
use parrot_protocol::{AppSettings, NativeCorePaths};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

#[derive(Clone, Default)]
pub struct WhisperCppPipeline {
    contexts: Arc<Mutex<HashMap<PathBuf, Arc<WhisperContext>>>>,
}

pub struct Transcription {
    pub text: String,
    pub language: DictationLanguageMetadata,
}

impl WhisperCppPipeline {
    pub fn warm(&self, settings: &AppSettings, paths: &NativeCorePaths) -> anyhow::Result<()> {
        let descriptor = speech_descriptor(settings)?;
        let path = model_path(&descriptor, paths)?;
        ensure_model_file(&path)?;
        let _ = self.context_for(&path)?;
        Ok(())
    }

    pub fn transcribe(
        &self,
        samples_16khz: &[f32],
        settings: &AppSettings,
        paths: &NativeCorePaths,
    ) -> anyhow::Result<Transcription> {
        self.transcribe_inner(samples_16khz, settings, paths, None)
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn transcribe_with_cancel(
        &self,
        samples_16khz: &[f32],
        settings: &AppSettings,
        paths: &NativeCorePaths,
        cancel_flag: Arc<AtomicBool>,
    ) -> anyhow::Result<Transcription> {
        self.transcribe_inner(samples_16khz, settings, paths, Some(cancel_flag))
    }

    fn transcribe_inner(
        &self,
        samples_16khz: &[f32],
        settings: &AppSettings,
        paths: &NativeCorePaths,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<Transcription> {
        if samples_16khz.is_empty() {
            return Err(anyhow!("No speech detected."));
        }
        if is_cancelled(cancel_flag.as_ref()) {
            return Err(anyhow!("Recording cancelled."));
        }

        let language_settings = DictationLanguageSettings::from(settings);
        let descriptor = speech_descriptor(settings)?;
        let path = model_path(&descriptor, paths)?;
        ensure_model_file(&path)?;

        let context = self.context_for(&path)?;
        let mut state = context
            .create_state()
            .context("failed to create Whisper.cpp state")?;
        let language_code = decode_language_code(&language_settings);
        let detect_language = language_code.is_none();
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(default_thread_count());
        params.set_language(language_code.as_deref());
        params.set_detect_language(detect_language);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, samples_16khz)
            .context("Whisper.cpp transcription failed")?;
        if is_cancelled(cancel_flag.as_ref()) {
            return Err(anyhow!("Recording cancelled."));
        }

        let text = state
            .as_iter()
            .map(|segment| segment.to_string())
            .collect::<String>()
            .trim()
            .to_string();
        if text.is_empty() {
            return Err(anyhow!("Transcription was empty."));
        }

        let language = if detect_language {
            detected_language_metadata(get_lang_str(state.full_lang_id_from_state()))
        } else {
            selected_language_metadata(&language_settings)
        };

        Ok(Transcription { text, language })
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

fn speech_descriptor(settings: &AppSettings) -> anyhow::Result<parrot_models::ModelDescriptor> {
    let language_settings = DictationLanguageSettings::from(settings);
    let public_id = match parrot_language::speech_model_slot(&language_settings) {
        SpeechModelSlot::Speech => "speech",
        SpeechModelSlot::SpeechMultilingual => "speech-multilingual",
    };
    windows_descriptor_for(public_id)
        .ok_or_else(|| anyhow!("No Windows speech model is available for {public_id}."))
}

fn ensure_model_file(path: &std::path::Path) -> anyhow::Result<()> {
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

fn is_cancelled(cancel_flag: Option<&Arc<AtomicBool>>) -> bool {
    cancel_flag
        .map(|flag| flag.load(Ordering::SeqCst))
        .unwrap_or(false)
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
