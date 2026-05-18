use anyhow::Context;
use parrot_protocol::SoundEvent;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SOUNDS_JSON: &str = include_str!("../../../native-core/shared/sounds.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SoundManifest {
    pub recording_start: String,
    pub recording_success: String,
    pub recording_cancel: String,
    pub recording_error: String,
}

impl SoundManifest {
    pub fn load_embedded() -> anyhow::Result<Self> {
        serde_json::from_str(SOUNDS_JSON).context("shared sound manifest must be valid JSON")
    }

    pub fn relative_path(&self, event: SoundEvent) -> &str {
        match event {
            SoundEvent::RecordingStart => &self.recording_start,
            SoundEvent::RecordingSuccess => &self.recording_success,
            SoundEvent::RecordingCancel => &self.recording_cancel,
            SoundEvent::RecordingError => &self.recording_error,
        }
    }

    pub fn path_for(&self, shared_resources_dir: impl AsRef<Path>, event: SoundEvent) -> PathBuf {
        shared_resources_dir
            .as_ref()
            .join(self.relative_path(event))
    }
}

pub trait SoundFeedback {
    fn play(&self, event: SoundEvent, enabled: bool);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shared_sound_manifest() {
        let manifest = SoundManifest::load_embedded().unwrap();

        assert_eq!(
            manifest.relative_path(SoundEvent::RecordingSuccess),
            "sounds/recording-success.wav"
        );
    }

    #[test]
    fn missing_sound_files_do_not_make_manifest_invalid() {
        let manifest = SoundManifest::load_embedded().unwrap();
        let path = manifest.path_for("/tmp/parrot-shared", SoundEvent::RecordingError);

        assert_eq!(
            path,
            PathBuf::from("/tmp/parrot-shared/sounds/recording-error.wav")
        );
    }
}
