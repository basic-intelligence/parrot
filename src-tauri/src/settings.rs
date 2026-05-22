use anyhow::Context;
use parrot_settings::{
    default_settings_for_platform, normalize_settings_for_platform, SettingsPlatform,
};
pub use parrot_settings::{AppSettings, ShortcutSettings};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

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
            let missing_input_monitoring_onboarding = value
                .get("inputMonitoringPermissionShownInOnboarding")
                .is_none();
            (
                serde_json::from_value(value)?,
                missing_cleanup_model
                    || missing_paste_target_setting
                    || missing_input_monitoring_onboarding,
            )
        } else {
            (
                default_settings_for_platform(current_settings_platform()),
                false,
            )
        };

        migrated |= normalize_settings_for_platform(&mut settings, current_settings_platform());

        if migrated {
            fs::write(&path, serde_json::to_vec_pretty(&settings)?)?;
        }
        Ok(Self { settings, path })
    }

    pub fn save(&mut self, mut settings: AppSettings) -> anyhow::Result<()> {
        normalize_settings_for_platform(&mut settings, current_settings_platform());
        self.settings = settings;
        fs::write(&self.path, serde_json::to_vec_pretty(&self.settings)?)?;
        Ok(())
    }
}

fn current_settings_platform() -> SettingsPlatform {
    if cfg!(target_os = "windows") {
        SettingsPlatform::Windows
    } else {
        SettingsPlatform::Macos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn current_platform_uses_windows_shortcut_defaults() {
        let settings = default_settings_for_platform(current_settings_platform());

        assert_eq!(settings.push_to_talk_shortcut.display_name, "Right Ctrl");
        assert_eq!(
            settings.push_to_talk_shortcut.windows_virtual_keys(),
            &[163]
        );
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
    #[cfg(not(target_os = "windows"))]
    fn current_platform_preserves_macos_shortcut_defaults() {
        let settings = default_settings_for_platform(current_settings_platform());

        assert_eq!(settings.push_to_talk_shortcut.display_name, "Fn");
        assert_eq!(settings.push_to_talk_shortcut.macos_key_codes(), &[63]);
        assert_eq!(settings.hands_free_shortcut.display_name, "Control + Space");
        assert_eq!(settings.hands_free_shortcut.macos_key_codes(), &[59, 49]);
    }
}
