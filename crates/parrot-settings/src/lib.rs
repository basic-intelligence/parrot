use parrot_language::canonical_language_code;
pub use parrot_protocol::{
    default_hands_free_shortcut, default_linux_hands_free_shortcut,
    default_linux_push_to_talk_shortcut, default_push_to_talk_shortcut,
    default_windows_hands_free_shortcut, default_windows_push_to_talk_shortcut, AppSettings,
    DictationLanguageMode, DictionaryEntry, ShortcutChord, ShortcutKey, ShortcutMode,
    ShortcutModifier, ShortcutPlatformCodes, ShortcutSettings, DEFAULT_CLEANUP_MODEL_ID,
    GEMMA_CLEANUP_MODEL_ID,
};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPlatform {
    Macos,
    Windows,
    Linux,
}

const OLD_WINDOWS_HANDS_FREE_DEFAULT_KEYS: &[u16] = &[17, 18, 32];
const INTERMEDIATE_WINDOWS_HANDS_FREE_DEFAULT_KEYS: &[u16] = &[162, 164, 32];

pub fn normalize_settings(settings: &mut AppSettings) -> bool {
    normalize_settings_for_platform(settings, SettingsPlatform::Macos)
}

pub fn default_settings_for_platform(platform: SettingsPlatform) -> AppSettings {
    let mut settings = AppSettings::default();
    normalize_settings_for_platform(&mut settings, platform);
    settings
}

pub fn normalize_settings_for_platform(
    settings: &mut AppSettings,
    platform: SettingsPlatform,
) -> bool {
    let mut migrated = false;
    migrated |= normalize_shortcuts_for_platform(settings, platform);
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
    normalize_shortcuts_for_platform(settings, SettingsPlatform::Macos)
}

pub fn normalize_shortcuts_for_platform(
    settings: &mut AppSettings,
    platform: SettingsPlatform,
) -> bool {
    let original_push_to_talk = settings.push_to_talk_shortcut.clone();
    let original_hands_free = settings.hands_free_shortcut.clone();

    match platform {
        SettingsPlatform::Macos => {
            if settings.push_to_talk_shortcut.macos_key_codes().is_empty()
                || settings
                    .push_to_talk_shortcut
                    .display_name
                    .trim()
                    .is_empty()
            {
                settings.push_to_talk_shortcut = default_push_to_talk_shortcut();
            }

            if settings.hands_free_shortcut.macos_key_codes().is_empty()
                || settings.hands_free_shortcut.display_name.trim().is_empty()
            {
                settings.hands_free_shortcut = default_hands_free_shortcut();
            }
        }
        SettingsPlatform::Windows => {
            if settings
                .push_to_talk_shortcut
                .windows_virtual_keys()
                .is_empty()
                || settings
                    .push_to_talk_shortcut
                    .display_name
                    .trim()
                    .is_empty()
            {
                settings.push_to_talk_shortcut = windows_shortcut_or_default(
                    &settings.push_to_talk_shortcut,
                    default_windows_push_to_talk_shortcut(),
                );
            }

            if settings
                .hands_free_shortcut
                .windows_virtual_keys()
                .is_empty()
                || settings.hands_free_shortcut.display_name.trim().is_empty()
            {
                settings.hands_free_shortcut = windows_shortcut_or_default(
                    &settings.hands_free_shortcut,
                    default_windows_hands_free_shortcut(),
                );
            }

            if is_windows_hands_free_default_to_migrate(&settings.hands_free_shortcut) {
                settings.hands_free_shortcut = default_windows_hands_free_shortcut();
            }
        }
        SettingsPlatform::Linux => {
            if settings.push_to_talk_shortcut.linux_key_codes().is_empty()
                || settings
                    .push_to_talk_shortcut
                    .display_name
                    .trim()
                    .is_empty()
            {
                settings.push_to_talk_shortcut = linux_shortcut_or_default(
                    &settings.push_to_talk_shortcut,
                    default_linux_push_to_talk_shortcut(),
                );
            }

            if settings.hands_free_shortcut.linux_key_codes().is_empty()
                || settings.hands_free_shortcut.display_name.trim().is_empty()
            {
                settings.hands_free_shortcut = linux_shortcut_or_default(
                    &settings.hands_free_shortcut,
                    default_linux_hands_free_shortcut(),
                );
            }
        }
    }

    if !matches!(settings.push_to_talk_shortcut.mode, ShortcutMode::Hold) {
        settings.push_to_talk_shortcut.mode = ShortcutMode::Hold;
    }
    if !matches!(settings.hands_free_shortcut.mode, ShortcutMode::Toggle) {
        settings.hands_free_shortcut.mode = ShortcutMode::Toggle;
    }

    original_push_to_talk != settings.push_to_talk_shortcut
        || original_hands_free != settings.hands_free_shortcut
}

