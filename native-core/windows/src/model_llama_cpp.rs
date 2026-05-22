use crate::models::downloads::{model_path, windows_descriptor_for};
use anyhow::anyhow;
use parrot_language::DictationLanguageMetadata;
use parrot_protocol::{AppSettings, NativeCorePaths};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Default)]
pub struct LlamaCleanupPipeline;

impl LlamaCleanupPipeline {
    pub fn warm(&self, settings: &AppSettings, cleanup_models_dir: &str) -> anyhow::Result<()> {
        if !settings.cleanup_enabled {
            return Ok(());
        }
        let Some(descriptor) = windows_descriptor_for(&settings.cleanup_model_id) else {
            return Ok(());
        };
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
        if path.exists() {
            Ok(())
        } else {
            Err(anyhow!("Model download required: {}", path.display()))
        }
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
        _language: &DictationLanguageMetadata,
        _cleanup_models_dir: &str,
        _cleanup_prompt: &str,
        _debug_cleanup_failures: bool,
        cancel_flag: &Arc<AtomicBool>,
    ) -> anyhow::Result<String> {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err(anyhow!("Recording cancelled."));
        }
        if !settings.cleanup_enabled {
            return Ok(raw.trim().to_string());
        }

        // The Windows QA branch does not yet have a llama.cpp text-generation adapter.
        // Keep dictation usable by returning the raw transcript instead of failing.
        Ok(raw.trim().to_string())
    }
}
