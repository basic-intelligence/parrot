use crate::platform::{detect_session, wayland_paste_message, LinuxSession};
use anyhow::{anyhow, Context};
use serde::Serialize;
use std::{
    env,
    ffi::OsStr,
    fs,
    io::Write,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(800);
const XDO_TOOL_CANDIDATES: &[&str] =
    &["/usr/bin/xdotool", "/usr/local/bin/xdotool", "/bin/xdotool"];
const WL_COPY_CANDIDATES: &[&str] = &["/usr/bin/wl-copy", "/usr/local/bin/wl-copy"];
const WL_PASTE_CANDIDATES: &[&str] = &["/usr/bin/wl-paste", "/usr/local/bin/wl-paste"];
const WTYPE_CANDIDATES: &[&str] = &["/usr/bin/wtype", "/usr/local/bin/wtype"];
const HYPRCTL_CANDIDATES: &[&str] = &["/usr/bin/hyprctl", "/usr/local/bin/hyprctl"];
const DOTOOL_CANDIDATES: &[&str] = &["/usr/bin/dotool", "/usr/local/bin/dotool"];
const YDOTOOL_CANDIDATES: &[&str] = &["/usr/bin/ydotool", "/usr/local/bin/ydotool"];
const RECORDING_OVERLAY_TITLE: &str = "Parrot Recording";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteTarget {
    pub session: String,
    pub hyprland_window: Option<String>,
}

impl PasteTarget {
    const PLATFORM_ID_SEPARATOR: char = '\t';

    pub fn platform_id(&self) -> String {
        match &self.hyprland_window {
            Some(window) if !window.trim().is_empty() => {
                format!("{}{}{}", self.session, Self::PLATFORM_ID_SEPARATOR, window)
            }
            _ => self.session.clone(),
        }
    }

    pub fn from_platform_id(platform_id: impl Into<String>) -> Self {
        let platform_id = platform_id.into();
        if let Some((session, hyprland_window)) =
            platform_id.split_once(Self::PLATFORM_ID_SEPARATOR)
        {
            return Self {
                session: session.to_string(),
                hyprland_window: (!hyprland_window.trim().is_empty())
                    .then(|| hyprland_window.to_string()),
            };
        }

        Self {
            session: platform_id,
            hyprland_window: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardBackup {
    Text(String),
    NonTextOrEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClipboardWrite {
    Arboard(ClipboardBackup),
    WlCopy {
        wl_copy: String,
        backup: ClipboardBackup,
    },
}

impl ClipboardBackup {
    pub fn text_to_restore(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::NonTextOrEmpty => None,
        }
    }
}

impl ClipboardWrite {
    fn text_to_restore(&self) -> Option<&str> {
        match self {
            Self::Arboard(backup) => backup.text_to_restore(),
            Self::WlCopy { backup, .. } => backup.text_to_restore(),
        }
    }
}

pub fn capture_current_target() -> Option<PasteTarget> {
    paste_target_for_session(detect_session())
}

pub fn paste_text(text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    match detect_session() {
        LinuxSession::X11 => paste_text_x11(text),
        LinuxSession::Wayland => paste_text_wayland(text, target),
        LinuxSession::Unsupported => Err(anyhow!(
            "Automatic paste needs a desktop session with clipboard and paste support."
        )),
    }
}

fn paste_target_for_session(session: LinuxSession) -> Option<PasteTarget> {
    match session {
        LinuxSession::X11 => Some(PasteTarget {
            session: "x11".into(),
            hyprland_window: None,
        }),
        LinuxSession::Wayland => Some(PasteTarget {
            session: "wayland".into(),
            hyprland_window: capture_hyprland_active_window(),
        }),
        LinuxSession::Unsupported => None,
    }
}

fn paste_text_x11(text: &str) -> anyhow::Result<()> {
    let backup = write_clipboard_text(text)?;
    send_ctrl_v().context(
        "Could not send the X11 paste shortcut. Install xdotool or paste the result manually.",
    )?;

    if let Some(previous_text) = backup.text_to_restore().map(str::to_string) {
        thread::spawn(move || {
            thread::sleep(CLIPBOARD_RESTORE_DELAY);
            let _ = set_clipboard_text(&previous_text);
        });
    }

    Ok(())
}

fn paste_text_wayland(text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()> {
    let clipboard = write_wayland_clipboard_text(text)?;
    send_wayland_ctrl_v(target).with_context(|| wayland_paste_message())?;

    if let Some(previous_text) = clipboard.text_to_restore().map(str::to_string) {
        thread::spawn(move || {
            thread::sleep(CLIPBOARD_RESTORE_DELAY);
            match clipboard {
                ClipboardWrite::Arboard(_) => {
                    let _ = set_clipboard_text(&previous_text);
                }
                ClipboardWrite::WlCopy { wl_copy, .. } => {
                    let _ = set_wayland_clipboard_text(&wl_copy, &previous_text);
                }
            }
        });
    }

    Ok(())
}

fn write_clipboard_text(text: &str) -> anyhow::Result<ClipboardBackup> {
    let mut clipboard = arboard::Clipboard::new().context("failed to open Linux clipboard")?;
    let backup = clipboard
        .get_text()
        .map(ClipboardBackup::Text)
        .unwrap_or(ClipboardBackup::NonTextOrEmpty);
    clipboard
        .set_text(text.to_string())
        .context("failed to write Linux clipboard text")?;
    Ok(backup)
}

fn set_clipboard_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("failed to open Linux clipboard")?;
    clipboard
        .set_text(text.to_string())
        .context("failed to restore Linux clipboard text")
}

fn write_wayland_clipboard_text(text: &str) -> anyhow::Result<ClipboardWrite> {
    if let Some(wl_copy) = find_runtime_helper(WL_COPY_CANDIDATES) {
        let backup = find_runtime_helper(WL_PASTE_CANDIDATES)
            .as_deref()
            .map(read_wayland_clipboard_text)
            .transpose()?
            .unwrap_or(ClipboardBackup::NonTextOrEmpty);
        set_wayland_clipboard_text(&wl_copy, text)?;
        return Ok(ClipboardWrite::WlCopy { wl_copy, backup });
    }

    write_clipboard_text(text).map(ClipboardWrite::Arboard)
}

fn read_wayland_clipboard_text(wl_paste: &str) -> anyhow::Result<ClipboardBackup> {
    let mut command = Command::new(wl_paste);
    command.arg("--no-newline");
    apply_wayland_env(&mut command);
    let output = command.output().context("failed to run wl-paste")?;
    if !output.status.success() {
        return Ok(ClipboardBackup::NonTextOrEmpty);
    }

    String::from_utf8(output.stdout)
        .map(ClipboardBackup::Text)
        .or(Ok(ClipboardBackup::NonTextOrEmpty))
}

fn set_wayland_clipboard_text(wl_copy: &str, text: &str) -> anyhow::Result<()> {
    let mut command = Command::new(wl_copy);
    command.stdin(Stdio::piped());
    apply_wayland_env(&mut command);
    let mut child = command.spawn().context("failed to run wl-copy")?;
    let mut stdin = child.stdin.take().context("failed to open wl-copy stdin")?;
    stdin
        .write_all(text.as_bytes())
        .context("failed to write text to wl-copy")?;
    drop(stdin);

    let status = child.wait().context("failed to wait for wl-copy")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("wl-copy exited with status {status}"))
    }
}

fn send_ctrl_v() -> anyhow::Result<()> {
    let xdotool = find_runtime_helper(XDO_TOOL_CANDIDATES)
        .ok_or_else(|| anyhow!("xdotool was not found in known runtime locations."))?;
    let status = Command::new(&xdotool)
        .args(["key", "--clearmodifiers", "ctrl+v"])
        .status()
        .context("failed to run xdotool")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("xdotool exited with status {status}"))
    }
}

