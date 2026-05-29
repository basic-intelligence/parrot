use crate::platform::{
    compositor, detect_environment, evdev_hotkeys, LinuxEnvironment, LinuxHotkeyBackend,
    LinuxSession,
};
use crate::platform::portal_probe;

#[derive(Debug, Clone, Copy)]
pub struct BackendAvailability {
    pub compositor_installed: bool,
    pub portal_available: bool,
    pub evdev_available: bool,
}

pub fn resolve_hotkey_backend(session: LinuxSession) -> LinuxHotkeyBackend {
    let mut environment = detect_environment();
    environment.session = session;
    choose_backend(environment)
}

#[cfg_attr(any(not(target_os = "linux"), test), allow(dead_code))]
pub fn choose_backend(environment: LinuxEnvironment) -> LinuxHotkeyBackend {
    match environment.session {
        LinuxSession::X11 => LinuxHotkeyBackend::X11,
        LinuxSession::Unsupported => LinuxHotkeyBackend::NeedsSetup,
        LinuxSession::Wayland => {
            choose_backend_with_availability(environment, runtime_backend_availability(environment))
        }
    }
}

#[cfg_attr(any(not(target_os = "linux"), test), allow(dead_code))]
pub fn runtime_backend_availability(environment: LinuxEnvironment) -> BackendAvailability {
    BackendAvailability {
        compositor_installed: compositor::compositor_shortcuts_installed(environment.desktop),
        portal_available: portal_probe::wayland_global_shortcuts_available(),
        evdev_available: evdev_hotkeys::can_open_any_keyboard(),
    }
}

pub fn choose_backend_with_availability(
    environment: LinuxEnvironment,
    availability: BackendAvailability,
) -> LinuxHotkeyBackend {
    match environment.session {
        LinuxSession::X11 => LinuxHotkeyBackend::X11,
        LinuxSession::Unsupported => LinuxHotkeyBackend::NeedsSetup,
        LinuxSession::Wayland => {
            if availability.compositor_installed {
                LinuxHotkeyBackend::CompositorCommand
            } else if availability.portal_available {
                LinuxHotkeyBackend::Portal
            } else if availability.evdev_available {
                LinuxHotkeyBackend::Evdev
            } else {
                LinuxHotkeyBackend::NeedsSetup
            }
        }
    }
}
