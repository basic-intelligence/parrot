use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::PathBuf};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const DEFAULT_PUSH_TO_TALK_DISPLAY_NAME: &str = "Fn";
const DEFAULT_PUSH_TO_TALK_KEY_CODES: [u16; 1] = [63];
const DEFAULT_HANDS_FREE_DISPLAY_NAME: &str = "Control + Space";
const DEFAULT_HANDS_FREE_KEY_CODES: [u16; 2] = [59, 49];
const DEFAULT_CLEANUP_MODEL_ID: &str = "cleanup";
const GEMMA_CLEANUP_MODEL_ID: &str = "cleanup-gemma-4-e2b";
const LANGUAGE_CATALOG_JSON: &str = include_str!("../../native-core/shared/languages.json");

#[derive(Debug, Deserialize)]
struct SharedLanguageOption {
    code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub term: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
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
    pub cleanup_enabled: bool,
    #[serde(default)]
    pub cleanup_prompt: String,
    #[serde(default)]
    pub dictionary_entries: Vec<DictionaryEntry>,
    pub play_sounds: bool,
    #[serde(default)]
    pub paste_into_recording_start_window: bool,
    pub history_enabled: bool,
    pub launch_at_login: bool,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub input_monitoring_permission_shown_in_onboarding: bool,
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
pub struct ShortcutSettings {
    pub display_name: String,
    pub macos_key_codes: Vec<u16>,
    pub mode: ShortcutMode,
    #[serde(default = "default_shortcut_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub double_tap_toggle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutMode {
    Hold,
    Toggle,
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

fn default_push_to_talk_shortcut() -> ShortcutSettings {
    ShortcutSettings {
        display_name: DEFAULT_PUSH_TO_TALK_DISPLAY_NAME.into(),
        macos_key_codes: DEFAULT_PUSH_TO_TALK_KEY_CODES.to_vec(),
        mode: ShortcutMode::Hold,
        enabled: true,
        double_tap_toggle: false,
    }
}

fn default_hands_free_shortcut() -> ShortcutSettings {
    ShortcutSettings {
        display_name: DEFAULT_HANDS_FREE_DISPLAY_NAME.into(),
        macos_key_codes: DEFAULT_HANDS_FREE_KEY_CODES.to_vec(),
        mode: ShortcutMode::Toggle,
        enabled: true,
        double_tap_toggle: false,
    }
}

fn default_shortcut_enabled() -> bool {
    true
}

fn default_cleanup_model_id() -> String {
    DEFAULT_CLEANUP_MODEL_ID.to_string()
}

fn normalize_dictation_language(settings: &mut AppSettings) -> bool {
    let original_mode = settings.dictation_language_mode.clone();
    let original_code = settings.dictation_language_code.clone();

    match settings.dictation_language_mode {
        DictationLanguageMode::English => {
            settings.dictation_language_code = None;
        }
        DictationLanguageMode::Detect => {
            settings.dictation_language_code = None;
        }
        DictationLanguageMode::Specific => {
            let code = canonical_dictation_language_code(
                settings
                    .dictation_language_code
                    .as_deref()
                    .unwrap_or_default(),
            );

            if code.as_deref() == Some("en") {
                settings.dictation_language_mode = DictationLanguageMode::English;
                settings.dictation_language_code = None;
            } else if let Some(code) = code {
                settings.dictation_language_code = Some(code);
            } else {
                settings.dictation_language_mode = DictationLanguageMode::English;
                settings.dictation_language_code = None;
            }
        }
    }

    original_mode != settings.dictation_language_mode
        || original_code != settings.dictation_language_code
}

fn canonical_dictation_language_code(code: &str) -> Option<String> {
    let normalized = code.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let canonical = shared_language_options()
        .into_iter()
        .find(|language| language.code.eq_ignore_ascii_case(&normalized))
        .map(|language| language.code);

    Some(canonical.unwrap_or(normalized))
}

fn shared_language_options() -> Vec<SharedLanguageOption> {
    serde_json::from_str(LANGUAGE_CATALOG_JSON).expect("shared language catalog must be valid JSON")
}

fn normalize_cleanup_model_id(settings: &mut AppSettings) -> bool {
    let original = settings.cleanup_model_id.clone();
    let id = settings.cleanup_model_id.trim();
    settings.cleanup_model_id = match id {
        DEFAULT_CLEANUP_MODEL_ID => DEFAULT_CLEANUP_MODEL_ID.to_string(),
        GEMMA_CLEANUP_MODEL_ID => GEMMA_CLEANUP_MODEL_ID.to_string(),
        _ => default_cleanup_model_id(),
    };

    original != settings.cleanup_model_id
}

fn normalize_dictionary_entries(settings: &mut AppSettings) -> bool {
    let original_entries = settings.dictionary_entries.clone();
    let mut seen_terms = HashSet::new();
    let mut normalized_entries = Vec::new();

    for entry in &settings.dictionary_entries {
        let term = normalize_dictionary_term(&entry.term);
        if term.is_empty() {
            continue;
        }

        if seen_terms.insert(term.to_lowercase()) {
            normalized_entries.push(DictionaryEntry {
                id: if entry.id.trim().is_empty() {
                    Uuid::new_v4().to_string()
                } else {
                    entry.id.clone()
                },
                term,
            });
        }
    }

    let changed = original_entries != normalized_entries;
    if changed {
        settings.dictionary_entries = normalized_entries;
    }
    changed
}

fn normalize_dictionary_term(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_shortcuts(settings: &mut AppSettings) -> bool {
    let original_push_to_talk = settings.push_to_talk_shortcut.clone();
    let original_hands_free = settings.hands_free_shortcut.clone();

    if settings.push_to_talk_shortcut.macos_key_codes.is_empty()
        || settings
            .push_to_talk_shortcut
            .display_name
            .trim()
            .is_empty()
    {
        settings.push_to_talk_shortcut = default_push_to_talk_shortcut();
    }
    if !matches!(settings.push_to_talk_shortcut.mode, ShortcutMode::Hold) {
        settings.push_to_talk_shortcut.mode = ShortcutMode::Hold;
    }

    if settings.hands_free_shortcut.macos_key_codes.is_empty()
        || settings.hands_free_shortcut.display_name.trim().is_empty()
    {
        settings.hands_free_shortcut = default_hands_free_shortcut();
    }
    if !matches!(settings.hands_free_shortcut.mode, ShortcutMode::Toggle) {
        settings.hands_free_shortcut.mode = ShortcutMode::Toggle;
    }

    original_push_to_talk != settings.push_to_talk_shortcut
        || original_hands_free != settings.hands_free_shortcut
}

pub struct SettingsStore {
    pub settings: AppSettings,
    path: PathBuf,
}

impl SettingsStore {
    pub fn load(app: &AppHandle) -> anyhow::Result<Self> {
        let dir = app.path().app_data_dir().context("missing app data dir")?;
        fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");
        let (mut settings, mut migrated) = if path.exists() {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            let missing_cleanup_model = value.get("cleanupModelId").is_none();
            let missing_paste_target_setting = value.get("pasteIntoRecordingStartWindow").is_none();
            (
                serde_json::from_value(value)?,
                missing_cleanup_model || missing_paste_target_setting,
            )
        } else {
            (AppSettings::default(), false)
        };

        migrated |= normalize_shortcuts(&mut settings);
        migrated |= normalize_dictionary_entries(&mut settings);
        migrated |= normalize_dictation_language(&mut settings);
        migrated |= normalize_cleanup_model_id(&mut settings);

        if migrated {
            fs::write(&path, serde_json::to_vec_pretty(&settings)?)?;
        }
        Ok(Self { settings, path })
    }

    pub fn save(&mut self, mut settings: AppSettings) -> anyhow::Result<()> {
        normalize_shortcuts(&mut settings);
        normalize_dictionary_entries(&mut settings);
        normalize_dictation_language(&mut settings);
        normalize_cleanup_model_id(&mut settings);
        self.settings = settings;
        fs::write(&self.path, serde_json::to_vec_pretty(&self.settings)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shortcuts_use_fn_push_to_talk() {
        let settings = AppSettings::default();

        assert_eq!(settings.push_to_talk_shortcut.display_name, "Fn");
        assert_eq!(settings.push_to_talk_shortcut.macos_key_codes, vec![63]);
        assert_eq!(settings.hands_free_shortcut.display_name, "Control + Space");
        assert_eq!(settings.hands_free_shortcut.macos_key_codes, vec![59, 49]);
    }

    #[test]
    fn deserializes_missing_shortcut_flags_to_back_compatible_defaults() {
        let value = serde_json::json!({
            "pushToTalkShortcut": {
                "displayName": "Fn",
                "macosKeyCodes": [63],
                "mode": "hold"
            },
            "handsFreeShortcut": {
                "displayName": "Control + Space",
                "macosKeyCodes": [59, 49],
                "mode": "toggle"
            },
            "cleanupEnabled": true,
            "playSounds": true,
            "historyEnabled": false,
            "launchAtLogin": false
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();

        assert!(settings.push_to_talk_shortcut.enabled);
        assert!(!settings.push_to_talk_shortcut.double_tap_toggle);
        assert!(settings.hands_free_shortcut.enabled);
        assert!(!settings.hands_free_shortcut.double_tap_toggle);
        assert!(!settings.paste_into_recording_start_window);
        assert!(!settings.input_monitoring_permission_shown_in_onboarding);
    }

    #[test]
    fn normalizes_dictionary_entries_to_terms_only() {
        let mut settings = AppSettings {
            dictionary_entries: vec![
                DictionaryEntry {
                    id: "entry-1".into(),
                    term: " Project   Atlas ".into(),
                },
                DictionaryEntry {
                    id: "entry-2".into(),
                    term: "project atlas".into(),
                },
            ],
            ..AppSettings::default()
        };

        assert!(normalize_dictionary_entries(&mut settings));
        assert_eq!(settings.dictionary_entries.len(), 1);
        assert_eq!(settings.dictionary_entries[0].term, "Project Atlas");
    }

    #[test]
    fn serializes_simplified_settings_shape() {
        let mut settings = AppSettings {
            dictionary_entries: vec![DictionaryEntry {
                id: "entry-1".into(),
                term: "Project Atlas".into(),
            }],
            ..AppSettings::default()
        };
        normalize_dictionary_entries(&mut settings);

        let value = serde_json::to_value(&settings).unwrap();
        assert!(value.get(concat!("auto", "AddDictionary")).is_none());
        assert!(value.get(concat!("commonly", "MisheardDraft")).is_none());
        assert!(value.get("shortcut").is_none());
        assert!(value.get("pushToTalkShortcut").is_some());
        assert!(value.get("handsFreeShortcut").is_some());
        assert_eq!(
            value
                .get("pushToTalkShortcut")
                .and_then(|value| value.get("enabled"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            value
                .get("pushToTalkShortcut")
                .and_then(|value| value.get("doubleTapToggle"))
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            value.get("cleanupModelId").and_then(|v| v.as_str()),
            Some("cleanup")
        );
        assert_eq!(
            value
                .get("pasteIntoRecordingStartWindow")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(value
            .as_object()
            .unwrap()
            .keys()
            .all(|key| !key.ends_with("CleanupModelId") || key == "cleanupModelId"));
        assert_eq!(
            value
                .get("inputMonitoringPermissionShownInOnboarding")
                .and_then(|v| v.as_bool()),
            Some(false)
        );

        let entries = value
            .get("dictionaryEntries")
            .and_then(|value| value.as_array())
            .unwrap();
        let entry = entries[0].as_object().unwrap();
        assert!(entry.get("source").is_none());
        assert!(entry.get(concat!("use", "Count")).is_none());
    }

    #[test]
    fn normalizes_valid_specific_language() {
        let mut settings = AppSettings {
            dictation_language_mode: DictationLanguageMode::Specific,
            dictation_language_code: Some(" ES ".into()),
            ..AppSettings::default()
        };

        assert!(normalize_dictation_language(&mut settings));
        assert!(matches!(
            settings.dictation_language_mode,
            DictationLanguageMode::Specific
        ));
        assert_eq!(settings.dictation_language_code.as_deref(), Some("es"));
    }

    #[test]
    fn normalizes_specific_language_locale_to_catalog_casing() {
        let mut settings = AppSettings {
            dictation_language_mode: DictationLanguageMode::Specific,
            dictation_language_code: Some(" pt-br ".into()),
            ..AppSettings::default()
        };

        assert!(normalize_dictation_language(&mut settings));
        assert!(matches!(
            settings.dictation_language_mode,
            DictationLanguageMode::Specific
        ));
        assert_eq!(settings.dictation_language_code.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn english_specific_locale_stays_specific_to_preserve_locale() {
        let mut settings = AppSettings {
            dictation_language_mode: DictationLanguageMode::Specific,
            dictation_language_code: Some("en-gb".into()),
            ..AppSettings::default()
        };

        assert!(normalize_dictation_language(&mut settings));
        assert!(matches!(
            settings.dictation_language_mode,
            DictationLanguageMode::Specific
        ));
        assert_eq!(settings.dictation_language_code.as_deref(), Some("en-GB"));
    }

    #[test]
    fn empty_specific_language_falls_back_to_english() {
        let mut settings = AppSettings {
            dictation_language_mode: DictationLanguageMode::Specific,
            dictation_language_code: Some(" ".into()),
            ..AppSettings::default()
        };

        assert!(normalize_dictation_language(&mut settings));
        assert!(matches!(
            settings.dictation_language_mode,
            DictationLanguageMode::English
        ));
        assert!(settings.dictation_language_code.is_none());
    }

    #[test]
    fn english_specific_language_routes_to_english_mode() {
        let mut settings = AppSettings {
            dictation_language_mode: DictationLanguageMode::Specific,
            dictation_language_code: Some("en".into()),
            ..AppSettings::default()
        };

        assert!(normalize_dictation_language(&mut settings));
        assert!(matches!(
            settings.dictation_language_mode,
            DictationLanguageMode::English
        ));
        assert!(settings.dictation_language_code.is_none());
    }

    #[test]
    fn normalize_cleanup_model_id_defaults_unknown_id() {
        let mut settings = AppSettings {
            cleanup_model_id: "invalid-model-id".into(),
            ..AppSettings::default()
        };

        assert!(normalize_cleanup_model_id(&mut settings));
        assert_eq!(settings.cleanup_model_id, DEFAULT_CLEANUP_MODEL_ID);
    }

    #[test]
    fn normalize_cleanup_model_id_defaults_empty_id() {
        let mut settings = AppSettings {
            cleanup_model_id: " ".into(),
            ..AppSettings::default()
        };

        assert!(normalize_cleanup_model_id(&mut settings));
        assert_eq!(settings.cleanup_model_id, DEFAULT_CLEANUP_MODEL_ID);
    }

    #[test]
    fn deserializes_missing_cleanup_model_id_to_default() {
        let value = serde_json::json!({
            "cleanupEnabled": true,
            "playSounds": true,
            "historyEnabled": false,
            "launchAtLogin": false
        });
        let settings: AppSettings = serde_json::from_value(value).unwrap();

        assert_eq!(settings.cleanup_model_id, DEFAULT_CLEANUP_MODEL_ID);
    }
}
