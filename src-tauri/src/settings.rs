use anyhow::Context;
use parrot_settings::normalize_settings;
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
            (AppSettings::default(), false)
        };

        migrated |= normalize_settings(&mut settings);

        if migrated {
            fs::write(&path, serde_json::to_vec_pretty(&settings)?)?;
        }
        Ok(Self { settings, path })
    }

    pub fn save(&mut self, mut settings: AppSettings) -> anyhow::Result<()> {
        normalize_settings(&mut settings);
        self.settings = settings;
        fs::write(&self.path, serde_json::to_vec_pretty(&self.settings)?)?;
        Ok(())
    }
}
