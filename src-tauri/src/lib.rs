mod commands;
mod core_bridge;
mod history;
mod settings;
mod tray;

use commands::*;
use core_bridge::CoreBridge;
use settings::SettingsStore;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::Mutex;

pub struct AppState {
    pub settings: Mutex<SettingsStore>,
    pub history: Mutex<history::HistoryStore>,
    pub core: CoreBridge,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == "--background") {
                show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                app.set_dock_visibility(false);
            }

            let launch_in_background = std::env::args().any(|arg| arg == "--background");
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
            if let Err(error) = tauri::async_runtime::block_on(initialize_core(
                &core,
                initial_settings,
            )) {
                eprintln!("failed to initialize native core: {error:?}");
                return Err(error.into());
            }

            let core_for_boot = core.clone();
            let app_for_boot = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let permissions = core_for_boot
                    .request("permissionStatuses", serde_json::json!({}))
                    .await
                    .ok();

                let all_ready = permissions
                    .as_ref()
                    .map(permission_value_all_granted)
                    .unwrap_or(false);

                if all_ready {
                    let result = core_for_boot
                        .request("startHotkeyMonitor", serde_json::json!({}))
                        .await;

                    match result {
                        Ok(value) => {
                            let status = value.get("status").and_then(|value| value.as_str());
                            if status != Some("hotkey-monitoring") {
                                let message = "Shortcut monitor did not start. Check Accessibility permission. Some Macs may also require Input Monitoring.";
                                eprintln!("{message}");
                                let _ = app_for_boot.emit(
                                    "parrot:hotkey-monitor-failed",
                                    serde_json::json!({ "error": message }),
                                );
                            }
                        }
                        Err(error) => {
                            let message = format!("Shortcut monitor did not start: {error}");
                            eprintln!("{message}");
                            let _ = app_for_boot.emit(
                                "parrot:hotkey-monitor-failed",
                                serde_json::json!({ "error": message }),
                            );
                        }
                    }
                }

                // Keep startup lightweight: model warmup can be requested explicitly after setup.
            });

            app.manage(AppState {
                settings: Mutex::new(settings),
                history: Mutex::new(history),
                core,
            });

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
            delete_model,
            warm_models,
            start_test_dictation,
            stop_test_dictation,
            set_hotkey_monitor_enabled,
            capture_shortcut,
            permission_statuses,
            request_permission,
            save_recording_result,
            delete_history_item,
            clear_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running Parrot");
}
