mod backend;
mod engine;
mod keys;
mod types;
mod validation;

#[cfg(all(target_os = "linux", not(test)))]
mod platform_linux;
#[cfg(any(not(target_os = "linux"), test))]
mod platform_stub;

#[cfg(all(target_os = "linux", not(test)))]
use platform_linux::PlatformHook;
#[cfg(any(not(target_os = "linux"), test))]
use platform_stub::PlatformHook;

pub use backend::resolve_hotkey_backend;
#[cfg(test)]
pub use backend::{choose_backend_with_availability, BackendAvailability};
pub use engine::HotkeyEngine;
pub use keys::{
    is_ascii_alphanumeric_key, is_modifier_key, linux_key_label, linux_key_sort_key,
    normalize_configured_key_for_capture, XK_ALT_L, XK_ALT_R, XK_CONTROL_L, XK_CONTROL_R,
    XK_DELETE, XK_DOWN, XK_ESCAPE, XK_F1, XK_F24, XK_LEFT, XK_META_L, XK_META_R, XK_RETURN,
    XK_RIGHT, XK_SHIFT_L, XK_SHIFT_R, XK_SPACE, XK_TAB, XK_UP,
};
pub use types::{HotkeyAction, HotkeySource, KeyEventKind};
#[cfg(test)]
pub use validation::validate_shortcut;
pub use validation::validate_shortcut_pair;

use crate::platform::{detect_environment, LinuxSession};
use anyhow::anyhow;
use parrot_protocol::ShortcutSettings;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct HotkeyMonitor {
    inner: Arc<Mutex<MonitorState>>,
}

impl Default for HotkeyMonitor {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MonitorState::default())),
        }
    }
}

#[derive(Default)]
struct MonitorState {
    hook: Option<PlatformHook>,
    action_tx: Option<mpsc::UnboundedSender<HotkeyAction>>,
}

impl Drop for MonitorState {
    fn drop(&mut self) {
        let _ = self.action_tx.take();
        if let Some(hook) = self.hook.take() {
            hook.stop();
        }
    }
}

impl std::fmt::Debug for HotkeyMonitor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HotkeyMonitor")
    }
}

impl HotkeyMonitor {
    pub fn start(
        &self,
        push_to_talk: ShortcutSettings,
        hands_free: ShortcutSettings,
        action_tx: mpsc::UnboundedSender<HotkeyAction>,
    ) -> anyhow::Result<()> {
        let environment = detect_environment();
        if environment.session == LinuxSession::X11 {
            validate_shortcut_pair(&push_to_talk, &hands_free)?;
        }

        match environment.session {
            LinuxSession::Unsupported => {
                return Err(anyhow!(
                    "Global shortcut dictation needs a desktop session."
                ))
            }
            LinuxSession::X11 | LinuxSession::Wayland => {}
        }

        self.stop();

        let engine = HotkeyEngine::new(push_to_talk.clone(), hands_free.clone());
        let hook = PlatformHook::start(
            engine,
            action_tx.clone(),
            environment,
            push_to_talk,
            hands_free,
        )?;
        let mut state = self.inner.lock().expect("hotkey monitor poisoned");
        state.hook = Some(hook);
        state.action_tx = Some(action_tx);
        Ok(())
    }

    pub fn stop(&self) {
        let hook = {
            let mut state = self.inner.lock().expect("hotkey monitor poisoned");
            let _ = state.action_tx.take();
            state.hook.take()
        };

        if let Some(hook) = hook {
            hook.stop();
        }
    }

    pub fn set_cancellation_enabled(&self, enabled: bool) {
        if let Some(hook) = self
            .inner
            .lock()
            .expect("hotkey monitor poisoned")
            .hook
            .as_ref()
        {
            hook.set_cancellation_enabled(enabled);
        }
    }

    pub fn force_toggle_off(&self, source: HotkeySource) {
        if let Some(hook) = self
            .inner
            .lock()
            .expect("hotkey monitor poisoned")
            .hook
            .as_ref()
        {
            hook.force_toggle_off(source);
        }
    }
}

#[cfg(test)]
mod tests;
