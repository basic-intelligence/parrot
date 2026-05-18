use parrot_language::canonical_language_code;
pub use parrot_protocol::{
    default_hands_free_shortcut, default_push_to_talk_shortcut, AppSettings, DictationLanguageMode,
    DictionaryEntry, ShortcutMode, ShortcutSettings, DEFAULT_CLEANUP_MODEL_ID,
    GEMMA_CLEANUP_MODEL_ID,
};
use std::collections::HashSet;
use uuid::Uuid;

pub fn normalize_settings(settings: &mut AppSettings) -> bool {
    let mut migrated = false;
    migrated |= normalize_shortcuts(settings);
    migrated |= normalize_dictionary_entries(settings);
    migrated |= normalize_dictation_language(settings);
    migrated |= normalize_cleanup_model_id(settings);
    migrated
}

pub fn normalize_dictation_language(settings: &mut AppSettings) -> bool {
    let original_mode = settings.dictation_language_mode.clone();
    let original_code = settings.dictation_language_code.clone();

    match settings.dictation_language_mode {
        DictationLanguageMode::English | DictationLanguageMode::Detect => {
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

pub fn canonical_dictation_language_code(code: &str) -> Option<String> {
    canonical_language_code(code)
}

pub fn normalize_cleanup_model_id(settings: &mut AppSettings) -> bool {
    let original = settings.cleanup_model_id.clone();
    let id = settings.cleanup_model_id.trim();
    settings.cleanup_model_id = match id {
        DEFAULT_CLEANUP_MODEL_ID => DEFAULT_CLEANUP_MODEL_ID.to_string(),
        GEMMA_CLEANUP_MODEL_ID => GEMMA_CLEANUP_MODEL_ID.to_string(),
        _ => DEFAULT_CLEANUP_MODEL_ID.to_string(),
    };

    original != settings.cleanup_model_id
}

pub fn normalize_dictionary_entries(settings: &mut AppSettings) -> bool {
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

pub fn normalize_dictionary_term(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn normalize_shortcuts(settings: &mut AppSettings) -> bool {
    let original_push_to_talk = settings.push_to_talk_shortcut.clone();
    let original_hands_free = settings.hands_free_shortcut.clone();

    if settings.push_to_talk_shortcut.macos_key_codes().is_empty()
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

    if settings.hands_free_shortcut.macos_key_codes().is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::{ShortcutChord, ShortcutKey, ShortcutModifier};

    #[test]
    fn default_shortcuts_use_platform_neutral_shape_with_macos_codes() {
        let settings = AppSettings::default();

        assert_eq!(settings.push_to_talk_shortcut.display_name, "Fn");
        assert_eq!(settings.push_to_talk_shortcut.macos_key_codes(), &[63]);
        assert_eq!(settings.hands_free_shortcut.display_name, "Control + Space");
        assert_eq!(settings.hands_free_shortcut.macos_key_codes(), &[59, 49]);
        assert_eq!(
            settings.hands_free_shortcut.chord,
            Some(ShortcutChord {
                modifiers: vec![ShortcutModifier::Control],
                key: Some(ShortcutKey::Space),
            })
        );
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
    fn migrates_old_macos_key_codes_into_platform_codes_and_chord() {
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
    fn serializes_platform_neutral_settings_shape() {
        let mut settings = AppSettings {
            dictionary_entries: vec![DictionaryEntry {
                id: "entry-1".into(),
                term: "Project Atlas".into(),
            }],
            ..AppSettings::default()
        };
        normalize_dictionary_entries(&mut settings);

        let value = serde_json::to_value(&settings).unwrap();
        assert!(value.get("shortcut").is_none());
        assert!(value.get("pushToTalkShortcut").is_some());
        assert!(value["pushToTalkShortcut"].get("macosKeyCodes").is_none());
        assert_eq!(
            value["pushToTalkShortcut"]["platformCodes"]["macosKeyCodes"],
            serde_json::json!([63])
        );
        assert_eq!(value["cleanupModelId"], serde_json::json!("cleanup"));
        assert_eq!(
            value["pasteIntoRecordingStartWindow"],
            serde_json::json!(false)
        );
        assert_eq!(
            value["inputMonitoringPermissionShownInOnboarding"],
            serde_json::json!(false)
        );

        let entries = value
            .get("dictionaryEntries")
            .and_then(|value| value.as_array())
            .unwrap();
        let entry = entries[0].as_object().unwrap();
        assert!(entry.get("source").is_none());
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