fn windows_shortcut_or_default(
    shortcut: &ShortcutSettings,
    fallback: ShortcutSettings,
) -> ShortcutSettings {
    if is_macos_default_shortcut(shortcut) {
        return fallback;
    }

    let Some(keys) = windows_virtual_keys_from_chord(shortcut.chord.as_ref(), &shortcut.mode)
    else {
        return fallback;
    };

    let mut shortcut = shortcut.clone();
    if shortcut.display_name.trim().is_empty() {
        shortcut.display_name = display_name_for_windows_keys(&keys);
    }
    shortcut.platform_codes = ShortcutPlatformCodes {
        macos_key_codes: None,
        windows_virtual_keys: Some(keys),
        linux_key_codes: None,
    };
    shortcut
}

fn linux_shortcut_or_default(
    shortcut: &ShortcutSettings,
    fallback: ShortcutSettings,
) -> ShortcutSettings {
    if is_macos_default_shortcut(shortcut) {
        return fallback;
    }

    let Some(keys) = linux_key_codes_from_chord(shortcut.chord.as_ref(), &shortcut.mode) else {
        return fallback;
    };

    let mut shortcut = shortcut.clone();
    if shortcut.display_name.trim().is_empty() {
        shortcut.display_name = display_name_for_linux_keys(&keys);
    }
    shortcut.platform_codes = ShortcutPlatformCodes {
        macos_key_codes: None,
        windows_virtual_keys: None,
        linux_key_codes: Some(keys),
    };
    shortcut
}

fn is_macos_default_shortcut(shortcut: &ShortcutSettings) -> bool {
    shortcut.macos_key_codes() == [63]
        || shortcut.macos_key_codes() == [59, 49]
        || shortcut.display_name == "Fn"
        || shortcut.display_name == "Control + Space"
}

fn is_windows_hands_free_default_to_migrate(shortcut: &ShortcutSettings) -> bool {
    let keys = shortcut.windows_virtual_keys();

    let matches_known_default = (shortcut.display_name == "Ctrl + Alt + Space"
        && keys == OLD_WINDOWS_HANDS_FREE_DEFAULT_KEYS)
        || (shortcut.display_name == "Left Ctrl + Left Alt + Space"
            && keys == INTERMEDIATE_WINDOWS_HANDS_FREE_DEFAULT_KEYS);

    matches_known_default
        && shortcut.mode == ShortcutMode::Toggle
        && shortcut.enabled
        && !shortcut.double_tap_toggle
        && shortcut.macos_key_codes().is_empty()
        && shortcut
            .platform_codes
            .linux_key_codes
            .as_deref()
            .unwrap_or_default()
            .is_empty()
}

