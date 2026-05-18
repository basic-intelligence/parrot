use crate::{
    core_bridge::{is_native_core_disconnect, CoreBridge},
    history::HistoryEntry,
    settings::{AppSettings, ShortcutSettings},
    AppState,
};
use anyhow::Context;
use chrono::Utc;
use parrot_protocol::{
    AudioDevice, ModelStatus, NativeCoreMethod, NativeCorePaths, PermissionKind, PermissionSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use uuid::Uuid;

const CLEANUP_DEFAULT_INSTRUCTIONS: &str =
    include_str!("../../native-core/shared/prompts/cleanup-default-instructions.md");
const CLEANUP_SYSTEM_CONTRACT: &str =
    include_str!("../../native-core/shared/prompts/cleanup-system-contract.md");
const CLEANUP_USER_TEMPLATE: &str =
    include_str!("../../native-core/shared/prompts/cleanup-user-template.md");
const CLEANUP_QWEN3_CHATML: &str =
    include_str!("../../native-core/shared/prompts/formats/qwen3-chatml.txt");
const CLEANUP_GEMMA4_TURNS: &str =
    include_str!("../../native-core/shared/prompts/formats/gemma4-turns.txt");
const LANGUAGE_CATALOG_JSON: &str = include_str!("../../native-core/shared/languages.json");

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

pub async fn initialize_core(
    app: &AppHandle,
    core: &CoreBridge,
    settings: AppSettings,
) -> anyhow::Result<()> {
    let language_catalog: serde_json::Value = serde_json::from_str(LANGUAGE_CATALOG_JSON)
        .context("shared language catalog must be valid JSON")?;
    let paths = native_core_paths(app)?;

    core.request(
        NativeCoreMethod::Initialize,
        json!({
            "settings": settings,
            "paths": paths,
            "languageCatalog": language_catalog,
            "debugCleanupFailures": cfg!(debug_assertions),
            "prompts": {
                "cleanupDefaultInstructions": CLEANUP_DEFAULT_INSTRUCTIONS,
                "cleanupSystemContract": CLEANUP_SYSTEM_CONTRACT,
                "cleanupUserTemplate": CLEANUP_USER_TEMPLATE,
                "formats": {
                    "qwen3Chatml": CLEANUP_QWEN3_CHATML,
                    "gemma4Turns": CLEANUP_GEMMA4_TURNS
                },
                "cleanupTranscript": CLEANUP_DEFAULT_INSTRUCTIONS
            }
        }),
    )
    .await?;

    Ok(())
}

fn native_core_paths(app: &AppHandle) -> anyhow::Result<NativeCorePaths> {
    let app_data_dir = app.path().app_data_dir().context("missing app data dir")?;
    let model_cache_dir = model_cache_dir(&app_data_dir);
    let resources_dir = app
        .path()
        .resource_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let shared_resources_dir = shared_resources_dir(&resources_dir);
    let temp_dir = std::env::temp_dir();

    Ok(NativeCorePaths {
        app_data_dir: path_to_string(app_data_dir.clone())?,
        models_dir: path_to_string(model_cache_dir.clone())?,
        speech_models_dir: path_to_string(model_cache_dir.join("whisper-models"))?,
        cleanup_models_dir: path_to_string(model_cache_dir.join("cleanup-models"))?,
        resources_dir: path_to_string(resources_dir)?,
        shared_resources_dir: path_to_string(shared_resources_dir)?,
        temp_dir: path_to_string(temp_dir)?,
    })
}

#[cfg(target_os = "macos")]
fn model_cache_dir(app_data_dir: &std::path::Path) -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("Parrot")
        })
        .unwrap_or_else(|| app_data_dir.to_path_buf())
}

#[cfg(not(target_os = "macos"))]
fn model_cache_dir(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.to_path_buf()
}

fn shared_resources_dir(resources_dir: &std::path::Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let candidates = [
        // If resources are explicitly mapped to native-core/shared.
        resources_dir.join("native-core").join("shared"),
        // Tauri may preserve parent-relative resource paths under _up_.
        resources_dir
            .join("_up_")
            .join("native-core")
            .join("shared"),
        // Tauri copies configured resource files/dirs directly into
        // Contents/Resources by default, e.g. Resources/models.json.
        resources_dir.to_path_buf(),
        // If a whole shared directory is bundled as Resources/shared.
        resources_dir.join("shared"),
        // Dev layouts.
        cwd.join("native-core").join("shared"),
        cwd.join("..").join("native-core").join("shared"),
    ];

    candidates
        .into_iter()
        .find(|candidate| has_required_shared_resources(candidate))
        .unwrap_or_else(|| resources_dir.join("native-core").join("shared"))
}