fn send_wayland_ctrl_v(target: Option<&PasteTarget>) -> anyhow::Result<()> {
    wait_for_modifier_release_before_paste()?;

    if is_hyprland_session() {
        let hyprctl = find_runtime_helper(HYPRCTL_CANDIDATES)
            .ok_or_else(|| anyhow!("Hyprland was detected, but hyprctl was not found."))?;
        return send_wayland_ctrl_v_with_hyprctl(&hyprctl, target);
    }

    let mut errors = Vec::new();

    if let Some(wtype) = find_runtime_helper(WTYPE_CANDIDATES) {
        match send_wayland_ctrl_v_with_wtype(&wtype) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("wtype: {error}")),
        }
    }

    if let Some(dotool) = find_runtime_helper(DOTOOL_CANDIDATES) {
        match send_ctrl_v_with_dotool(&dotool) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("dotool: {error}")),
        }
    }

    if let Some(ydotool) = find_runtime_helper(YDOTOOL_CANDIDATES) {
        match send_ctrl_v_with_ydotool(&ydotool) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("ydotool: {error}")),
        }
    }

    Err(if errors.is_empty() {
        anyhow!(
            "Text was copied to the clipboard, but Parrot could not press Ctrl+V automatically. Paste manually with Ctrl+V. No supported Wayland paste helper was found."
        )
    } else {
        anyhow!(
            "Text was copied to the clipboard, but Parrot could not press Ctrl+V automatically. Paste manually with Ctrl+V. Details: {}",
            errors.join("; ")
        )
    })
}