fn windows_virtual_keys_from_chord(
    chord: Option<&ShortcutChord>,
    mode: &ShortcutMode,
) -> Option<Vec<u16>> {
    let chord = chord?;
    let mut keys = Vec::new();
    for modifier in &chord.modifiers {
        match modifier {
            ShortcutModifier::Control => keys.push(17),
            ShortcutModifier::Alt | ShortcutModifier::Option => keys.push(18),
            ShortcutModifier::Shift => keys.push(16),
            ShortcutModifier::Meta | ShortcutModifier::Command => keys.push(91),
            ShortcutModifier::Fn => return None,
        }
    }

    if let Some(key) = &chord.key {
        keys.push(windows_virtual_key_for_key(key)?);
    } else if matches!(mode, ShortcutMode::Hold)
        && chord.modifiers.as_slice() == [ShortcutModifier::Control]
    {
        keys = vec![163];
    } else {
        return None;
    }

    keys.sort_by_key(|key| windows_key_sort_key(*key));
    keys.dedup();
    Some(keys)
}

fn windows_virtual_key_for_key(key: &ShortcutKey) -> Option<u16> {
    match key {
        ShortcutKey::Space => Some(32),
        ShortcutKey::Return => Some(13),
        ShortcutKey::Tab => Some(9),
        ShortcutKey::Escape => None,
        ShortcutKey::Character(value) => value
            .chars()
            .next()
            .filter(|character| character.is_ascii_alphanumeric())
            .map(|character| character.to_ascii_uppercase() as u16),
        ShortcutKey::Function(number) if (1..=24).contains(number) => {
            Some(0x70 + u16::from(*number) - 1)
        }
        ShortcutKey::Function(_) => None,
        ShortcutKey::ArrowLeft => Some(37),
        ShortcutKey::ArrowRight => Some(39),
        ShortcutKey::ArrowUp => Some(38),
        ShortcutKey::ArrowDown => Some(40),
        ShortcutKey::Delete => Some(46),
    }
}

fn linux_key_codes_from_chord(
    chord: Option<&ShortcutChord>,
    mode: &ShortcutMode,
) -> Option<Vec<u32>> {
    let chord = chord?;
    let mut keys = Vec::new();
    for modifier in &chord.modifiers {
        match modifier {
            ShortcutModifier::Control => keys.push(0xffe3),
            ShortcutModifier::Alt | ShortcutModifier::Option => keys.push(0xffe9),
            ShortcutModifier::Shift => keys.push(0xffe1),
            ShortcutModifier::Meta | ShortcutModifier::Command => keys.push(0xffeb),
            ShortcutModifier::Fn => return None,
        }
    }

    if let Some(key) = &chord.key {
        keys.push(linux_key_code_for_key(key)?);
    } else if matches!(mode, ShortcutMode::Hold) {
        return None;
    } else {
        return None;
    }

    keys.sort_by_key(|key| linux_key_sort_key(*key));
    keys.dedup();
    Some(keys)
}

fn linux_key_code_for_key(key: &ShortcutKey) -> Option<u32> {
    match key {
        ShortcutKey::Space => Some(0x20),
        ShortcutKey::Return => Some(0xff0d),
        ShortcutKey::Tab => Some(0xff09),
        ShortcutKey::Escape => None,
        ShortcutKey::Character(value) => value
            .chars()
            .next()
            .filter(|character| character.is_ascii_alphanumeric())
            .map(|character| character.to_ascii_lowercase() as u32),
        ShortcutKey::Function(number) if (1..=24).contains(number) => {
            Some(0xffbe + u32::from(*number) - 1)
        }
        ShortcutKey::Function(_) => None,
        ShortcutKey::ArrowLeft => Some(0xff51),
        ShortcutKey::ArrowUp => Some(0xff52),
        ShortcutKey::ArrowRight => Some(0xff53),
        ShortcutKey::ArrowDown => Some(0xff54),
        ShortcutKey::Delete => Some(0xffff),
    }
}

fn linux_key_sort_key(key: u32) -> (u8, u32) {
    let rank = match key {
        0xffe3 | 0xffe4 => 0,
        0xffe9 | 0xffea => 1,
        0xffe1 | 0xffe2 => 2,
        0xffeb | 0xffec => 3,
        _ => 4,
    };
    (rank, key)
}

