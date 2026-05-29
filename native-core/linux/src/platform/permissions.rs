use crate::platform::{
    audio::probe_input_device, compositor, desktop_supports_compositor_commands,
    detect_environment, detect_session, evdev_hotkeys, hotkeys::resolve_hotkey_backend,
    portal_probe, wayland_paste_message, LinuxDesktop, LinuxHotkeyBackend, LinuxSession,
};
use anyhow::anyhow;
use parrot_protocol::{
    LinuxHotkeyBackendKind, PermissionKind, PermissionRequirement, PermissionSnapshot,
    PermissionState,
};

#[derive(Debug, Default)]
pub struct PermissionManager;

impl PermissionManager {
    pub fn statuses(&self) -> PermissionSnapshot {
        linux_snapshot(microphone_state(None), detect_session())
    }

    pub fn request_permission(
        &self,
        kind: PermissionKind,
        _open_settings: bool,
    ) -> anyhow::Result<PermissionSnapshot> {
        match kind {
            PermissionKind::Microphone | PermissionKind::GlobalShortcut | PermissionKind::Paste => {
                Ok(self.statuses())
            }
            PermissionKind::Accessibility
            | PermissionKind::InputMonitoring
            | PermissionKind::FocusedTextContext => Err(anyhow!(
                "Linux permission `{kind:?}` does not have a native permission dialog."
            )),
        }
    }
}

fn linux_snapshot(microphone: PermissionState, session: LinuxSession) -> PermissionSnapshot {
    let (shortcut_state, shortcut_description) = linux_shortcut_state(session);
    let (paste_state, paste_description) = linux_platform_state(
        session,
        "Paste finished dictation into the focused text field.",
        wayland_paste_message(),
    );

    let shortcut_required = matches!(session, LinuxSession::X11 | LinuxSession::Wayland);
    let paste_required = matches!(session, LinuxSession::X11);
    let requirements = vec![
        PermissionRequirement {
            kind: PermissionKind::Microphone,
            title: "Microphone".into(),
            description: "Record your voice locally for dictation.".into(),
            state: microphone.clone(),
            required: true,
            requestable: false,
            opens_settings: false,
        },
        PermissionRequirement {
            kind: PermissionKind::GlobalShortcut,
            title: "Global shortcut".into(),
            description: shortcut_description,
            state: shortcut_state,
            required: shortcut_required,
            requestable: false,
            opens_settings: false,
        },
        PermissionRequirement {
            kind: PermissionKind::Paste,
            title: "Automatic paste".into(),
            description: paste_description,
            state: paste_state,
            required: paste_required,
            requestable: false,
            opens_settings: false,
        },
    ];

    let all_required_granted = requirements
        .iter()
        .filter(|requirement| requirement.required)
        .all(|requirement| requirement.state == PermissionState::Granted);

    PermissionSnapshot {
        requirements,
        all_required_granted,
        microphone: Some(microphone),
        accessibility: None,
        input_monitoring: None,
        all_granted: Some(all_required_granted),
        linux_hotkey_backend: Some(linux_hotkey_backend_kind(session)),
    }
}

fn linux_hotkey_backend_kind(session: LinuxSession) -> LinuxHotkeyBackendKind {
    match session {
        LinuxSession::Unsupported => LinuxHotkeyBackendKind::Unsupported,
        _ => match resolve_hotkey_backend(session) {
            LinuxHotkeyBackend::X11 => LinuxHotkeyBackendKind::X11,
            LinuxHotkeyBackend::CompositorCommand => LinuxHotkeyBackendKind::Compositor,
            LinuxHotkeyBackend::Portal => LinuxHotkeyBackendKind::Portal,
            LinuxHotkeyBackend::Evdev => LinuxHotkeyBackendKind::Evdev,
            LinuxHotkeyBackend::NeedsSetup => LinuxHotkeyBackendKind::NeedsSetup,
        },
    }
}

fn linux_platform_state(
    session: LinuxSession,
    x11_description: &str,
    wayland_description: &str,
) -> (PermissionState, String) {
    match session {
        LinuxSession::X11 => (PermissionState::Granted, x11_description.into()),
        LinuxSession::Wayland => (PermissionState::Granted, wayland_description.into()),
        LinuxSession::Unsupported => (
            PermissionState::Unknown,
            "Start Parrot from a desktop session so this feature can be checked.".into(),
        ),
    }
}

