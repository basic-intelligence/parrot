use async_trait::async_trait;
use parrot_audio::RecordedAudio;
use parrot_protocol::{
    AudioDevice, PermissionKind, PermissionSnapshot, ShortcutSettings, SoundEvent,
};

#[derive(Debug, Clone)]
pub struct ShortcutBindings {
    pub push_to_talk: ShortcutSettings,
    pub hands_free: ShortcutSettings,
}

#[derive(Debug, Clone, Copy)]
pub enum ShortcutTarget {
    PushToTalk,
    HandsFree,
}

#[derive(Debug, Clone)]
pub struct PasteTarget {
    pub platform_id: String,
}

/// Future sidecar boundary.
///
/// This crate intentionally does not implement Windows or Linux adapters. It
/// documents the split future Rust sidecars should follow while macOS remains
/// implemented in Swift.
#[async_trait]
pub trait PlatformAdapter {
    async fn list_audio_devices(&self) -> anyhow::Result<Vec<AudioDevice>>;
    async fn start_audio_recording(&self, input_uid: Option<&str>) -> anyhow::Result<()>;
    async fn stop_audio_recording(&self) -> anyhow::Result<RecordedAudio>;

    async fn permission_snapshot(&self) -> anyhow::Result<PermissionSnapshot>;
    async fn request_permission(
        &self,
        kind: PermissionKind,
        open_settings: bool,
    ) -> anyhow::Result<PermissionSnapshot>;

    async fn start_hotkey_monitor(&self, shortcuts: ShortcutBindings) -> anyhow::Result<()>;
    async fn stop_hotkey_monitor(&self) -> anyhow::Result<()>;
    async fn capture_shortcut(&self, target: ShortcutTarget) -> anyhow::Result<ShortcutSettings>;

    async fn capture_paste_target(&self) -> anyhow::Result<Option<PasteTarget>>;
    async fn focused_text_before_cursor(
        &self,
        target: Option<&PasteTarget>,
    ) -> anyhow::Result<Option<String>>;
    async fn paste_text(&self, text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()>;

    fn play_sound(&self, event: SoundEvent, enabled: bool);
}

pub const MACOS_PLATFORM_ADAPTER_FILES: &[(&str, &str)] = &[
    ("AudioRecorder.swift", "macOS audio adapter"),
    ("HotkeyMonitor.swift", "macOS hotkey adapter"),
    ("PermissionManager.swift", "macOS permission adapter"),
    (
        "FocusedTextContextReader.swift",
        "macOS focused text adapter",
    ),
    ("TextPaster.swift", "macOS paste adapter"),
    ("InputDeviceManager.swift", "macOS input device adapter"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_current_macos_adapter_files() {
        let files = MACOS_PLATFORM_ADAPTER_FILES
            .iter()
            .map(|(file, _)| *file)
            .collect::<Vec<_>>();

        assert!(files.contains(&"AudioRecorder.swift"));
        assert!(files.contains(&"HotkeyMonitor.swift"));
        assert!(files.contains(&"TextPaster.swift"));
    }
}
