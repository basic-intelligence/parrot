use crate::whisper_protocol::Transcription;
use anyhow::{anyhow, Context};
use parrot_language::{
    decode_language_code, detected_language_metadata, selected_language_metadata,
    DictationLanguageSettings,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

#[derive(Default)]
pub struct WhisperEngine {
    contexts: HashMap<PathBuf, Arc<WhisperContext>>,
}

impl WhisperEngine {
    pub fn warm(&mut self, model_path: &Path) -> anyhow::Result<()> {
        ensure_model_file(model_path)?;
        let _ = self.context_for(model_path)?;
        Ok(())
    }

    pub fn transcribe_file(
        &mut self,
        model_path: &Path,
        audio_path: &Path,
        language_settings: &DictationLanguageSettings,
    ) -> anyhow::Result<Transcription> {
        let samples = load_float_samples(audio_path)?;
        self.transcribe_samples(model_path, &samples, language_settings)
    }

    fn transcribe_samples(
        &mut self,
        model_path: &Path,
        samples_16khz: &[f32],
        language_settings: &DictationLanguageSettings,
    ) -> anyhow::Result<Transcription> {
        if samples_16khz.is_empty() {
            return Err(anyhow!("No speech detected."));
        }

        ensure_model_file(model_path)?;
        let context = self.context_for(model_path)?;
        let mut state = context
            .create_state()
            .context("failed to create Whisper.cpp state")?;
        let language_code = decode_language_code(language_settings);
        let detect_language = language_code.is_none();
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(default_thread_count());
        params.set_language(language_code.as_deref());
        params.set_detect_language(detect_language);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, samples_16khz)
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

        let language = if detect_language {
            detected_language_metadata(get_lang_str(state.full_lang_id_from_state()))
        } else {
            selected_language_metadata(language_settings)
        };

        Ok(Transcription { text, language })
    }

    fn context_for(&mut self, path: &Path) -> anyhow::Result<Arc<WhisperContext>> {
        let key = path.to_path_buf();
        if let Some(context) = self.contexts.get(&key).cloned() {
            return Ok(context);
        }

        let context = Arc::new(
            WhisperContext::new_with_params(path, WhisperContextParameters::default())
                .with_context(|| format!("failed to load Whisper.cpp model {}", path.display()))?,
        );
        self.contexts.insert(key, context.clone());
        Ok(context)
    }
}

fn load_float_samples(path: &Path) -> anyhow::Result<Vec<f32>> {
    let data =
        fs::read(path).with_context(|| format!("failed to read audio file {}", path.display()))?;
    if data.len() % std::mem::size_of::<f32>() != 0 {
        return Err(anyhow!("Audio data must be raw Float32 samples."));
    }

    Ok(data
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
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
    use tempfile::TempDir;

    #[test]
    fn default_thread_count_stays_in_supported_range() {
        let count = default_thread_count();
        assert!((1..=16).contains(&count));
    }

    #[test]
    fn load_float_samples_requires_even_float_bytes() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("bad.f32");
        std::fs::write(&path, [0_u8, 1, 2]).unwrap();

        let error = load_float_samples(&path).unwrap_err();

        assert!(error.to_string().contains("raw Float32"));
    }

    #[test]
    fn load_float_samples_decodes_little_endian_floats() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("ok.f32");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.25_f32.to_le_bytes());
        bytes.extend_from_slice(&(-2.5_f32).to_le_bytes());
        std::fs::write(&path, bytes).unwrap();

        let samples = load_float_samples(&path).unwrap();

        assert_eq!(samples, vec![1.25, -2.5]);
    }
}
