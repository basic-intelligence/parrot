use super::engine::HotkeyEngine;
use super::types::{HotkeyAction, HotkeySource};
use crate::platform::LinuxEnvironment;
use parrot_protocol::ShortcutSettings;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub(super) struct PlatformHook {
    shared: Arc<Mutex<HotkeyEngine>>,
    _action_tx: mpsc::UnboundedSender<HotkeyAction>,
}

impl PlatformHook {
    pub(super) fn start(
        engine: HotkeyEngine,
        action_tx: mpsc::UnboundedSender<HotkeyAction>,
        _environment: LinuxEnvironment,
        _push_to_talk: ShortcutSettings,
        _hands_free: ShortcutSettings,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            shared: Arc::new(Mutex::new(engine)),
            _action_tx: action_tx,
        })
    }

    pub(super) fn stop(self) {}

    pub(super) fn set_cancellation_enabled(&self, enabled: bool) {
        self.shared
            .lock()
            .expect("hotkey engine poisoned")
            .set_cancellation_enabled(enabled);
    }

    pub(super) fn force_toggle_off(&self, source: HotkeySource) {
        self.shared
            .lock()
            .expect("hotkey engine poisoned")
            .force_toggle_off(source);
    }
}
