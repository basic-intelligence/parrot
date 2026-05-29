use super::keys::{active_linux_keys, normalize_observed_key, required_keys_active, XK_ESCAPE};
use super::types::{HotkeyAction, HotkeySource, KeyEventKind};
use super::validation::shortcut_required_keys;
use parrot_protocol::{ShortcutMode, ShortcutSettings};
use std::collections::HashSet;

#[derive(Debug)]
struct Binding {
    source: HotkeySource,
    mode: ShortcutMode,
    required_keys: Vec<u32>,
    chord_active: bool,
    toggle_active: bool,
}

#[derive(Debug)]
pub struct HotkeyEngine {
    bindings: Vec<Binding>,
    pressed_actual_keys: HashSet<u32>,
    cancellation_enabled: bool,
}

impl HotkeyEngine {
    pub fn new(push_to_talk: ShortcutSettings, hands_free: ShortcutSettings) -> Self {
        let mut bindings = Vec::new();
        if push_to_talk.enabled {
            if let Some(required_keys) = shortcut_required_keys(&push_to_talk) {
                bindings.push(Binding {
                    source: HotkeySource::PushToTalk,
                    mode: ShortcutMode::Hold,
                    required_keys,
                    chord_active: false,
                    toggle_active: false,
                });
            }
        }
        if hands_free.enabled {
            if let Some(required_keys) = shortcut_required_keys(&hands_free) {
                bindings.push(Binding {
                    source: HotkeySource::HandsFree,
                    mode: ShortcutMode::Toggle,
                    required_keys,
                    chord_active: false,
                    toggle_active: false,
                });
            }
        }

        Self {
            bindings,
            pressed_actual_keys: HashSet::new(),
            cancellation_enabled: false,
        }
    }

    pub fn required_key_sets(&self) -> impl Iterator<Item = &[u32]> + '_ {
        self.bindings
            .iter()
            .map(|binding| binding.required_keys.as_slice())
    }

    pub fn set_cancellation_enabled(&mut self, enabled: bool) {
        self.cancellation_enabled = enabled;
        if !enabled {
            self.reset_active_state();
        }
    }

    pub fn force_toggle_off(&mut self, source: HotkeySource) {
        for binding in &mut self.bindings {
            if binding.source == source && binding.mode == ShortcutMode::Toggle {
                binding.toggle_active = false;
            }
        }
    }

    fn reset_active_state(&mut self) {
        self.pressed_actual_keys.clear();
        for binding in &mut self.bindings {
            binding.chord_active = false;
            binding.toggle_active = false;
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn handle_key_event(&mut self, key: u32, kind: KeyEventKind) -> HotkeyEngineOutcome {
        let key = normalize_observed_key(key);
        match kind {
            KeyEventKind::Down => {
                self.pressed_actual_keys.insert(key);
            }
            KeyEventKind::Up => {
                self.pressed_actual_keys.remove(&key);
            }
        }

        self.evaluate(key, kind)
    }

    #[cfg_attr(any(not(target_os = "linux"), test), allow(dead_code))]
    pub fn handle_active_keys(
        &mut self,
        event_key: u32,
        kind: KeyEventKind,
        active_keys: HashSet<u32>,
    ) -> HotkeyEngineOutcome {
        self.pressed_actual_keys = active_keys
            .into_iter()
            .map(normalize_observed_key)
            .collect();
        self.evaluate(normalize_observed_key(event_key), kind)
    }

    fn evaluate(&mut self, key: u32, kind: KeyEventKind) -> HotkeyEngineOutcome {
        if kind == KeyEventKind::Down && key == XK_ESCAPE && self.cancellation_enabled {
            return HotkeyEngineOutcome {
                actions: vec![HotkeyAction::Cancel],
                consume: true,
            };
        }

        let active_keys = active_linux_keys(&self.pressed_actual_keys);
        let mut actions = Vec::new();
        let mut consume = false;

        for binding in &mut self.bindings {
            let active = required_keys_active(&binding.required_keys, &active_keys);
            if active {
                consume = true;
            }

            match (&binding.mode, active, binding.chord_active) {
                (ShortcutMode::Hold, true, false) => {
                    binding.chord_active = true;
                    actions.push(HotkeyAction::Start {
                        source: binding.source,
                    });
                }
                (ShortcutMode::Hold, false, true) => {
                    binding.chord_active = false;
                    actions.push(HotkeyAction::Stop {
                        source: binding.source,
                    });
                    consume = true;
                }
                (ShortcutMode::Toggle, true, false) => {
                    binding.chord_active = true;
                    if binding.toggle_active {
                        binding.toggle_active = false;
                        actions.push(HotkeyAction::Stop {
                            source: binding.source,
                        });
                    } else {
                        binding.toggle_active = true;
                        actions.push(HotkeyAction::Start {
                            source: binding.source,
                        });
                    }
                }
                (ShortcutMode::Toggle, false, true) => {
                    binding.chord_active = false;
                    consume = true;
                }
                _ => {}
            }
        }

        HotkeyEngineOutcome { actions, consume }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HotkeyEngineOutcome {
    pub actions: Vec<HotkeyAction>,
    pub consume: bool,
}
