use super::*;
use crate::platform::{LinuxDesktop, LinuxEnvironment};
use parrot_protocol::{
    default_linux_hands_free_shortcut, default_linux_push_to_talk_shortcut, ShortcutChord,
    ShortcutKey, ShortcutModifier, ShortcutPlatformCodes,
};

fn shortcut(name: &str, mode: ShortcutMode, keys: Vec<u32>) -> ShortcutSettings {
    ShortcutSettings {
        display_name: name.into(),
        mode,
        enabled: true,
        double_tap_toggle: false,
        chord: Some(ShortcutChord {
            modifiers: vec![ShortcutModifier::Control],
            key: Some(ShortcutKey::Space),
        }),
        platform_codes: ShortcutPlatformCodes {
            macos_key_codes: None,
            windows_virtual_keys: None,
            linux_key_codes: Some(keys),
        },
    }
}

#[test]
fn hold_shortcut_starts_on_key_down_and_stops_on_key_up() {
    let push = default_linux_push_to_talk_shortcut();
    let hands = default_linux_hands_free_shortcut();
    let mut engine = HotkeyEngine::new(push, hands);

    let down = engine.handle_key_event(XK_F1 + 8, KeyEventKind::Down);
    assert_eq!(
        down.actions,
        vec![HotkeyAction::Start {
            source: HotkeySource::PushToTalk
        }]
    );
    assert!(down.consume);

    let up = engine.handle_key_event(XK_F1 + 8, KeyEventKind::Up);
    assert_eq!(
        up.actions,
        vec![HotkeyAction::Stop {
            source: HotkeySource::PushToTalk
        }]
    );
    assert!(up.consume);
}

#[test]
fn generic_modifier_chord_matches_left_and_right_modifier_keys() {
    let push = default_linux_push_to_talk_shortcut();
    let hands = shortcut(
        "Ctrl + Alt + Space",
        ShortcutMode::Toggle,
        vec![XK_CONTROL_L, XK_ALT_L, XK_SPACE],
    );
    let mut engine = HotkeyEngine::new(push, hands);

    assert!(engine
        .handle_key_event(XK_CONTROL_R, KeyEventKind::Down)
        .actions
        .is_empty());
    assert!(engine
        .handle_key_event(XK_ALT_R, KeyEventKind::Down)
        .actions
        .is_empty());
    let space = engine.handle_key_event(XK_SPACE, KeyEventKind::Down);
    assert_eq!(
        space.actions,
        vec![HotkeyAction::Start {
            source: HotkeySource::HandsFree
        }]
    );
    assert!(space.consume);
}

#[test]
fn toggle_shortcut_alternates_on_new_chord_presses() {
    let push = default_linux_push_to_talk_shortcut();
    let hands = default_linux_hands_free_shortcut();
    let mut engine = HotkeyEngine::new(push, hands);

    for key in [XK_CONTROL_L, XK_SPACE] {
        let _ = engine.handle_key_event(key, KeyEventKind::Down);
    }
    for key in [XK_SPACE, XK_CONTROL_L] {
        let _ = engine.handle_key_event(key, KeyEventKind::Up);
    }
    let _ = engine.handle_key_event(XK_CONTROL_L, KeyEventKind::Down);
    let stop = engine.handle_key_event(XK_SPACE, KeyEventKind::Down);

    assert_eq!(
        stop.actions,
        vec![HotkeyAction::Stop {
            source: HotkeySource::HandsFree
        }]
    );
}

#[test]
fn escape_emits_cancel_only_when_cancellation_is_enabled() {
    let push = default_linux_push_to_talk_shortcut();
    let hands = default_linux_hands_free_shortcut();
    let mut engine = HotkeyEngine::new(push, hands);

    assert!(engine
        .handle_key_event(XK_ESCAPE, KeyEventKind::Down)
        .actions
        .is_empty());
    engine.set_cancellation_enabled(true);
    let outcome = engine.handle_key_event(XK_ESCAPE, KeyEventKind::Down);
    assert_eq!(outcome.actions, vec![HotkeyAction::Cancel]);
    assert!(outcome.consume);
}

#[test]
fn shortcut_rejects_modifier_only_chord() {
    let toggle = shortcut("Ctrl", ShortcutMode::Toggle, vec![XK_CONTROL_L]);

    let error = validate_shortcut(&toggle).unwrap_err();

    assert!(error
        .to_string()
        .contains("Linux shortcuts need a function key"));
}

#[test]
fn backend_selects_x11_on_x11() {
    let backend = choose_backend_with_availability(
        LinuxEnvironment {
            session: LinuxSession::X11,
            desktop: LinuxDesktop::Unknown,
        },
        BackendAvailability {
            compositor_installed: false,
            portal_available: false,
            evdev_available: false,
        },
    );

    assert_eq!(backend, LinuxHotkeyBackend::X11);
}

#[test]
fn backend_selects_compositor_when_hyprland_config_is_installed() {
    let backend = choose_backend_with_availability(
        LinuxEnvironment {
            session: LinuxSession::Wayland,
            desktop: LinuxDesktop::Hyprland,
        },
        BackendAvailability {
            compositor_installed: true,
            portal_available: true,
            evdev_available: true,
        },
    );

    assert_eq!(backend, LinuxHotkeyBackend::CompositorCommand);
}

#[test]
fn backend_falls_back_to_portal_when_no_compositor_config() {
    let backend = choose_backend_with_availability(
        LinuxEnvironment {
            session: LinuxSession::Wayland,
            desktop: LinuxDesktop::Hyprland,
        },
        BackendAvailability {
            compositor_installed: false,
            portal_available: true,
            evdev_available: true,
        },
    );

    assert_eq!(backend, LinuxHotkeyBackend::Portal);
}

#[test]
fn backend_falls_back_to_evdev_when_portal_is_unavailable() {
    let backend = choose_backend_with_availability(
        LinuxEnvironment {
            session: LinuxSession::Wayland,
            desktop: LinuxDesktop::Gnome,
        },
        BackendAvailability {
            compositor_installed: false,
            portal_available: false,
            evdev_available: true,
        },
    );

    assert_eq!(backend, LinuxHotkeyBackend::Evdev);
}

#[test]
#[ignore = "requires a real X11 session"]
fn real_x11_hotkey_monitor_smoke_test() {
    let monitor = HotkeyMonitor::default();
    let (tx, _rx) = mpsc::unbounded_channel();

    monitor
        .start(
            default_linux_push_to_talk_shortcut(),
            default_linux_hands_free_shortcut(),
            tx,
        )
        .unwrap();
    monitor.stop();
}
