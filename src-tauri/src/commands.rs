use crate::{
    core_bridge::{is_native_core_disconnect, CoreBridge},
    history::HistoryEntry,
    settings::{AppSettings, ShortcutSettings},
    AppState,
};
use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use uuid::Uuid;

const CLEANUP_TRANSCRIPT_PROMPT: &str =
    include_str!("../../native-core/shared/prompts/cleanup-transcript.md");
const LANGUAGE_CATALOG_JSON: &str = include_str!("../../native-core/shared/languages.json");

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub uid: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub role: String,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSnapshot {
    pub microphone: String,
    pub accessibility: String,
    #[serde(default)]
    pub input_monitoring: String,
    #[serde(default)]
    pub all_granted: bool,
}

impl Default for PermissionSnapshot {
    fn default() -> Self {
        Self {
            microphone: "unknown".to_string(),
            accessibility: "unknown".to_string(),
            input_monitoring: "unknown".to_string(),
            all_granted: false,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    settings: AppSettings,
    devices: Vec<AudioDevice>,
    models: Vec<ModelStatus>,
    history: Vec<HistoryEntry>,
    permissions: PermissionSnapshot,
    default_cleanup_prompt: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationResult {
    raw: String,
    cleaned: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResultPayload {
    raw: String,
    cleaned: String,
    audio_duration_seconds: f64,
}

pub async fn initialize_core(core: &CoreBridge, settings: AppSettings) -> anyhow::Result<()> {
    let language_catalog: serde_json::Value = serde_json::from_str(LANGUAGE_CATALOG_JSON)
        .context("shared language catalog must be valid JSON")?;

    core.request(
        "initialize",
        json!({
            "settings": settings,
            "languageCatalog": language_catalog,
            "debugCleanupFailures": cfg!(debug_assertions),
            "prompts": {
                "cleanupTranscript": CLEANUP_TRANSCRIPT_PROMPT
            }
        }),
    )
    .await?;

    Ok(())
}

pub fn permission_value_all_granted(value: &serde_json::Value) -> bool {
    let microphone = value.get("microphone").and_then(|value| value.as_str());
    let accessibility = value.get("accessibility").and_then(|value| value.as_str());

    microphone == Some("granted") && accessibility == Some("granted")
}

async fn initialize_core_from_state(state: &State<'_, AppState>) -> anyhow::Result<()> {
    let settings = state.settings.lock().await.settings.clone();
    initialize_core(&state.core, settings).await
}

async fn restart_hotkey_monitor_if_ready(state: &State<'_, AppState>) -> anyhow::Result<()> {
    let permissions = state.core.request("permissionStatuses", json!({})).await?;
    if permission_value_all_granted(&permissions) {
        let value = state.core.request("startHotkeyMonitor", json!({})).await?;
        let status = value.get("status").and_then(|value| value.as_str());
        if status != Some("hotkey-monitoring") {
            return Err(anyhow::anyhow!(
                "Shortcut monitor did not start. Check Accessibility permission. Some Macs may also require Input Monitoring."
            ));
        }
    }
    Ok(())
}

async fn core_request_recovering(
    state: &State<'_, AppState>,
    method: &str,
    payload: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    match state.core.request(method, payload.clone()).await {
        Ok(value) => Ok(value),
        Err(error) if is_native_core_disconnect(&error) => {
            state.core.reconnect().await?;
            initialize_core_from_state(state).await?;
            if let Err(error) = restart_hotkey_monitor_if_ready(state).await {
                eprintln!("failed to restart hotkey monitor after native-core reconnect: {error}");
            }
            state.core.request(method, payload).await
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn get_app_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> Result<AppSnapshot, String> {
    let saved_settings = {
        let mut store = state.settings.lock().await;
        settings.launch_at_login = app
            .autolaunch()
            .is_enabled()
            .unwrap_or(store.settings.launch_at_login);
        store.save(settings.clone()).map_err(|e| e.to_string())?;
        store.settings.clone()
    };
    core_request_recovering(
        &state,
        "updateSettings",
        json!({ "settings": saved_settings }),
    )
    .await
    .map_err(|e| e.to_string())?;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_launch_at_login(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    if enabled {
        app.autolaunch().enable().map_err(|e| e.to_string())?;
    } else {
        app.autolaunch().disable().map_err(|e| e.to_string())?;
    }
    let actual_enabled = app.autolaunch().is_enabled().map_err(|e| e.to_string())?;
    let settings = {
        let mut store = state.settings.lock().await;
        let mut settings = store.settings.clone();
        settings.launch_at_login = actual_enabled;
        store.save(settings.clone()).map_err(|e| e.to_string())?;
        settings
    };
    let _ =
        core_request_recovering(&state, "updateSettings", json!({ "settings": settings })).await;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<AppSnapshot, String> {
    core_request_recovering(&state, "downloadModel", json!({ "kind": kind }))
        .await
        .map_err(|e| e.to_string())?;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<AppSnapshot, String> {
    core_request_recovering(&state, "deleteModel", json!({ "kind": kind }))
        .await
        .map_err(|e| e.to_string())?;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn warm_models(state: State<'_, AppState>) -> Result<(), String> {
    core_request_recovering(&state, "warmModels", json!({}))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_test_dictation(state: State<'_, AppState>) -> Result<(), String> {
    core_request_recovering(&state, "startRecording", json!({ "kind": "test" }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_test_dictation(state: State<'_, AppState>) -> Result<DictationResult, String> {
    let value = state
        .core
        .request("stopRecording", json!({ "kind": "test" }))
        .await
        .map_err(|e| e.to_string())?;
    let raw = value
        .get("raw")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let cleaned = value
        .get("cleaned")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let duration = value
        .get("audioDurationSeconds")
        .and_then(|v| v.as_f64())
        .unwrap_or_default();

    let settings = state.settings.lock().await.settings.clone();
    if settings.history_enabled {
        insert_history(&state, raw.clone(), cleaned.clone(), duration).await?;
    }

    Ok(DictationResult { raw, cleaned })
}

#[tauri::command]
pub async fn set_hotkey_monitor_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let method = if enabled {
        "startHotkeyMonitor"
    } else {
        "stopHotkeyMonitor"
    };

    let value = core_request_recovering(&state, method, json!({}))
        .await
        .map_err(|e| e.to_string())?;

    if enabled {
        let status = value.get("status").and_then(|v| v.as_str());
        if status != Some("hotkey-monitoring") {
            return Err(
                "Shortcut monitor did not start. Check Accessibility permission. Some Macs may also require Input Monitoring."
                    .to_string(),
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn capture_shortcut(
    state: State<'_, AppState>,
    target: String,
) -> Result<ShortcutSettings, String> {
    let value = core_request_recovering(&state, "captureShortcut", json!({ "target": target }))
        .await
        .map_err(|e| e.to_string())?;

    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permission_statuses(state: State<'_, AppState>) -> Result<PermissionSnapshot, String> {
    permission_snapshot(&state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn request_permission(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    open_settings: Option<bool>,
) -> Result<PermissionSnapshot, String> {
    if kind != "microphone" && kind != "accessibility" && kind != "inputMonitoring" {
        return Err("Unknown permission kind.".to_string());
    }

    let open_settings = open_settings.unwrap_or(false);

    core_request_recovering(
        &state,
        "requestPermission",
        json!({
            "kind": kind.clone(),
            "openSettings": open_settings
        }),
    )
    .await
    .map_err(|e| e.to_string())?;
    let permissions = permission_snapshot(&state)
        .await
        .map_err(|e| e.to_string())?;

    if kind == "microphone" && !open_settings {
        refocus_main_window_after_permission(app);
    }

    Ok(permissions)
}

fn refocus_main_window_after_permission(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;

        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
}

#[tauri::command]
pub async fn save_recording_result(
    app: AppHandle,
    state: State<'_, AppState>,
    result: RecordingResultPayload,
) -> Result<AppSnapshot, String> {
    let settings = state.settings.lock().await.settings.clone();
    if settings.history_enabled {
        insert_history(
            &state,
            result.raw,
            result.cleaned,
            result.audio_duration_seconds,
        )
        .await?;
    }
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<AppSnapshot, String> {
    state
        .history
        .lock()
        .await
        .delete(id)
        .map_err(|e| e.to_string())?;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_history(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    state
        .history
        .lock()
        .await
        .clear()
        .map_err(|e| e.to_string())?;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

async fn insert_history(
    state: &State<'_, AppState>,
    raw: String,
    cleaned: String,
    duration: f64,
) -> Result<(), String> {
    let mut history = state.history.lock().await;
    history
        .insert(HistoryEntry {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            audio_duration_seconds: duration,
            raw_transcription: Some(raw),
            cleaned_transcription: Some(cleaned),
        })
        .map_err(|e| e.to_string())
}

async fn snapshot(app: &AppHandle, state: &State<'_, AppState>) -> anyhow::Result<AppSnapshot> {
    let mut settings = state.settings.lock().await.settings.clone();
    settings.launch_at_login = app
        .autolaunch()
        .is_enabled()
        .unwrap_or(settings.launch_at_login);
    let devices_value = core_request_recovering(state, "listAudioDevices", json!({}))
        .await
        .unwrap_or_else(|_| json!([]));
    let devices: Vec<AudioDevice> = serde_json::from_value(devices_value).unwrap_or_default();
    let models_value = core_request_recovering(state, "modelStatuses", json!({}))
        .await
        .unwrap_or_else(|_| json!([]));
    let models: Vec<ModelStatus> = serde_json::from_value(models_value).unwrap_or_default();
    let permissions = permission_snapshot(state).await?;
    let history = state.history.lock().await.entries();
    Ok(AppSnapshot {
        settings,
        devices,
        models,
        history,
        permissions,
        default_cleanup_prompt: CLEANUP_TRANSCRIPT_PROMPT.to_string(),
    })
}

async fn permission_snapshot(state: &State<'_, AppState>) -> anyhow::Result<PermissionSnapshot> {
    let permissions_value = core_request_recovering(state, "permissionStatuses", json!({})).await?;
    let mut permissions: PermissionSnapshot =
        serde_json::from_value(permissions_value).unwrap_or_default();
    permissions.all_granted =
        permissions.microphone == "granted" && permissions.accessibility == "granted";
    Ok(permissions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_status_preserves_catalog_fields() {
        let value = serde_json::json!({
            "id": "cleanup",
            "role": "cleanup",
            "displayName": "Qwen3 1.7B Q5_K_M",
            "subtitle": "Fast local cleanup model",
            "expectedBytes": 1,
            "localBytes": 0,
            "progressBytes": 0,
            "progressTotalBytes": 1,
            "downloaded": false,
            "downloading": false,
            "required": true,
            "error": null
        });

        let status: ModelStatus = serde_json::from_value(value).unwrap();
        let output = serde_json::to_value(status).unwrap();

        assert_eq!(output["role"], "cleanup");
    }

    #[test]
    fn permission_snapshot_includes_input_monitoring() {
        let value = serde_json::json!({
            "microphone": "granted",
            "accessibility": "granted",
            "inputMonitoring": "granted"
        });

        let mut permissions: PermissionSnapshot = serde_json::from_value(value.clone()).unwrap();
        permissions.all_granted =
            permissions.microphone == "granted" && permissions.accessibility == "granted";
        let output = serde_json::to_value(permissions).unwrap();

        assert!(permission_value_all_granted(&value));
        assert_eq!(output["inputMonitoring"], "granted");
        assert_eq!(output["allGranted"], true);
    }

    #[test]
    fn permission_readiness_does_not_require_input_monitoring() {
        let value = serde_json::json!({
            "microphone": "granted",
            "accessibility": "granted",
            "inputMonitoring": "denied"
        });

        let mut permissions: PermissionSnapshot = serde_json::from_value(value.clone()).unwrap();
        permissions.all_granted =
            permissions.microphone == "granted" && permissions.accessibility == "granted";
        let output = serde_json::to_value(permissions).unwrap();

        assert!(permission_value_all_granted(&value));
        assert_eq!(output["inputMonitoring"], "denied");
        assert_eq!(output["allGranted"], true);
    }
}
