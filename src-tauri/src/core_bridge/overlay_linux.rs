use crate::hyprland::{
    hyprctl_command, RECORDING_OVERLAY_BOTTOM_MARGIN, RECORDING_OVERLAY_HEIGHT,
    RECORDING_OVERLAY_TITLE, RECORDING_OVERLAY_WIDTH,
};
use gtk::prelude::{ContainerExt, GtkWindowExt, WidgetExt};
use serde_json::Value;
use std::process::Stdio;
use tauri::window::Color;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use std::time::Duration;

pub fn show_recording_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window("recording") else {
        eprintln!("recording overlay window not found");
        return;
    };

    constrain_linux_recording_overlay(&window);

    let overlay_size = PhysicalSize::new(RECORDING_OVERLAY_WIDTH, RECORDING_OVERLAY_HEIGHT);
    if let Err(error) = window.set_min_size(Some(overlay_size)) {
        eprintln!("failed to constrain recording overlay minimum size: {error}");
    }
    if let Err(error) = window.set_max_size(Some(overlay_size)) {
        eprintln!("failed to constrain recording overlay maximum size: {error}");
    }
    if let Err(error) = window.set_size(overlay_size) {
        eprintln!("failed to size recording overlay: {error}");
    }
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        let x = monitor_pos.x + ((monitor_size.width as i32 - RECORDING_OVERLAY_WIDTH as i32) / 2);
        let y = monitor_pos.y + monitor_size.height as i32
            - RECORDING_OVERLAY_HEIGHT as i32
            - RECORDING_OVERLAY_BOTTOM_MARGIN;
        if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
            eprintln!("failed to position recording overlay: {error}");
        }
    }

    if let Err(error) = window.unminimize() {
        eprintln!("failed to unminimize recording overlay: {error}");
    }
    if let Err(error) = window.set_always_on_top(true) {
        eprintln!("failed to keep recording overlay above other windows: {error}");
    }
    if let Err(error) = window.show() {
        eprintln!("failed to show recording overlay: {error}");
    }
    constrain_linux_recording_overlay(&window);
    position_linux_recording_overlay_with_hyprland();
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(Duration::from_millis(80)).await;
        position_linux_recording_overlay_with_hyprland();
        tokio::time::sleep(Duration::from_millis(140)).await;
        position_linux_recording_overlay_with_hyprland();
    });
}

fn constrain_linux_recording_overlay(window: &tauri::WebviewWindow) {
    let width = RECORDING_OVERLAY_WIDTH as i32;
    let height = RECORDING_OVERLAY_HEIGHT as i32;

    if let Err(error) = window.with_webview(move |webview| {
        let webview = webview.inner();
        webview.set_size_request(width, height);
        webview.set_hexpand(false);
        webview.set_vexpand(false);
    }) {
        eprintln!("failed to size recording overlay webview: {error}");
    }

    if let Err(error) = window.set_background_color(Some(Color(0, 0, 0, 0))) {
        eprintln!("failed to make recording overlay background transparent: {error}");
    }

    if let Ok(vbox) = window.default_vbox() {
        vbox.set_size_request(width, height);
        vbox.set_hexpand(false);
        vbox.set_vexpand(false);
        for child in vbox.children() {
            child.set_size_request(width, height);
            child.set_hexpand(false);
            child.set_vexpand(false);
        }
    }

    if let Ok(gtk_window) = window.gtk_window() {
        gtk_window.set_default_size(width, height);
        gtk_window.set_size_request(width, height);
        gtk_window.set_resizable(false);
        gtk_window.set_decorated(false);
        gtk_window.set_accept_focus(false);
        gtk_window.set_focus_on_map(false);
        gtk_window.resize(width, height);
    }
}

fn position_linux_recording_overlay_with_hyprland() {
    let Some((x, y)) = hyprland_recording_overlay_position() else {
        return;
    };

    move_linux_recording_overlay_with_hyprland(x, y);
}

fn hyprland_recording_overlay_position() -> Option<(i32, i32)> {
    let output = hyprctl_command().args(["monitors", "-j"]).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let monitors: Value = serde_json::from_slice(&output.stdout).ok()?;
    let monitors = monitors.as_array()?;
    let monitor = monitors
        .iter()
        .find(|monitor| {
            monitor
                .get("focused")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| monitors.first())?;

    let scale = monitor
        .get("scale")
        .and_then(Value::as_f64)
        .filter(|scale| *scale > 0.0)
        .unwrap_or(1.0);
    let monitor_x = monitor.get("x").and_then(Value::as_f64).unwrap_or(0.0) / scale;
    let monitor_y = monitor.get("y").and_then(Value::as_f64).unwrap_or(0.0) / scale;
    let monitor_width = monitor.get("width").and_then(Value::as_f64)? / scale;
    let monitor_height = monitor.get("height").and_then(Value::as_f64)? / scale;
    let overlay_width = RECORDING_OVERLAY_WIDTH as f64;
    let overlay_height = RECORDING_OVERLAY_HEIGHT as f64;
    let bottom_margin = RECORDING_OVERLAY_BOTTOM_MARGIN as f64;

    Some((
        (monitor_x + ((monitor_width - overlay_width) / 2.0)).round() as i32,
        (monitor_y + monitor_height - overlay_height - bottom_margin).round() as i32,
    ))
}

fn move_linux_recording_overlay_with_hyprland(x: i32, y: i32) {
    let x = x.to_string();
    let y_and_selector = format!("{y},title:{RECORDING_OVERLAY_TITLE}");
    if let Err(error) = hyprctl_command()
        .args([
            "dispatch",
            "movewindowpixel",
            "exact",
            x.as_str(),
            y_and_selector.as_str(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        eprintln!("failed to position recording overlay with Hyprland: {error}");
    }
}
