use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

const TRAY_ID: &str = "main-tray";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-icon.png");
const TRAY_ICON_UPDATE: &[u8] = include_bytes!("../icons/tray-icon-update.png");

static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, false)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Parrot")
        .menu(&menu)
        .icon_as_template(true)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }

                if UPDATE_AVAILABLE.load(Ordering::Relaxed) {
                    let _ = app.emit(
                        "parrot:open-settings",
                        serde_json::json!({ "tab": "general" }),
                    );
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Ok(icon) = Image::from_bytes(TRAY_ICON) {
        builder = builder.icon(icon);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

fn build_menu(app: &AppHandle, update_available: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let settings_text = if update_available {
        "Settings…  • Update available"
    } else {
        "Settings…"
    };

    let settings = MenuItem::with_id(app, "settings", settings_text, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    Menu::with_items(app, &[&settings, &quit])
}

pub fn set_update_badge(
    app: &AppHandle,
    available: bool,
    version: Option<&str>,
) -> tauri::Result<()> {
    UPDATE_AVAILABLE.store(available, Ordering::Relaxed);

    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let icon_bytes = if available {
        TRAY_ICON_UPDATE
    } else {
        TRAY_ICON
    };
    let icon = Image::from_bytes(icon_bytes)?;

    tray.set_icon_with_as_template(Some(icon), !available)?;

    let tooltip = match (available, version) {
        (true, Some(version)) => format!("Parrot — update {version} available"),
        (true, None) => "Parrot — update available".to_string(),
        _ => "Parrot".to_string(),
    };
    tray.set_tooltip(Some(tooltip))?;

    let menu = build_menu(app, available)?;
    tray.set_menu(Some(menu))?;

    Ok(())
}