fn has_required_shared_resources(candidate: &Path) -> bool {
    candidate.join("languages.json").exists()
        && candidate.join("models.json").exists()
        && candidate
            .join("prompts")
            .join("cleanup-system-contract.md")
            .exists()
}

fn path_to_string(path: PathBuf) -> anyhow::Result<String> {
    path.to_str()
        .context("native core path is not valid UTF-8")
        .map(str::to_owned)
}

pub fn permission_value_all_granted(value: &serde_json::Value) -> bool {
    if let Some(requirements) = value.get("requirements").and_then(|value| value.as_array()) {
        if !requirements.is_empty() {
            return requirements.iter().all(|requirement| {
                requirement
                    .get("required")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
                    == false
                    || requirement.get("state").and_then(|value| value.as_str()) == Some("granted")
            });
        }
    }

    if let Some(all_required_granted) = value
        .get("allRequiredGranted")
        .and_then(|value| value.as_bool())
    {
        return all_required_granted;
    }

    let microphone = value.get("microphone").and_then(|value| value.as_str());
    let accessibility = value.get("accessibility").and_then(|value| value.as_str());

    microphone == Some("granted") && accessibility == Some("granted")
}

async fn initialize_core_from_state(state: &State<'_, AppState>) -> anyhow::Result<()> {
    let settings = state.settings.lock().await.settings.clone();
    initialize_core(state.inner().core.app(), &state.core, settings).await
}