fn send_wayland_ctrl_v_with_wtype(wtype: &str) -> anyhow::Result<()> {
    let mut command = Command::new(wtype);
    command.args(["-M", "ctrl", "-P", "v", "-p", "v", "-m", "ctrl"]);
    apply_wayland_env(&mut command);
    let status = command.status().context("failed to run wtype")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("wtype exited with status {status}"))
    }
}

fn send_wayland_ctrl_v_with_hyprctl(
    hyprctl: &str,
    target: Option<&PasteTarget>,
) -> anyhow::Result<()> {
    let shortcut = hyprland_paste_shortcut(target);
    let mut command = Command::new(hyprctl);
    command.args(["dispatch", "sendshortcut", shortcut.as_str()]);
    apply_hyprland_env(&mut command);
    let status = command.status().context("failed to run hyprctl")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("hyprctl exited with status {status}"))
    }
}

fn hyprland_paste_shortcut(target: Option<&PasteTarget>) -> String {
    let selector = target
        .and_then(|target| target.hyprland_window.as_deref())
        .filter(|window| !window.trim().is_empty())
        .unwrap_or("activewindow");
    format!("CTRL, V, {selector}")
}

fn send_ctrl_v_with_dotool(dotool: &str) -> anyhow::Result<()> {
    let status = Command::new(dotool)
        .args(["key", "ctrl+v"])
        .status()
        .context("failed to run dotool")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("dotool exited with status {status}"))
    }
}

fn send_ctrl_v_with_ydotool(ydotool: &str) -> anyhow::Result<()> {
    let status = Command::new(ydotool)
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .status()
        .context("failed to run ydotool")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("ydotool exited with status {status}"))
    }
}

fn is_hyprland_session() -> bool {
    env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || env::var("XDG_CURRENT_DESKTOP")
            .map(|value| value.to_ascii_lowercase().contains("hyprland"))
            .unwrap_or(false)
        || inferred_runtime_dir()
            .as_deref()
            .and_then(discover_hyprland_signature)
            .is_some()
}

