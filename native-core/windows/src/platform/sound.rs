use parrot_protocol::NativeCorePaths;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    RecordingStart,
    RecordingSuccess,
    RecordingCancel,
    RecordingError,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoundManifest {
    recording_start: Option<String>,
    recording_success: Option<String>,
    recording_cancel: Option<String>,
    recording_error: Option<String>,
}

impl SoundManifest {
    fn relative_path(&self, event: SoundEvent) -> Option<&str> {
        match event {
            SoundEvent::RecordingStart => self.recording_start.as_deref(),
            SoundEvent::RecordingSuccess => self.recording_success.as_deref(),
            SoundEvent::RecordingCancel => self.recording_cancel.as_deref(),
            SoundEvent::RecordingError => self.recording_error.as_deref(),
        }
    }
}

pub fn play(event: SoundEvent, enabled: bool, paths: Option<&NativeCorePaths>) {
    if !enabled {
        return;
    }
    let Some(paths) = paths else {
        return;
    };
    let Some(path) = sound_path_for_event(Path::new(&paths.shared_resources_dir), event) else {
        return;
    };
    play_file(&path);
}

fn sound_path_for_event(shared_resources_dir: &Path, event: SoundEvent) -> Option<PathBuf> {
    let manifest_path = shared_resources_dir.join("sounds.json");
    let manifest: SoundManifest = serde_json::from_str(&fs::read_to_string(manifest_path).ok()?)
        .inspect_err(|_error| {
            #[cfg(debug_assertions)]
            eprintln!("Windows sound manifest is invalid: {_error}");
        })
        .ok()?;
    let relative_path = manifest.relative_path(event)?;
    let path = shared_resources_dir.join(relative_path);
    path.exists().then_some(path)
}

#[cfg(target_os = "windows")]
fn play_file(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    unsafe {
        let _ = PlaySoundW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn play_file(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn paths(temp: &TempDir) -> NativeCorePaths {
        NativeCorePaths {
            app_data_dir: temp.path().join("app-data").display().to_string(),
            models_dir: temp.path().join("models").display().to_string(),
            speech_models_dir: temp.path().join("models/speech").display().to_string(),
            cleanup_models_dir: temp.path().join("models/cleanup").display().to_string(),
            resources_dir: temp.path().join("resources").display().to_string(),
            shared_resources_dir: temp.path().join("shared").display().to_string(),
            temp_dir: temp.path().join("temp").display().to_string(),
        }
    }

    #[test]
    fn missing_manifest_returns_no_sound_path() {
        let temp = TempDir::new().unwrap();

        assert_eq!(
            sound_path_for_event(temp.path(), SoundEvent::RecordingStart),
            None
        );
    }

    #[test]
    fn invalid_manifest_returns_no_sound_path() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("sounds.json"), "{").unwrap();

        assert_eq!(
            sound_path_for_event(temp.path(), SoundEvent::RecordingStart),
            None
        );
    }

    #[test]
    fn missing_referenced_file_returns_no_sound_path() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("sounds.json"),
            r#"{"recordingStart":"sounds/recording-start.wav"}"#,
        )
        .unwrap();

        assert_eq!(
            sound_path_for_event(temp.path(), SoundEvent::RecordingStart),
            None
        );
    }

    #[test]
    fn resolves_existing_file_relative_to_shared_resources_dir() {
        let temp = TempDir::new().unwrap();
        let sound_dir = temp.path().join("sounds");
        fs::create_dir_all(&sound_dir).unwrap();
        let sound_path = sound_dir.join("recording-start.wav");
        fs::write(&sound_path, []).unwrap();
        fs::write(
            temp.path().join("sounds.json"),
            r#"{"recordingStart":"sounds/recording-start.wav"}"#,
        )
        .unwrap();

        assert_eq!(
            sound_path_for_event(temp.path(), SoundEvent::RecordingStart),
            Some(sound_path)
        );
    }

    #[test]
    fn sound_failures_do_not_fail_callers() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);

        play(SoundEvent::RecordingSuccess, true, Some(&paths));
        play(SoundEvent::RecordingError, false, Some(&paths));
        play(SoundEvent::RecordingCancel, true, None);
    }
}