async fn restart_hotkey_monitor_if_ready(state: &State<'_, AppState>) -> anyhow::Result<()> {
    let permissions = state
        .core
        .request(NativeCoreMethod::PermissionStatuses, json!({}))
        .await?;
    if permission_value_all_granted(&permissions) {
        let value = state
            .core
            .request(NativeCoreMethod::StartHotkeyMonitor, json!({}))
            .await?;
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
    method: NativeCoreMethod,
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
        NativeCoreMethod::UpdateSettings,
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
    let _ = core_request_recovering(
        &state,
        NativeCoreMethod::UpdateSettings,
        json!({ "settings": settings }),
    )
    .await;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_update_badge(
    app: AppHandle,
    available: bool,
    version: Option<String>,
) -> Result<(), String> {
    crate::tray::set_update_badge(&app, available, version.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<AppSnapshot, String> {
    core_request_recovering(
        &state,
        NativeCoreMethod::DownloadModel,
        json!({ "kind": kind }),
    )
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
    core_request_recovering(
        &state,
        NativeCoreMethod::DeleteModel,
        json!({ "kind": kind }),
    )
    .await
    .map_err(|e| e.to_string())?;
    snapshot(&app, &state).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn warm_models(state: State<'_, AppState>) -> Result<(), String> {
    core_request_recovering(&state, NativeCoreMethod::WarmModels, json!({}))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_test_dictation(state: State<'_, AppState>) -> Result<(), String> {
    core_request_recovering(
        &state,
        NativeCoreMethod::StartRecording,
        json!({ "kind": "test" }),
    )
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_test_dictation(state: State<'_, AppState>) -> Result<DictationResult, String> {
    let value = state
        .core
        .request(NativeCoreMethod::StopRecording, json!({ "kind": "test" }))
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
        NativeCoreMethod::StartHotkeyMonitor
    } else {
        NativeCoreMethod::StopHotkeyMonitor
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
    let value = core_request_recovering(
        &state,
        NativeCoreMethod::CaptureShortcut,
        json!({ "target": target }),
    )
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
    let permission_kind: PermissionKind =
        serde_json::from_value(serde_json::Value::String(kind.clone()))
            .map_err(|_| "Unknown permission kind.".to_string())?;
    if !matches!(
        permission_kind,
        PermissionKind::Microphone
            | PermissionKind::Accessibility
            | PermissionKind::InputMonitoring
    ) {
        return Err("Unknown permission kind.".to_string());
    }

    let open_settings = open_settings.unwrap_or(false);

    core_request_recovering(
        &state,
        NativeCoreMethod::RequestPermission,
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
    let devices_value =
        core_request_recovering(state, NativeCoreMethod::ListAudioDevices, json!({}))
            .await
            .unwrap_or_else(|_| json!([]));
    let devices: Vec<AudioDevice> = serde_json::from_value(devices_value).unwrap_or_default();
    let models_value =
        match core_request_recovering(state, NativeCoreMethod::ModelStatuses, json!({})).await {
            Ok(value) => value,
            Err(error) => {
                eprintln!("failed to read native-core model statuses: {error:?}");
                json!([])
            }
        };
    let models: Vec<ModelStatus> = serde_json::from_value(models_value).unwrap_or_default();
    let permissions = permission_snapshot(state).await?;
    let history = state.history.lock().await.entries();
    Ok(AppSnapshot {
        settings,
        devices,
        models,
        history,
        permissions,
        default_cleanup_prompt: CLEANUP_DEFAULT_INSTRUCTIONS.to_string(),
    })
}

async fn permission_snapshot(state: &State<'_, AppState>) -> anyhow::Result<PermissionSnapshot> {
    let permissions_value =
        core_request_recovering(state, NativeCoreMethod::PermissionStatuses, json!({})).await?;
    let mut permissions: PermissionSnapshot =
        serde_json::from_value(permissions_value).unwrap_or_default();
    permissions.ensure_macos_compat_requirements();
    Ok(permissions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

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
        permissions.ensure_macos_compat_requirements();
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
        permissions.ensure_macos_compat_requirements();
        let output = serde_json::to_value(permissions).unwrap();

        assert!(permission_value_all_granted(&value));
        assert_eq!(output["inputMonitoring"], "denied");
        assert_eq!(output["allGranted"], true);
    }

    #[test]
    fn native_core_paths_payload_uses_camel_case_keys() {
        let paths = NativeCorePaths {
            app_data_dir: "/tmp/parrot".into(),
            models_dir: "/tmp/parrot".into(),
            speech_models_dir: "/tmp/parrot/whisper-models".into(),
            cleanup_models_dir: "/tmp/parrot/cleanup-models".into(),
            resources_dir: "/tmp/resources".into(),
            shared_resources_dir: "/tmp/resources/native-core/shared".into(),
            temp_dir: "/tmp".into(),
        };

        let output = serde_json::to_value(paths).unwrap();

        assert_eq!(output["appDataDir"], "/tmp/parrot");
        assert_eq!(
            output["sharedResourcesDir"],
            "/tmp/resources/native-core/shared"
        );
    }

    #[test]
    fn model_cache_dir_preserves_platform_cache_root() {
        let app_data_dir = std::path::Path::new("/tmp/parrot-app-data");
        let model_cache_dir = model_cache_dir(app_data_dir);

        #[cfg(target_os = "macos")]
        assert!(model_cache_dir.ends_with("Library/Application Support/Parrot"));

        #[cfg(not(target_os = "macos"))]
        assert_eq!(model_cache_dir, app_data_dir);
    }

    #[test]
    fn shared_resources_dir_accepts_resource_root_layout() {
        let temp_dir = temp_test_dir("resource-root");
        let resources_dir = temp_dir.join("Resources");
        write_shared_resource_markers(&resources_dir);

        let output = shared_resources_dir(&resources_dir);

        assert_eq!(output, resources_dir);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn shared_resources_dir_prefers_native_core_shared_layout() {
        let temp_dir = temp_test_dir("native-core-shared");
        let resources_dir = temp_dir.join("Resources");
        let native_shared_dir = resources_dir.join("native-core").join("shared");
        write_shared_resource_markers(&resources_dir);
        write_shared_resource_markers(&native_shared_dir);

        let output = shared_resources_dir(&resources_dir);

        assert_eq!(output, native_shared_dir);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn shared_resources_dir_accepts_tauri_parent_relative_layout() {
        let temp_dir = temp_test_dir("tauri-parent-relative");
        let resources_dir = temp_dir.join("Resources");
        let shared_dir = resources_dir
            .join("_up_")
            .join("native-core")
            .join("shared");
        write_shared_resource_markers(&shared_dir);

        let output = shared_resources_dir(&resources_dir);

        assert_eq!(output, shared_dir);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn shared_resources_dir_accepts_shared_directory_layout() {
        let temp_dir = temp_test_dir("shared-directory");
        let resources_dir = temp_dir.join("Resources");
        let shared_dir = resources_dir.join("shared");
        write_shared_resource_markers(&shared_dir);

        let output = shared_resources_dir(&resources_dir);

        assert_eq!(output, shared_dir);
        let _ = fs::remove_dir_all(temp_dir);
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("parrot-{name}-{}", Uuid::new_v4()))
    }

    fn write_shared_resource_markers(directory: &Path) {
        fs::create_dir_all(directory.join("prompts")).unwrap();
        fs::write(directory.join("languages.json"), "{}").unwrap();
        fs::write(directory.join("models.json"), "{}").unwrap();
        fs::write(
            directory.join("prompts").join("cleanup-system-contract.md"),
            "",
        )
        .unwrap();
    }
}
