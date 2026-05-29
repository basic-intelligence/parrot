mod commands;
mod core_bridge;
mod history;
#[cfg(any(target_os = "linux", test))]
mod hyprland;
#[cfg(any(target_os = "linux", test))]
mod linux_shortcuts;
mod settings;
mod tray;

use commands::*;
use core_bridge::CoreBridge;
use parrot_protocol::NativeCoreMethod;
use settings::SettingsStore;
use std::{sync::OnceLock, time::Duration};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::{Mutex, Notify};

pub struct AppRuntime {
    pub settings: Mutex<SettingsStore>,
    pub history: Mutex<history::HistoryStore>,
    pub core: CoreBridge,
}

pub struct AppState {
    runtime: OnceLock<AppRuntime>,
    ready: Notify,
}

impl AppState {
    fn new() -> Self {
        Self {
            runtime: OnceLock::new(),
            ready: Notify::new(),
        }
    }

    fn set_runtime(&self, runtime: AppRuntime) -> anyhow::Result<()> {
        self.runtime
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("app runtime was already initialized"))?;
        self.ready.notify_waiters();
        Ok(())
    }

    pub async fn runtime(&self) -> anyhow::Result<&AppRuntime> {
        if let Some(runtime) = self.runtime.get() {
            return Ok(runtime);
        }

        let runtime = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                self.ready.notified().await;
                if let Some(runtime) = self.runtime.get() {
                    return runtime;
                }
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("Parrot is still starting."))?;
        Ok(runtime)
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn sync_launch_at_login(app: &AppHandle, settings: &mut SettingsStore) -> anyhow::Result<()> {
    if let Ok(actual_launch_at_login) = app.autolaunch().is_enabled() {
        if settings.settings.launch_at_login != actual_launch_at_login {
            let mut next_settings = settings.settings.clone();
            next_settings.launch_at_login = actual_launch_at_login;
            settings.save(next_settings)?;
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalRecordAction {
    Start,
    Stop,
    Toggle,
    Cancel,
}

#[cfg(any(target_os = "linux", test))]
fn external_record_action(args: &[String]) -> Option<ExternalRecordAction> {
    let normalized = args.iter().map(String::as_str).collect::<Vec<_>>();

    for window in normalized.windows(2) {
        match window {
            ["record", "start"] | ["--record", "start"] => {
                return Some(ExternalRecordAction::Start);
            }
            ["record", "stop"] | ["--record", "stop"] => {
                return Some(ExternalRecordAction::Stop);
            }
            ["record", "toggle"] | ["--record", "toggle"] => {
                return Some(ExternalRecordAction::Toggle);
            }
            ["record", "cancel"] | ["--record", "cancel"] => {
                return Some(ExternalRecordAction::Cancel);
            }
            _ => {}
        }
    }

    None
}

#[cfg(any(target_os = "linux", test))]
fn native_method_for_record_action(action: ExternalRecordAction) -> NativeCoreMethod {
    match action {
        ExternalRecordAction::Start => NativeCoreMethod::StartHotkeyRecording,
        ExternalRecordAction::Stop => NativeCoreMethod::StopHotkeyRecording,
        ExternalRecordAction::Toggle => NativeCoreMethod::ToggleHotkeyRecording,
        ExternalRecordAction::Cancel => NativeCoreMethod::CancelHotkeyRecording,
    }
}

#[cfg(target_os = "linux")]
fn dispatch_external_record_action(app: AppHandle, action: ExternalRecordAction) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let runtime = match state.runtime().await {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Linux record command ignored; app is not ready: {error}");
                return;
            }
        };

        if let Err(error) = runtime
            .core
            .request(
                native_method_for_record_action(action),
                serde_json::json!({}),
            )
            .await
        {
            eprintln!("Linux record command failed: {error}");
            let _ = app.emit(
                "parrot:hotkey-monitor-failed",
                serde_json::json!({ "error": error.to_string() }),
            );
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            #[cfg(target_os = "linux")]
            if let Some(action) = external_record_action(&args) {
                dispatch_external_record_action(app.clone(), action);
                return;
            }

            if !args.iter().any(|arg| arg == "--background") {
                show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .manage(AppState::new())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                app.set_dock_visibility(false);
            }

            let args = std::env::args().collect::<Vec<_>>();
            #[cfg(target_os = "linux")]
            let pending_record_action = external_record_action(&args);
            let launch_in_background = args.iter().any(|arg| arg == "--background")
                || {
                    #[cfg(target_os = "linux")]
                    {
                        pending_record_action.is_some()
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        false
                    }
                };
            tray::install(app.handle())?;

            let app_handle = app.handle().clone();
            let mut settings = SettingsStore::load(&app_handle)?;
            sync_launch_at_login(&app_handle, &mut settings)?;
            let should_show_main_window =
                !launch_in_background || !settings.settings.onboarding_completed;
            let history = history::HistoryStore::load(&app_handle)?;
            let core = match tauri::async_runtime::block_on(CoreBridge::spawn(app_handle.clone())) {
                Ok(core) => core,
                Err(error) => {
                    eprintln!("failed to spawn native core: {error:?}");
                    return Err(error.into());
                }
            };

            let initial_settings = settings.settings.clone();
            #[cfg(target_os = "windows")]
            let warm_models_on_boot = initial_settings.onboarding_completed;
            if let Err(error) = tauri::async_runtime::block_on(initialize_core(
                &app_handle,
                &core,
                initial_settings,
            )) {
                eprintln!("failed to initialize native core: {error:?}");
                return Err(error.into());
            }

            app.state::<AppState>().set_runtime(AppRuntime {
                settings: Mutex::new(settings),
                history: Mutex::new(history),
                core: core.clone(),
            })?;

            #[cfg(target_os = "linux")]
            if let Some(action) = pending_record_action {
                dispatch_external_record_action(app_handle.clone(), action);
            }

            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            {
                let core_for_boot = core.clone();
                let app_for_boot = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let permissions = core_for_boot
                        .request(NativeCoreMethod::PermissionStatuses, serde_json::json!({}))
                        .await
                        .ok();

                    let all_ready = permissions
                        .as_ref()
                        .map(permission_value_hotkey_monitor_ready)
                        .unwrap_or(false);

                    if all_ready {
                        let result = core_for_boot
                            .request(NativeCoreMethod::StartHotkeyMonitor, serde_json::json!({}))
                            .await;

                        match result {
                            Ok(value) => {
                                let status = value.get("status").and_then(|value| value.as_str());
                                if status != Some("hotkey-monitoring") {
                                    let message = if cfg!(target_os = "linux") {
                                        "Shortcut monitor did not start on Linux. Use compositor shortcuts, the XDG GlobalShortcuts portal, or the evdev fallback.".to_string()
                                    } else {
                                        "Shortcut monitor did not start. Check Accessibility permission. Some Macs may also require Input Monitoring.".to_string()
                                    };
                                    eprintln!("{message}");
                                    let _ = app_for_boot.emit(
                                        "parrot:hotkey-monitor-failed",
                                        serde_json::json!({ "error": message }),
                                    );
                                }
                            }
                            Err(error) => {
                                let message = if cfg!(target_os = "linux") {
                                    format!(
                                        "Shortcut monitor did not start on Linux: {error}. Use compositor shortcuts, the XDG GlobalShortcuts portal, or the evdev fallback."
                                    )
                                } else {
                                    format!("Shortcut monitor did not start: {error}")
                                };
                                eprintln!("{message}");
                                let _ = app_for_boot.emit(
                                    "parrot:hotkey-monitor-failed",
                                    serde_json::json!({ "error": message }),
                                );
                            }
                        }

                        #[cfg(target_os = "windows")]
                        if warm_models_on_boot {
                            if let Err(error) = core_for_boot
                                .request(NativeCoreMethod::WarmModels, serde_json::json!({}))
                                .await
                            {
                                eprintln!("Windows model warmup failed: {error}");
                            }
                        }
                    }

                    // Fresh setup warms models explicitly before completing onboarding.
                });
            }

            if should_show_main_window {
                show_main_window(&app_handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            save_settings,
            set_launch_at_login,
            set_update_badge,
            download_model,
            model_statuses,
            delete_model,
            warm_models,
            start_test_dictation,
            stop_test_dictation,
            set_hotkey_monitor_enabled,
            capture_shortcut,
            install_linux_shortcuts,
            permission_statuses,
            request_permission,
            save_recording_result,
            delete_history_item,
            clear_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running Parrot");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn external_record_action_parses_subcommands() {
        assert_eq!(
            external_record_action(&args(&["parrot", "record", "start"])),
            Some(ExternalRecordAction::Start)
        );
        assert_eq!(
            external_record_action(&args(&["parrot", "record", "stop"])),
            Some(ExternalRecordAction::Stop)
        );
        assert_eq!(
            external_record_action(&args(&["parrot", "record", "toggle"])),
            Some(ExternalRecordAction::Toggle)
        );
        assert_eq!(
            external_record_action(&args(&["parrot", "record", "cancel"])),
            Some(ExternalRecordAction::Cancel)
        );
    }

    #[test]
    fn external_record_action_parses_record_flag() {
        assert_eq!(
            external_record_action(&args(&["parrot", "--record", "start"])),
            Some(ExternalRecordAction::Start)
        );
        assert_eq!(
            external_record_action(&args(&["parrot", "--record", "stop"])),
            Some(ExternalRecordAction::Stop)
        );
    }

    #[test]
    fn record_actions_map_to_native_methods() {
        assert_eq!(
            native_method_for_record_action(ExternalRecordAction::Toggle),
            NativeCoreMethod::ToggleHotkeyRecording
        );
    }
}