fn linux_shortcut_state(session: LinuxSession) -> (PermissionState, String) {
    let mut environment = detect_environment();
    environment.session = session;

    match environment.session {
        LinuxSession::X11 => (
            PermissionState::Granted,
            "Using X11 global keyboard grabs.".into(),
        ),
        LinuxSession::Wayland
            if compositor::compositor_shortcuts_installed(environment.desktop) =>
        {
            (
                PermissionState::Granted,
                "Using compositor keybindings that call Parrot record commands.".into(),
            )
        }
        LinuxSession::Wayland if portal_probe::wayland_global_shortcuts_available() => (
            PermissionState::Granted,
            "Using the XDG GlobalShortcuts portal for runtime shortcut activation.".into(),
        ),
        LinuxSession::Wayland if evdev_hotkeys::can_open_any_keyboard() => (
            PermissionState::Granted,
            "Using kernel-level evdev shortcut detection.".into(),
        ),
        LinuxSession::Wayland if environment.desktop == LinuxDesktop::Hyprland => (
            PermissionState::NotDetermined,
            "Install compositor shortcuts from Parrot to use this Wayland desktop without input-device permissions.".into(),
        ),
        LinuxSession::Wayland if desktop_supports_compositor_commands(environment.desktop) => (
            PermissionState::NotDetermined,
            "Configure compositor shortcuts to call Parrot record commands, or enable the evdev fallback by adding your user to the input group.".into(),
        ),
        LinuxSession::Wayland => (
            PermissionState::NotDetermined,
            "Enable the evdev fallback by adding your user to the input group, or configure desktop shortcuts manually.".into(),
        ),
        LinuxSession::Unsupported => (
            PermissionState::Unknown,
            "Start Parrot from a desktop session so this feature can be checked.".into(),
        ),
    }
}

fn microphone_state(selected_input_uid: Option<&str>) -> PermissionState {
    microphone_state_from_probe_result(probe_input_device(selected_input_uid))
}

fn microphone_state_from_probe_result(result: anyhow::Result<()>) -> PermissionState {
    match result {
        Ok(()) => PermissionState::Granted,
        Err(error) if is_known_microphone_denial(&error.to_string()) => PermissionState::Denied,
        Err(_) => PermissionState::Unknown,
    }
}

fn is_known_microphone_denial(message: &str) -> bool {
    let normalized = message.to_lowercase();
    [
        "access denied",
        "permission denied",
        "privacy",
        "not authorized",
        "unauthorized",
        "denied by system",
        "device access is denied",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::classify_session;

    #[test]
    fn linux_snapshot_uses_linux_requirements() {
        let snapshot = linux_snapshot(PermissionState::Granted, LinuxSession::X11);

        assert_eq!(snapshot.requirements.len(), 3);
        assert_eq!(snapshot.requirements[0].kind, PermissionKind::Microphone);
        assert_eq!(
            snapshot.requirements[1].kind,
            PermissionKind::GlobalShortcut
        );
        assert_eq!(snapshot.requirements[2].kind, PermissionKind::Paste);
        assert_eq!(snapshot.accessibility, None);
        assert_eq!(snapshot.input_monitoring, None);
        assert!(snapshot.all_required_granted);
    }

    #[test]
    fn linux_readiness_depends_on_required_linux_requirements() {
        let snapshot = linux_snapshot(PermissionState::Granted, LinuxSession::Wayland);

        assert!(snapshot.all_required_granted);
        assert!(snapshot.requirements[1].required);
        assert_eq!(snapshot.requirements[1].state, PermissionState::Granted);
    }

    #[test]
    fn classifies_known_privacy_errors_as_denied() {
        assert!(is_known_microphone_denial("Permission denied"));
        assert!(!is_known_microphone_denial("No input devices were found"));
    }

    #[test]
    fn maps_mocked_microphone_probe_results() {
        assert_eq!(
            microphone_state_from_probe_result(Ok(())),
            PermissionState::Granted
        );
        assert_eq!(
            microphone_state_from_probe_result(Err(anyhow!("Permission denied"))),
            PermissionState::Denied
        );
        assert_eq!(
            microphone_state_from_probe_result(Err(anyhow!("No input devices were found"))),
            PermissionState::Unknown
        );
    }

    #[test]
    fn classify_session_can_drive_snapshot_without_env() {
        let session = classify_session(Some("x11"), Some(":0"));
        assert!(linux_snapshot(PermissionState::Granted, session).all_required_granted);
    }
}