fn wait_for_modifier_release_before_paste() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        if crate::platform::evdev_hotkeys::can_open_any_keyboard() {
            crate::platform::evdev_hotkeys::wait_for_modifiers_released(Duration::from_millis(
                750,
            ))?;
        }
    }

    Ok(())
}

fn apply_hyprland_env(command: &mut Command) {
    let Some(runtime_dir) = inferred_runtime_dir() else {
        return;
    };

    if env::var_os("XDG_RUNTIME_DIR").is_none() {
        command.env("XDG_RUNTIME_DIR", &runtime_dir);
    }
    if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        if let Some(signature) = discover_hyprland_signature(&runtime_dir) {
            command.env("HYPRLAND_INSTANCE_SIGNATURE", signature);
        }
    }
}

fn apply_wayland_env(command: &mut Command) {
    let Some(runtime_dir) = inferred_runtime_dir() else {
        return;
    };

    if env::var_os("XDG_RUNTIME_DIR").is_none() {
        command.env("XDG_RUNTIME_DIR", &runtime_dir);
    }
    if env::var_os("WAYLAND_DISPLAY").is_none() {
        if let Some(display) = discover_wayland_display(&runtime_dir) {
            command.env("WAYLAND_DISPLAY", display);
        }
    }
}

fn inferred_runtime_dir() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| effective_uid().map(|uid| PathBuf::from(format!("/run/user/{uid}"))))
        .filter(|path| path.is_dir())
}

fn effective_uid() -> Option<String> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    effective_uid_from_proc_status(&status)
}

fn effective_uid_from_proc_status(status: &str) -> Option<String> {
    status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .map(str::to_string)
}

fn discover_wayland_display(runtime_dir: &Path) -> Option<String> {
    let mut displays = fs::read_dir(runtime_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().into_string().ok()?;
            if !file_name.starts_with("wayland-") {
                return None;
            }
            let file_type = entry.file_type().ok()?;
            file_type.is_socket().then_some(file_name)
        })
        .collect::<Vec<_>>();
    displays.sort();
    displays.pop()
}

fn capture_hyprland_active_window() -> Option<String> {
    if !is_hyprland_session() {
        return None;
    }

    let hyprctl = find_runtime_helper(HYPRCTL_CANDIDATES)?;
    let mut command = Command::new(&hyprctl);
    command.args(["activewindow", "-j"]);
    apply_hyprland_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    hyprland_window_selector(&value).or_else(|| capture_hyprland_previous_window(&hyprctl))
}

fn capture_hyprland_previous_window(hyprctl: &str) -> Option<String> {
    let mut command = Command::new(hyprctl);
    command.args(["clients", "-j"]);
    apply_hyprland_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let clients = value.as_array()?;
    let mut clients = clients.iter().collect::<Vec<_>>();
    clients.sort_by_key(|client| {
        client
            .get("focusHistoryID")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(i64::MAX)
    });
    clients.into_iter().find_map(hyprland_window_selector)
}

fn hyprland_window_selector(value: &serde_json::Value) -> Option<String> {
    if is_recording_overlay_window(value) {
        return None;
    }

    let address = value
        .get("address")
        .and_then(serde_json::Value::as_str)
        .filter(|address| !address.trim().is_empty())
        .filter(|address| *address != "0x0")?;
    Some(format!("address:{address}"))
}

fn is_recording_overlay_window(value: &serde_json::Value) -> bool {
    value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(|title| title == RECORDING_OVERLAY_TITLE)
        .unwrap_or(false)
}

fn discover_hyprland_signature(runtime_dir: &Path) -> Option<String> {
    let hypr_dir = runtime_dir.join("hypr");
    for entry in fs::read_dir(hypr_dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.join(".socket.sock").is_file() {
            return entry.file_name().to_str().map(str::to_string);
        }
    }
    None
}

fn find_runtime_helper(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .copied()
        .find(|candidate| Path::new(candidate).is_file())
        .map(str::to_string)
        .or_else(|| {
            env::var_os("PATH").and_then(|path| find_runtime_helper_on_path(candidates, &path))
        })
}

