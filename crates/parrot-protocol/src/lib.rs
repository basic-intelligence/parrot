use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_CLEANUP_MODEL_ID: &str = "cleanup";
pub const GEMMA_CLEANUP_MODEL_ID: &str = "cleanup-gemma-4-e2b";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub term: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DictationLanguageMode {
    English,
    Detect,
    Specific,
}

fn default_dictation_language_mode() -> DictationLanguageMode {
    DictationLanguageMode::English
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutMode {
    Hold,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutModifier {
    Command,
    Control,
    Option,
    Alt,
    Shift,
    Fn,
    Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutKey {
    Space,
    Return,
    Tab,
    Escape,
    Character(String),
    Function(u8),
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutChord {
    #[serde(default)]
    pub modifiers: Vec<ShortcutModifier>,
    #[serde(default)]
    pub key: Option<ShortcutKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutPlatformCodes {
    #[serde(default)]
    pub macos_key_codes: Option<Vec<u16>>,
    #[serde(default)]
    pub windows_virtual_keys: Option<Vec<u16>>,
    #[serde(default)]
    pub linux_key_codes: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutSettings {
    pub display_name: String,
    pub mode: ShortcutMode,
    #[serde(default = "default_shortcut_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub double_tap_toggle: bool,
    #[serde(default)]
    pub chord: Option<ShortcutChord>,
    #[serde(default)]
    pub platform_codes: ShortcutPlatformCodes,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawShortcutSettings {
    display_name: String,
    #[serde(default)]
    macos_key_codes: Vec<u16>,
    mode: ShortcutMode,
    #[serde(default = "default_shortcut_enabled")]
    enabled: bool,
    #[serde(default)]
    double_tap_toggle: bool,
    #[serde(default)]
    chord: Option<ShortcutChord>,
    #[serde(default)]
    platform_codes: ShortcutPlatformCodes,
}

impl<'de> Deserialize<'de> for ShortcutSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawShortcutSettings::deserialize(deserializer)?;
        let mut platform_codes = raw.platform_codes;
        if platform_codes.macos_key_codes.is_none() && !raw.macos_key_codes.is_empty() {
            platform_codes.macos_key_codes = Some(raw.macos_key_codes);
        }

        let macos_key_codes = platform_codes.macos_key_codes.clone().unwrap_or_default();
        let chord = raw
            .chord
            .or_else(|| infer_shortcut_chord(&macos_key_codes, &raw.display_name));

        Ok(Self {
            display_name: raw.display_name,
            mode: raw.mode,
            enabled: raw.enabled,
            double_tap_toggle: raw.double_tap_toggle,
            chord,
            platform_codes,
        })
    }
}

impl ShortcutSettings {
    pub fn macos_key_codes(&self) -> &[u16] {
        self.platform_codes
            .macos_key_codes
            .as_deref()
            .unwrap_or_default()
    }
}

pub fn default_shortcut_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub selected_input_uid: Option<String>,
    #[serde(default = "default_push_to_talk_shortcut")]
    pub push_to_talk_shortcut: ShortcutSettings,
    #[serde(default = "default_hands_free_shortcut")]
    pub hands_free_shortcut: ShortcutSettings,
    #[serde(default = "default_dictation_language_mode")]
    pub dictation_language_mode: DictationLanguageMode,
    #[serde(default)]
    pub dictation_language_code: Option<String>,
    #[serde(default = "default_cleanup_model_id")]
    pub cleanup_model_id: String,
    #[serde(default = "default_true")]
    pub cleanup_enabled: bool,
    #[serde(default)]
    pub cleanup_prompt: String,
    #[serde(default)]
    pub dictionary_entries: Vec<DictionaryEntry>,
    #[serde(default = "default_true")]
    pub play_sounds: bool,
    #[serde(default)]
    pub paste_into_recording_start_window: bool,
    #[serde(default)]
    pub history_enabled: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub input_monitoring_permission_shown_in_onboarding: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            selected_input_uid: None,
            push_to_talk_shortcut: default_push_to_talk_shortcut(),
            hands_free_shortcut: default_hands_free_shortcut(),
            dictation_language_mode: DictationLanguageMode::English,
            dictation_language_code: None,
            cleanup_model_id: default_cleanup_model_id(),
            cleanup_enabled: true,
            cleanup_prompt: String::new(),
            dictionary_entries: Vec::new(),
            play_sounds: true,
            paste_into_recording_start_window: false,
            history_enabled: false,
            launch_at_login: false,
            onboarding_completed: false,
            input_monitoring_permission_shown_in_onboarding: false,
        }
    }
}

pub fn default_push_to_talk_shortcut() -> ShortcutSettings {
    ShortcutSettings {
        display_name: "Fn".into(),
        mode: ShortcutMode::Hold,
        enabled: true,
        double_tap_toggle: false,
        chord: Some(ShortcutChord {
            modifiers: vec![ShortcutModifier::Fn],
            key: None,
        }),
        platform_codes: ShortcutPlatformCodes {
            macos_key_codes: Some(vec![63]),
            windows_virtual_keys: None,
            linux_key_codes: None,
        },
    }
}

pub fn default_hands_free_shortcut() -> ShortcutSettings {
    ShortcutSettings {
        display_name: "Control + Space".into(),
        mode: ShortcutMode::Toggle,
        enabled: true,
        double_tap_toggle: false,
        chord: Some(ShortcutChord {
            modifiers: vec![ShortcutModifier::Control],
            key: Some(ShortcutKey::Space),
        }),
        platform_codes: ShortcutPlatformCodes {
            macos_key_codes: Some(vec![59, 49]),
            windows_virtual_keys: None,
            linux_key_codes: None,
        },
    }
}

pub fn default_cleanup_model_id() -> String {
    DEFAULT_CLEANUP_MODEL_ID.to_string()
}

fn default_true() -> bool {
    true
}

fn infer_shortcut_chord(key_codes: &[u16], display_name: &str) -> Option<ShortcutChord> {
    if key_codes.is_empty() {
        return None;
    }

    let mut modifiers = Vec::new();
    let mut key = None;

    for code in key_codes {
        match *code {
            55 | 54 => modifiers.push(ShortcutModifier::Command),
            59 | 62 => modifiers.push(ShortcutModifier::Control),
            58 | 61 => modifiers.push(ShortcutModifier::Option),
            56 | 60 => modifiers.push(ShortcutModifier::Shift),
            63 => modifiers.push(ShortcutModifier::Fn),
            49 => key = Some(ShortcutKey::Space),
            36 | 76 => key = Some(ShortcutKey::Return),
            48 => key = Some(ShortcutKey::Tab),
            53 => key = Some(ShortcutKey::Escape),
            51 | 117 => key = Some(ShortcutKey::Delete),
            123 => key = Some(ShortcutKey::ArrowLeft),
            124 => key = Some(ShortcutKey::ArrowRight),
            125 => key = Some(ShortcutKey::ArrowDown),
            126 => key = Some(ShortcutKey::ArrowUp),
            code if (122..=135).contains(&code) => {
                key = Some(ShortcutKey::Function((code - 121) as u8))
            }
            _ => {}
        }
    }

    if modifiers.is_empty() && key.is_none() {
        return display_name_to_chord(display_name);
    }

    Some(ShortcutChord { modifiers, key })
}

fn display_name_to_chord(display_name: &str) -> Option<ShortcutChord> {
    let parts = display_name
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = Vec::new();
    let mut key = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "command" => modifiers.push(ShortcutModifier::Command),
            "control" | "ctrl" => modifiers.push(ShortcutModifier::Control),
            "option" => modifiers.push(ShortcutModifier::Option),
            "alt" => modifiers.push(ShortcutModifier::Alt),
            "shift" => modifiers.push(ShortcutModifier::Shift),
            "fn" => modifiers.push(ShortcutModifier::Fn),
            "space" => key = Some(ShortcutKey::Space),
            "return" | "enter" => key = Some(ShortcutKey::Return),
            "tab" => key = Some(ShortcutKey::Tab),
            "escape" | "esc" => key = Some(ShortcutKey::Escape),
            value if value.len() == 1 => key = Some(ShortcutKey::Character(value.to_string())),
            _ => {}
        }
    }

    Some(ShortcutChord { modifiers, key })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Microphone,
    Accessibility,
    InputMonitoring,
    GlobalShortcut,
    Paste,
    FocusedTextContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequirement {
    pub kind: PermissionKind,
    pub title: String,
    pub description: String,
    pub state: PermissionState,
    pub required: bool,
    pub requestable: bool,
    pub opens_settings: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSnapshot {
    #[serde(default)]
    pub requirements: Vec<PermissionRequirement>,
    #[serde(default)]
    pub all_required_granted: bool,
    #[serde(default)]
    pub microphone: Option<PermissionState>,
    #[serde(default)]
    pub accessibility: Option<PermissionState>,
    #[serde(default)]
    pub input_monitoring: Option<PermissionState>,
    #[serde(default)]
    pub all_granted: Option<bool>,
}

impl Default for PermissionSnapshot {
    fn default() -> Self {
        Self {
            requirements: Vec::new(),
            all_required_granted: false,
            microphone: Some(PermissionState::Unknown),
            accessibility: Some(PermissionState::Unknown),
            input_monitoring: Some(PermissionState::Unknown),
            all_granted: Some(false),
        }
    }
}

impl PermissionSnapshot {
    pub fn macos_compat_ready(&self) -> bool {
        self.microphone == Some(PermissionState::Granted)
            && self.accessibility == Some(PermissionState::Granted)
    }

    pub fn ensure_macos_compat_requirements(&mut self) {
        if self.requirements.is_empty() {
            self.requirements = vec![
                PermissionRequirement {
                    kind: PermissionKind::Microphone,
                    title: "Microphone".into(),
                    description: "Record your voice locally for dictation.".into(),
                    state: self.microphone.clone().unwrap_or(PermissionState::Unknown),
                    required: true,
                    requestable: true,
                    opens_settings: true,
                },
                PermissionRequirement {
                    kind: PermissionKind::Accessibility,
                    title: "Accessibility".into(),
                    description: "Consume the Parrot shortcut event and paste the finished text."
                        .into(),
                    state: self
                        .accessibility
                        .clone()
                        .unwrap_or(PermissionState::Unknown),
                    required: true,
                    requestable: true,
                    opens_settings: true,
                },
            ];

            if let Some(input_monitoring) = &self.input_monitoring {
                if *input_monitoring == PermissionState::Granted
                    || *input_monitoring == PermissionState::Denied
                    || *input_monitoring == PermissionState::NotDetermined
                {
                    self.requirements.push(PermissionRequirement {
                        kind: PermissionKind::InputMonitoring,
                        title: "Input Monitoring".into(),
                        description: "Some Macs require this so Parrot Core can listen for your shortcut while you use other apps.".into(),
                        state: input_monitoring.clone(),
                        required: false,
                        requestable: true,
                        opens_settings: true,
                    });
                }
            }
        }

        self.all_required_granted = self
            .requirements
            .iter()
            .filter(|requirement| requirement.required)
            .all(|requirement| requirement.state == PermissionState::Granted);
        self.all_granted = Some(self.macos_compat_ready());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub uid: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelRole {
    Speech,
    Cleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub role: ModelRole,
    pub display_name: String,
    pub subtitle: String,
    pub expected_bytes: i64,
    pub local_bytes: i64,
    pub progress_bytes: i64,
    pub progress_total_bytes: i64,
    pub downloaded: bool,
    pub downloading: bool,
    #[serde(default)]
    pub required: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResult {
    pub raw: String,
    pub cleaned: String,
    pub audio_duration_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub audio_duration_seconds: f64,
    pub raw_transcription: Option<String>,
    pub cleaned_transcription: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeCorePaths {
    pub app_data_dir: String,
    pub models_dir: String,
    pub speech_models_dir: String,
    pub cleanup_models_dir: String,
    pub resources_dir: String,
    pub shared_resources_dir: String,
    pub temp_dir: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum NativeCoreMethod {
    Initialize,
    PermissionStatuses,
    RequestPermission,
    UpdateSettings,
    WarmModels,
    ModelStatuses,
    DownloadModel,
    DeleteModel,
    ListAudioDevices,
    StartRecording,
    StopRecording,
    StartHotkeyMonitor,
    StopHotkeyMonitor,
    CaptureShortcut,
}

impl NativeCoreMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::PermissionStatuses => "permissionStatuses",
            Self::RequestPermission => "requestPermission",
            Self::UpdateSettings => "updateSettings",
            Self::WarmModels => "warmModels",
            Self::ModelStatuses => "modelStatuses",
            Self::DownloadModel => "downloadModel",
            Self::DeleteModel => "deleteModel",
            Self::ListAudioDevices => "listAudioDevices",
            Self::StartRecording => "startRecording",
            Self::StopRecording => "stopRecording",
            Self::StartHotkeyMonitor => "startHotkeyMonitor",
            Self::StopHotkeyMonitor => "stopHotkeyMonitor",
            Self::CaptureShortcut => "captureShortcut",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCoreRequest {
    pub id: String,
    pub method: NativeCoreMethod,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCoreResponse {
    pub id: String,
    pub ok: bool,
    pub payload: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCoreEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SoundEvent {
    RecordingStart,
    RecordingSuccess,
    RecordingCancel,
    RecordingError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_legacy_shortcut_shape_into_platform_codes() {
        let value = serde_json::json!({
            "displayName": "Fn",
            "macosKeyCodes": [63],
            "mode": "hold"
        });

        let shortcut: ShortcutSettings = serde_json::from_value(value).unwrap();

        assert_eq!(shortcut.macos_key_codes(), &[63]);
        assert_eq!(
            shortcut.chord,
            Some(ShortcutChord {
                modifiers: vec![ShortcutModifier::Fn],
                key: None
            })
        );
        assert!(shortcut.enabled);
        assert!(!shortcut.double_tap_toggle);
    }

    #[test]
    fn serializes_shortcut_without_top_level_macos_key_codes() {
        let value = serde_json::to_value(default_hands_free_shortcut()).unwrap();

        assert!(value.get("macosKeyCodes").is_none());
        assert_eq!(
            value["platformCodes"]["macosKeyCodes"],
            serde_json::json!([59, 49])
        );
        assert_eq!(value["chord"]["modifiers"], serde_json::json!(["control"]));
        assert_eq!(value["chord"]["key"], serde_json::json!("space"));
    }

    #[test]
    fn native_core_method_preserves_wire_names() {
        assert_eq!(
            NativeCoreMethod::PermissionStatuses.as_str(),
            "permissionStatuses"
        );
        assert_eq!(
            serde_json::to_value(NativeCoreMethod::CaptureShortcut).unwrap(),
            serde_json::json!("captureShortcut")
        );
    }
}
