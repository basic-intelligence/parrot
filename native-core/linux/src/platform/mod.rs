pub mod audio;
mod portal_probe;
pub mod evdev_hotkeys;
pub mod focused_text;
pub mod hotkeys;
pub mod paste;
pub mod permissions;
pub mod pulse_audio;
pub mod shortcut_capture;
pub mod sound;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxSession {
    X11,
    Wayland,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDesktop {
    Hyprland,
    Sway,
    River,
    Niri,
    Gnome,
    Kde,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxHotkeyBackend {
    X11,
    CompositorCommand,
    Portal,
    Evdev,
    NeedsSetup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxEnvironment {
    pub session: LinuxSession,
    pub desktop: LinuxDesktop,
}

pub fn detect_environment() -> LinuxEnvironment {
    LinuxEnvironment {
        session: detect_session(),
        desktop: detect_desktop(),
    }
}

pub fn detect_session() -> LinuxSession {
    #[cfg(test)]
    {
        return LinuxSession::X11;
    }

    #[cfg(not(test))]
    classify_session(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    )
}

pub fn classify_session(session_type: Option<&str>, display: Option<&str>) -> LinuxSession {
    match session_type
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("wayland") => LinuxSession::Wayland,
        Some("x11") if display.filter(|value| !value.trim().is_empty()).is_some() => {
            LinuxSession::X11
        }
        Some("x11") => LinuxSession::Unsupported,
        _ if display.filter(|value| !value.trim().is_empty()).is_some() => LinuxSession::X11,
        _ => LinuxSession::Unsupported,
    }
}

pub fn detect_desktop() -> LinuxDesktop {
    classify_desktop(
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var("XDG_SESSION_DESKTOP").ok().as_deref(),
    )
}

pub fn classify_desktop(
    hyprland_signature: Option<&str>,
    current_desktop: Option<&str>,
    session_desktop: Option<&str>,
) -> LinuxDesktop {
    if hyprland_signature
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return LinuxDesktop::Hyprland;
    }

    let current_desktop = current_desktop.unwrap_or_default().to_ascii_lowercase();
    let session_desktop = session_desktop.unwrap_or_default().to_ascii_lowercase();
    let combined = format!("{current_desktop}:{session_desktop}");

    if combined.contains("hyprland") {
        LinuxDesktop::Hyprland
    } else if combined.contains("sway") {
        LinuxDesktop::Sway
    } else if combined.contains("river") {
        LinuxDesktop::River
    } else if combined.contains("niri") {
        LinuxDesktop::Niri
    } else if combined.contains("gnome") {
        LinuxDesktop::Gnome
    } else if combined.contains("kde") || combined.contains("plasma") {
        LinuxDesktop::Kde
    } else {
        LinuxDesktop::Unknown
    }
}

pub fn desktop_supports_compositor_commands(desktop: LinuxDesktop) -> bool {
    matches!(
        desktop,
        LinuxDesktop::Hyprland | LinuxDesktop::Sway | LinuxDesktop::River | LinuxDesktop::Niri
    )
}

#[cfg_attr(any(not(target_os = "linux"), test), allow(dead_code))]
pub fn wayland_hotkey_message() -> &'static str {
    "Use compositor shortcuts, the XDG GlobalShortcuts portal, or the evdev fallback for Linux dictation shortcuts."
}

pub fn wayland_paste_message() -> &'static str {
    "Automatic paste needs a Wayland clipboard and a compositor paste helper."
}

pub mod compositor {
    use super::{desktop_supports_compositor_commands, LinuxDesktop};
    use std::path::{Path, PathBuf};

    const HYPRLAND_SOURCE_LINE: &str = "source = ~/.config/hypr/parrot.conf";

    pub fn compositor_shortcuts_installed(desktop: LinuxDesktop) -> bool {
        match desktop {
            LinuxDesktop::Hyprland => std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| hyprland_shortcuts_installed_in_home(&home))
                .unwrap_or(false),
            desktop if desktop_supports_compositor_commands(desktop) => false,
            _ => false,
        }
    }

    pub fn hyprland_shortcuts_installed_in_home(home: &Path) -> bool {
        let hypr_dir = home.join(".config").join("hypr");
        let parrot_conf = hypr_dir.join("parrot.conf");
        let hyprland_conf = hypr_dir.join("hyprland.conf");

        parrot_conf.is_file()
            && std::fs::read_to_string(hyprland_conf)
                .map(|contents| contents.contains(HYPRLAND_SOURCE_LINE))
                .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_wayland_before_display() {
        assert_eq!(
            classify_session(Some("wayland"), Some(":0")),
            LinuxSession::Wayland
        );
    }

    #[test]
    fn treats_display_as_x11_when_session_is_unset() {
        assert_eq!(classify_session(None, Some(":1")), LinuxSession::X11);
    }

    #[test]
    fn missing_display_is_unsupported() {
        assert_eq!(
            classify_session(Some("x11"), None),
            LinuxSession::Unsupported
        );
        assert_eq!(classify_session(None, None), LinuxSession::Unsupported);
    }

    #[test]
    fn detect_desktop_from_env_hyprland() {
        assert_eq!(
            classify_desktop(Some("abc123"), Some("GNOME"), None),
            LinuxDesktop::Hyprland
        );
        assert_eq!(
            classify_desktop(None, Some("Hyprland"), None),
            LinuxDesktop::Hyprland
        );
    }

    #[test]
    fn detect_desktop_from_env_wlroots_compositors() {
        assert_eq!(
            classify_desktop(None, Some("sway"), None),
            LinuxDesktop::Sway
        );
        assert_eq!(
            classify_desktop(None, Some("river"), None),
            LinuxDesktop::River
        );
        assert_eq!(
            classify_desktop(None, Some("niri"), None),
            LinuxDesktop::Niri
        );
    }

    #[test]
    fn detect_desktop_from_env_gnome_and_kde() {
        assert_eq!(
            classify_desktop(None, Some("GNOME"), None),
            LinuxDesktop::Gnome
        );
        assert_eq!(
            classify_desktop(None, Some("KDE"), Some("plasma")),
            LinuxDesktop::Kde
        );
    }

    #[test]
    fn compositor_installed_requires_hyprland_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let hypr_dir = temp_dir.path().join(".config").join("hypr");
        std::fs::create_dir_all(&hypr_dir).unwrap();
        std::fs::write(hypr_dir.join("parrot.conf"), "# managed by Parrot\n").unwrap();
        std::fs::write(
            hypr_dir.join("hyprland.conf"),
            "source = ~/.config/hypr/parrot.conf\n",
        )
        .unwrap();

        assert!(compositor::hyprland_shortcuts_installed_in_home(
            temp_dir.path()
        ));
    }
}