fn find_runtime_helper_on_path(candidates: &[&str], path: &OsStr) -> Option<String> {
    for candidate in candidates {
        let Some(name) = Path::new(candidate).file_name() else {
            continue;
        };
        for directory in env::split_paths(path) {
            let executable = directory.join(name);
            if executable.is_file() {
                return Some(executable.to_string_lossy().into_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_target_capture_is_stable_for_x11() {
        assert_eq!(
            paste_target_for_session(LinuxSession::X11),
            Some(PasteTarget {
                session: "x11".into(),
                hyprland_window: None,
            })
        );
        let wayland = paste_target_for_session(LinuxSession::Wayland)
            .expect("wayland should have a paste target");
        assert_eq!(wayland.session, "wayland");
    }

    #[test]
    fn paste_target_round_trips_hyprland_window_in_platform_id() {
        let target = PasteTarget {
            session: "wayland".into(),
            hyprland_window: Some("address:0xabc".into()),
        };

        let platform_id = target.platform_id();
        assert_eq!(
            PasteTarget::from_platform_id(platform_id),
            PasteTarget {
                session: "wayland".into(),
                hyprland_window: Some("address:0xabc".into()),
            }
        );

        assert_eq!(
            PasteTarget::from_platform_id("wayland"),
            PasteTarget {
                session: "wayland".into(),
                hyprland_window: None,
            }
        );
    }

    #[test]
    fn hyprland_window_selector_skips_recording_overlay() {
        assert_eq!(
            hyprland_window_selector(&serde_json::json!({
                "address": "0x123",
                "title": RECORDING_OVERLAY_TITLE
            })),
            None
        );

        assert_eq!(
            hyprland_window_selector(&serde_json::json!({
                "address": "0x456",
                "title": "Notes"
            })),
            Some("address:0x456".into())
        );
    }

    #[test]
    fn hyprland_paste_shortcut_targets_captured_window() {
        let target = PasteTarget {
            session: "wayland".into(),
            hyprland_window: Some("address:0xabc".into()),
        };

        assert_eq!(
            hyprland_paste_shortcut(Some(&target)),
            "CTRL, V, address:0xabc"
        );
    }

    #[test]
    fn hyprland_paste_shortcut_defaults_to_active_window() {
        assert_eq!(hyprland_paste_shortcut(None), "CTRL, V, activewindow");

        let target = PasteTarget {
            session: "wayland".into(),
            hyprland_window: None,
        };
        assert_eq!(
            hyprland_paste_shortcut(Some(&target)),
            "CTRL, V, activewindow"
        );
    }

    #[test]
    fn non_text_clipboard_backup_is_noop_restore() {
        assert_eq!(ClipboardBackup::NonTextOrEmpty.text_to_restore(), None);
        assert_eq!(
            ClipboardBackup::Text("previous".into()).text_to_restore(),
            Some("previous")
        );
    }

    #[test]
    fn missing_runtime_helper_is_reported_for_empty_path() {
        assert_eq!(
            find_runtime_helper_on_path(&["/definitely/not/xdotool"], OsStr::new("")),
            None
        );
    }

    #[test]
    fn runtime_helper_can_be_discovered_from_path() {
        let temp_dir = env::temp_dir().join(format!(
            "parrot-helper-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin_dir = temp_dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let helper = bin_dir.join("wtype");
        fs::write(&helper, []).unwrap();

        assert_eq!(
            find_runtime_helper_on_path(&["/usr/bin/wtype"], bin_dir.as_os_str()),
            Some(helper.to_string_lossy().into_owned())
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn effective_uid_parses_proc_status_line() {
        assert_eq!(
            effective_uid_from_proc_status("Name:\tparrot\nUid:\t1000\t1000\t1000\t1000\n"),
            Some("1000".into())
        );
    }
}