fn display_name_for_linux_keys(keys: &[u32]) -> String {
    keys.iter()
        .map(|key| match *key {
            0x20 => "Space".to_string(),
            0xff09 => "Tab".to_string(),
            0xff0d => "Enter".to_string(),
            0xff51 => "Left Arrow".to_string(),
            0xff52 => "Up Arrow".to_string(),
            0xff53 => "Right Arrow".to_string(),
            0xff54 => "Down Arrow".to_string(),
            0xffff => "Delete".to_string(),
            0xffe1 | 0xffe2 => "Shift".to_string(),
            0xffe3 | 0xffe4 => "Ctrl".to_string(),
            0xffe9 | 0xffea => "Alt".to_string(),
            0xffeb | 0xffec => "Meta".to_string(),
            key if (0xffbe..=0xffd5).contains(&key) => format!("F{}", key - 0xffbe + 1),
            key if (b'0' as u32..=b'9' as u32).contains(&key)
                || (b'a' as u32..=b'z' as u32).contains(&key)
                || (b'A' as u32..=b'Z' as u32).contains(&key) =>
            {
                char::from_u32(key)
                    .unwrap_or('?')
                    .to_ascii_uppercase()
                    .to_string()
            }
            key => format!("Key {key:#x}"),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn windows_key_sort_key(key: u16) -> (u8, u16) {
    let rank = match key {
        17 | 162 | 163 => 0,
        18 | 164 | 165 => 1,
        16 | 160 | 161 => 2,
        91 | 92 => 3,
        _ => 4,
    };
    (rank, key)
}

fn display_name_for_windows_keys(keys: &[u16]) -> String {
    keys.iter()
        .map(|key| match *key {
            9 => "Tab".to_string(),
            13 => "Enter".to_string(),
            16 => "Shift".to_string(),
            17 => "Ctrl".to_string(),
            18 => "Alt".to_string(),
            32 => "Space".to_string(),
            160 => "Left Shift".to_string(),
            161 => "Right Shift".to_string(),
            162 => "Left Ctrl".to_string(),
            163 => "Right Ctrl".to_string(),
            164 => "Left Alt".to_string(),
            165 => "Right Alt".to_string(),
            37 => "Left Arrow".to_string(),
            38 => "Up Arrow".to_string(),
            39 => "Right Arrow".to_string(),
            40 => "Down Arrow".to_string(),
            46 => "Delete".to_string(),
            91 => "Windows".to_string(),
            92 => "Right Windows".to_string(),
            key if (0x70..=0x87).contains(&key) => format!("F{}", key - 0x70 + 1),
            key if (b'0' as u16..=b'9' as u16).contains(&key)
                || (b'A' as u16..=b'Z' as u16).contains(&key) =>
            {
                char::from_u32(u32::from(key)).unwrap_or('?').to_string()
            }
            key => format!("VK {key}"),
        })
        .collect::<Vec<_>>()
        .join(" + ")
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
    fn normalizes_old_macos_shortcuts_to_windows_defaults() {
        let mut settings = AppSettings {
            push_to_talk_shortcut: serde_json::from_value(serde_json::json!({
                "displayName": "Fn",
                "macosKeyCodes": [63],
                "mode": "hold"
            }))
            .unwrap(),
            hands_free_shortcut: serde_json::from_value(serde_json::json!({
                "displayName": "Control + Space",
                "macosKeyCodes": [59, 49],
                "mode": "toggle"
            }))
            .unwrap(),
            ..AppSettings::default()
        };

        assert!(normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Windows
        ));

        assert_eq!(settings.push_to_talk_shortcut.display_name, "Right Ctrl");
        assert_eq!(
            settings.push_to_talk_shortcut.windows_virtual_keys(),
            &[163]
        );
        assert!(settings.push_to_talk_shortcut.macos_key_codes().is_empty());
        assert!(matches!(
            settings.push_to_talk_shortcut.mode,
            ShortcutMode::Hold
        ));

        assert_eq!(
            settings.hands_free_shortcut.display_name,
            "Left Ctrl + Space"
        );
        assert_eq!(
            settings.hands_free_shortcut.windows_virtual_keys(),
            &[162, 32]
        );
        assert!(settings.hands_free_shortcut.macos_key_codes().is_empty());
        assert!(matches!(
            settings.hands_free_shortcut.mode,
            ShortcutMode::Toggle
        ));
    }

    #[test]
    fn migrates_old_windows_hands_free_default_to_left_control_space() {
        let mut settings = default_settings_for_platform(SettingsPlatform::Windows);
        settings.hands_free_shortcut = ShortcutSettings {
            display_name: "Ctrl + Alt + Space".into(),
            mode: ShortcutMode::Toggle,
            enabled: true,
            double_tap_toggle: false,
            chord: Some(ShortcutChord {
                modifiers: vec![ShortcutModifier::Control, ShortcutModifier::Alt],
                key: Some(ShortcutKey::Space),
            }),
            platform_codes: ShortcutPlatformCodes {
                macos_key_codes: None,
                windows_virtual_keys: Some(vec![17, 18, 32]),
                linux_key_codes: None,
            },
        };

        assert!(normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Windows
        ));

        assert_eq!(
            settings.hands_free_shortcut.display_name,
            "Left Ctrl + Space"
        );
        assert_eq!(
            settings.hands_free_shortcut.windows_virtual_keys(),
            &[162, 32]
        );
    }

    #[test]
    fn migrates_intermediate_windows_hands_free_default_to_left_control_space() {
        let mut settings = default_settings_for_platform(SettingsPlatform::Windows);
        settings.hands_free_shortcut = ShortcutSettings {
            display_name: "Left Ctrl + Left Alt + Space".into(),
            mode: ShortcutMode::Toggle,
            enabled: true,
            double_tap_toggle: false,
            chord: Some(ShortcutChord {
                modifiers: vec![ShortcutModifier::Control, ShortcutModifier::Alt],
                key: Some(ShortcutKey::Space),
            }),
            platform_codes: ShortcutPlatformCodes {
                macos_key_codes: None,
                windows_virtual_keys: Some(vec![162, 164, 32]),
                linux_key_codes: None,
            },
        };

        assert!(normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Windows
        ));

        assert_eq!(
            settings.hands_free_shortcut.display_name,
            "Left Ctrl + Space"
        );
        assert_eq!(
            settings.hands_free_shortcut.windows_virtual_keys(),
            &[162, 32]
        );
    }

    #[test]
    fn preserves_custom_windows_hands_free_shortcut_with_generic_modifiers() {
        let mut settings = default_settings_for_platform(SettingsPlatform::Windows);
        settings.hands_free_shortcut.display_name = "Custom Ctrl + Alt + Space".into();
        settings
            .hands_free_shortcut
            .platform_codes
            .windows_virtual_keys = Some(vec![17, 18, 32]);

        assert!(!normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Windows
        ));
        assert_eq!(
            settings.hands_free_shortcut.display_name,
            "Custom Ctrl + Alt + Space"
        );
        assert_eq!(
            settings.hands_free_shortcut.windows_virtual_keys(),
            &[17, 18, 32]
        );
        assert!(settings.hands_free_shortcut.macos_key_codes().is_empty());
        assert!(matches!(
            settings.hands_free_shortcut.mode,
            ShortcutMode::Toggle
        ));
    }

    #[test]
    fn default_settings_for_platform_preserves_macos_and_sets_windows_defaults() {
        let macos = default_settings_for_platform(SettingsPlatform::Macos);
        assert_eq!(macos.push_to_talk_shortcut.display_name, "Fn");
        assert_eq!(macos.push_to_talk_shortcut.macos_key_codes(), &[63]);
        assert_eq!(macos.hands_free_shortcut.display_name, "Control + Space");
        assert_eq!(macos.hands_free_shortcut.macos_key_codes(), &[59, 49]);

        let windows = default_settings_for_platform(SettingsPlatform::Windows);
        assert_eq!(windows.push_to_talk_shortcut.display_name, "Right Ctrl");
        assert_eq!(windows.push_to_talk_shortcut.windows_virtual_keys(), &[163]);
        assert_eq!(
            windows.hands_free_shortcut.display_name,
            "Left Ctrl + Space"
        );
        assert_eq!(
            windows.hands_free_shortcut.windows_virtual_keys(),
            &[162, 32]
        );

        let linux = default_settings_for_platform(SettingsPlatform::Linux);
        assert_eq!(linux.push_to_talk_shortcut.display_name, "F9");
        assert_eq!(linux.push_to_talk_shortcut.linux_key_codes(), &[0xffc6]);
        assert_eq!(linux.hands_free_shortcut.display_name, "Ctrl + Space");
        assert_eq!(linux.hands_free_shortcut.linux_key_codes(), &[0xffe3, 0x20]);
    }

    #[test]
    fn normalizes_old_macos_shortcuts_to_linux_defaults() {
        let mut settings = AppSettings {
            push_to_talk_shortcut: serde_json::from_value(serde_json::json!({
                "displayName": "Fn",
                "macosKeyCodes": [63],
                "mode": "hold"
            }))
            .unwrap(),
            hands_free_shortcut: serde_json::from_value(serde_json::json!({
                "displayName": "Control + Space",
                "macosKeyCodes": [59, 49],
                "mode": "toggle"
            }))
            .unwrap(),
            ..AppSettings::default()
        };

        assert!(normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Linux
        ));

        assert_eq!(settings.push_to_talk_shortcut.display_name, "F9");
        assert_eq!(settings.push_to_talk_shortcut.linux_key_codes(), &[0xffc6]);
        assert!(settings.push_to_talk_shortcut.macos_key_codes().is_empty());
        assert_eq!(settings.hands_free_shortcut.display_name, "Ctrl + Space");
        assert_eq!(
            settings.hands_free_shortcut.linux_key_codes(),
            &[0xffe3, 0x20]
        );
        assert!(settings.hands_free_shortcut.macos_key_codes().is_empty());
    }

    #[test]
    fn derives_safe_windows_virtual_keys_from_chord() {
        let mut settings = AppSettings {
            hands_free_shortcut: ShortcutSettings {
                display_name: "Ctrl + Alt + K".into(),
                mode: ShortcutMode::Toggle,
                enabled: true,
                double_tap_toggle: false,
                chord: Some(ShortcutChord {
                    modifiers: vec![ShortcutModifier::Control, ShortcutModifier::Alt],
                    key: Some(ShortcutKey::Character("k".into())),
                }),
                platform_codes: Default::default(),
            },
            ..AppSettings::default()
        };

        assert!(normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Windows
        ));

        assert_eq!(
            settings.hands_free_shortcut.windows_virtual_keys(),
            &[17, 18, 75]
        );
        assert!(settings.hands_free_shortcut.macos_key_codes().is_empty());
    }

    #[test]
    fn fn_chord_without_windows_codes_migrates_to_windows_default() {
        let mut settings = AppSettings {
            push_to_talk_shortcut: ShortcutSettings {
                display_name: "Fn".into(),
                mode: ShortcutMode::Hold,
                enabled: true,
                double_tap_toggle: false,
                chord: Some(ShortcutChord {
                    modifiers: vec![ShortcutModifier::Fn],
                    key: None,
                }),
                platform_codes: Default::default(),
            },
            ..AppSettings::default()
        };

        assert!(normalize_shortcuts_for_platform(
            &mut settings,
            SettingsPlatform::Windows
        ));

        assert_eq!(settings.push_to_talk_shortcut.display_name, "Right Ctrl");
        assert_eq!(
            settings.push_to_talk_shortcut.windows_virtual_keys(),
            &[163]
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
