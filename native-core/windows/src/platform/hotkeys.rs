use super::windows_keys::{
    generic_modifier_for, is_generic_modifier, is_modifier_virtual_key, virtual_key_sort_key,
    VK_ALT, VK_CONTROL, VK_ESCAPE, VK_LALT, VK_LCONTROL, VK_LSHIFT, VK_RALT, VK_RCONTROL,
    VK_RSHIFT, VK_SHIFT,
};
use anyhow::anyhow;
use parrot_protocol::{ShortcutMode, ShortcutSettings};
use std::{
    collections::HashSet,
    sync::{mpsc, Arc, Mutex},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    Start { source: HotkeySource },
    Stop { source: HotkeySource },
    Cancel,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySource {
    PushToTalk,
    HandsFree,
}

impl HotkeySource {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PushToTalk => "pushToTalk",
            Self::HandsFree => "handsFree",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Down,
    Up,
}

#[derive(Clone, Default)]
pub struct HotkeyMonitor {
    inner: Arc<Mutex<MonitorState>>,
}

#[derive(Default)]
struct MonitorState {
    hook: Option<PlatformHook>,
    action_tx: Option<mpsc::Sender<HotkeyAction>>,
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
        action_tx: mpsc::Sender<HotkeyAction>,
    ) -> anyhow::Result<()> {
        validate_shortcut_pair(&push_to_talk, &hands_free)?;

        self.stop();

        let engine = HotkeyEngine::new(push_to_talk, hands_free);
        let hook = PlatformHook::start(engine, action_tx.clone())?;
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

    pub fn is_running(&self) -> bool {
        self.inner
            .lock()
            .expect("hotkey monitor poisoned")
            .hook
            .is_some()
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

#[derive(Debug)]
struct Binding {
    source: HotkeySource,
    mode: ShortcutMode,
    required_keys: Vec<u16>,
    chord_active: bool,
    toggle_active: bool,
}

#[derive(Debug)]
pub struct HotkeyEngine {
    bindings: Vec<Binding>,
    pressed_actual_keys: HashSet<u16>,
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

    pub fn handle_key_event(&mut self, vk: u16, kind: KeyEventKind) -> HotkeyEngineOutcome {
        let vk = normalize_observed_virtual_key(vk);
        match kind {
            KeyEventKind::Down => {
                self.pressed_actual_keys.insert(vk);
            }
            KeyEventKind::Up => {
                self.pressed_actual_keys.remove(&vk);
            }
        }

        if kind == KeyEventKind::Down && vk == VK_ESCAPE && self.cancellation_enabled {
            return HotkeyEngineOutcome {
                actions: vec![HotkeyAction::Cancel],
                consume: true,
            };
        }

        let active_keys = active_virtual_keys(&self.pressed_actual_keys);
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

pub fn validate_shortcut_pair(
    push_to_talk: &ShortcutSettings,
    hands_free: &ShortcutSettings,
) -> anyhow::Result<()> {
    validate_shortcut(push_to_talk)?;
    validate_shortcut(hands_free)?;

    if push_to_talk.enabled && hands_free.enabled && shortcuts_conflict(push_to_talk, hands_free) {
        return Err(anyhow!(
            "Push to talk and hands-free mode need different shortcuts."
        ));
    }

    Ok(())
}

pub fn validate_shortcut(shortcut: &ShortcutSettings) -> anyhow::Result<()> {
    if !shortcut.enabled {
        return Ok(());
    }

    let keys = shortcut_required_keys(shortcut)
        .ok_or_else(|| anyhow!("Shortcut is missing Windows virtual keys."))?;
    if keys.is_empty() {
        return Err(anyhow!("Shortcut is missing Windows virtual keys."));
    }
    if keys.iter().any(|key| *key == 0) {
        return Err(anyhow!("Shortcut contains an invalid Windows virtual key."));
    }
    if keys.iter().any(|key| *key == VK_ESCAPE) {
        return Err(anyhow!("Escape is reserved for cancelling dictation."));
    }
    if keys
        .iter()
        .all(|key| is_modifier_virtual_key(*key) || is_generic_modifier(*key))
        && keys.len() > 1
    {
        return Err(anyhow!(
            "Choose a single modifier, or hold a modifier and press another key."
        ));
    }
    if matches!(shortcut.mode, ShortcutMode::Toggle)
        && keys
            .iter()
            .all(|key| is_modifier_virtual_key(*key) || is_generic_modifier(*key))
    {
        return Err(anyhow!(
            "Hands-free mode needs a modifier plus key or a function key."
        ));
    }

    Ok(())
}

pub fn shortcuts_conflict(a: &ShortcutSettings, b: &ShortcutSettings) -> bool {
    let Some(a_keys) = shortcut_required_keys(a) else {
        return false;
    };
    let Some(b_keys) = shortcut_required_keys(b) else {
        return false;
    };
    if a_keys.len() != b_keys.len() {
        return false;
    }

    a_keys.iter().all(|a_key| {
        b_keys
            .iter()
            .any(|b_key| virtual_keys_overlap(*a_key, *b_key))
    }) && b_keys.iter().all(|b_key| {
        a_keys
            .iter()
            .any(|a_key| virtual_keys_overlap(*a_key, *b_key))
    })
}

pub fn shortcut_required_keys(shortcut: &ShortcutSettings) -> Option<Vec<u16>> {
    let mut keys = shortcut
        .windows_virtual_keys()
        .iter()
        .copied()
        .map(normalize_configured_virtual_key)
        .filter(|key| *key != 0)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return None;
    }

    keys.sort_by_key(|key| virtual_key_sort_key(*key));
    keys.dedup();
    Some(keys)
}

fn required_keys_active(required_keys: &[u16], active_keys: &HashSet<u16>) -> bool {
    required_keys.iter().all(|required| {
        if active_keys.contains(required) {
            return true;
        }

        match *required {
            VK_CONTROL => active_keys.contains(&VK_LCONTROL) || active_keys.contains(&VK_RCONTROL),
            VK_ALT => active_keys.contains(&VK_LALT) || active_keys.contains(&VK_RALT),
            VK_SHIFT => active_keys.contains(&VK_LSHIFT) || active_keys.contains(&VK_RSHIFT),
            _ => false,
        }
    })
}

fn active_virtual_keys(pressed_actual_keys: &HashSet<u16>) -> HashSet<u16> {
    let mut active = pressed_actual_keys.clone();
    if active.contains(&VK_LCONTROL) || active.contains(&VK_RCONTROL) {
        active.insert(VK_CONTROL);
    }
    if active.contains(&VK_LALT) || active.contains(&VK_RALT) {
        active.insert(VK_ALT);
    }
    if active.contains(&VK_LSHIFT) || active.contains(&VK_RSHIFT) {
        active.insert(VK_SHIFT);
    }
    active
}

fn normalize_configured_virtual_key(vk: u16) -> u16 {
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => vk,
        VK_ALT | VK_LALT | VK_RALT => vk,
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => vk,
        _ => vk,
    }
}

fn normalize_observed_virtual_key(vk: u16) -> u16 {
    vk
}

fn virtual_keys_overlap(a: u16, b: u16) -> bool {
    a == b
        || generic_modifier_for(a)
            .zip(generic_modifier_for(b))
            .map(|(a, b)| a == b)
            .unwrap_or(false)
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use anyhow::Context;
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU32, Ordering},
            OnceLock, Weak,
        },
        thread,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
            TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
            LLKHF_LOWER_IL_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
            WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    };

    static ACTIVE_HOOK: OnceLock<Mutex<Option<Weak<HookShared>>>> = OnceLock::new();

    pub(super) struct PlatformHook {
        shared: Arc<HookShared>,
        join: Mutex<Option<thread::JoinHandle<()>>>,
    }

    struct HookShared {
        engine: Mutex<HotkeyEngine>,
        action_tx: mpsc::Sender<HotkeyAction>,
        thread_id: AtomicU32,
        running: AtomicBool,
    }

    impl PlatformHook {
        pub(super) fn start(
            engine: HotkeyEngine,
            action_tx: mpsc::Sender<HotkeyAction>,
        ) -> anyhow::Result<Self> {
            let shared = Arc::new(HookShared {
                engine: Mutex::new(engine),
                action_tx,
                thread_id: AtomicU32::new(0),
                running: AtomicBool::new(true),
            });
            let (startup_tx, startup_rx) = mpsc::channel();
            let thread_shared = Arc::clone(&shared);

            let join = thread::Builder::new()
                .name("Parrot Windows Hotkey Monitor".into())
                .spawn(move || hook_thread_main(thread_shared, startup_tx))
                .context("failed to spawn Windows hotkey monitor thread")?;

            match startup_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Ok(())) => Ok(Self {
                    shared,
                    join: Mutex::new(Some(join)),
                }),
                Ok(Err(error)) => {
                    let _ = join.join();
                    Err(error)
                }
                Err(_) => {
                    shared.running.store(false, Ordering::SeqCst);
                    let thread_id = shared.thread_id.load(Ordering::SeqCst);
                    if thread_id != 0 {
                        unsafe {
                            PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
                        }
                    }
                    let _ = join.join();
                    Err(anyhow!("Timed out while starting the Windows hotkey hook."))
                }
            }
        }

        pub(super) fn stop(self) {
            self.shared.running.store(false, Ordering::SeqCst);
            let thread_id = self.shared.thread_id.load(Ordering::SeqCst);
            if thread_id != 0 {
                unsafe {
                    PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
                }
            }
            if let Some(join) = self.join.lock().expect("hotkey hook poisoned").take() {
                let _ = join.join();
            }
        }

        pub(super) fn set_cancellation_enabled(&self, enabled: bool) {
            self.shared
                .engine
                .lock()
                .expect("hotkey engine poisoned")
                .set_cancellation_enabled(enabled);
        }

        pub(super) fn force_toggle_off(&self, source: HotkeySource) {
            self.shared
                .engine
                .lock()
                .expect("hotkey engine poisoned")
                .force_toggle_off(source);
        }
    }

    fn hook_thread_main(shared: Arc<HookShared>, startup_tx: mpsc::Sender<anyhow::Result<()>>) {
        shared.thread_id.store(
            unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
            Ordering::SeqCst,
        );

        let registry = ACTIVE_HOOK.get_or_init(|| Mutex::new(None));
        *registry.lock().expect("active hook poisoned") = Some(Arc::downgrade(&shared));

        let hook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0)
        };
        if hook.is_null() {
            let _ = startup_tx.send(Err(anyhow!(
                "Could not install the Windows global shortcut hook."
            )));
            *registry.lock().expect("active hook poisoned") = None;
            return;
        }

        let _ = startup_tx.send(Ok(()));

        let mut message: MSG = unsafe { std::mem::zeroed() };
        while shared.running.load(Ordering::SeqCst)
            && unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0
        {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        unsafe {
            UnhookWindowsHookEx(hook);
        }
        *registry.lock().expect("active hook poisoned") = None;
        shared.thread_id.store(0, Ordering::SeqCst);
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code != HC_ACTION as i32 {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }

        let event = &*(lparam as *const KBDLLHOOKSTRUCT);
        if event.flags & (LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED) != 0 {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        }

        let kind = match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => KeyEventKind::Down,
            WM_KEYUP | WM_SYSKEYUP => KeyEventKind::Up,
            _ => return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam),
        };

        let shared = ACTIVE_HOOK
            .get()
            .and_then(|registry| registry.lock().ok()?.as_ref()?.upgrade());
        let Some(shared) = shared else {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        };

        let outcome = shared
            .engine
            .lock()
            .expect("hotkey engine poisoned")
            .handle_key_event(event.vkCode as u16, kind);
        for action in outcome.actions {
            let _ = shared.action_tx.send(action);
        }

        if outcome.consume {
            1
        } else {
            CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub(super) struct PlatformHook {
        #[allow(dead_code)]
        shared: Arc<Mutex<HotkeyEngine>>,
        _action_tx: mpsc::Sender<HotkeyAction>,
    }

    impl PlatformHook {
        pub(super) fn start(
            engine: HotkeyEngine,
            action_tx: mpsc::Sender<HotkeyAction>,
        ) -> anyhow::Result<Self> {
            Ok(Self {
                shared: Arc::new(Mutex::new(engine)),
                _action_tx: action_tx,
            })
        }

        pub(super) fn stop(self) {}

        #[allow(dead_code)]
        pub(super) fn set_cancellation_enabled(&self, enabled: bool) {
            self.shared
                .lock()
                .expect("hotkey engine poisoned")
                .set_cancellation_enabled(enabled);
        }

        #[allow(dead_code)]
        pub(super) fn force_toggle_off(&self, source: HotkeySource) {
            self.shared
                .lock()
                .expect("hotkey engine poisoned")
                .force_toggle_off(source);
        }
    }
}

use platform::PlatformHook;

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::{
        default_windows_hands_free_shortcut, default_windows_push_to_talk_shortcut, ShortcutChord,
        ShortcutKey, ShortcutModifier, ShortcutPlatformCodes,
    };

    fn shortcut(name: &str, mode: ShortcutMode, keys: Vec<u16>) -> ShortcutSettings {
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
                windows_virtual_keys: Some(keys),
                linux_key_codes: None,
            },
        }
    }

    #[test]
    fn hold_shortcut_starts_on_key_down_and_stops_on_key_up() {
        let push = default_windows_push_to_talk_shortcut();
        let hands = default_windows_hands_free_shortcut();
        let mut engine = HotkeyEngine::new(push, hands);

        let down = engine.handle_key_event(VK_RCONTROL, KeyEventKind::Down);
        assert_eq!(
            down.actions,
            vec![HotkeyAction::Start {
                source: HotkeySource::PushToTalk
            }]
        );
        assert!(down.consume);

        let repeat = engine.handle_key_event(VK_RCONTROL, KeyEventKind::Down);
        assert!(repeat.actions.is_empty());
        assert!(repeat.consume);

        let up = engine.handle_key_event(VK_RCONTROL, KeyEventKind::Up);
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
        let push = default_windows_push_to_talk_shortcut();
        let hands = shortcut(
            "Generic Ctrl + Alt + Space",
            ShortcutMode::Toggle,
            vec![VK_CONTROL, VK_ALT, 0x20],
        );
        let mut engine = HotkeyEngine::new(push, hands);

        assert!(engine
            .handle_key_event(VK_LCONTROL, KeyEventKind::Down)
            .actions
            .is_empty());
        assert!(engine
            .handle_key_event(VK_RALT, KeyEventKind::Down)
            .actions
            .is_empty());
        let space = engine.handle_key_event(0x20, KeyEventKind::Down);
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
        let push = default_windows_push_to_talk_shortcut();
        let hands = default_windows_hands_free_shortcut();
        let mut engine = HotkeyEngine::new(push, hands);

        for key in [VK_LCONTROL, 0x20] {
            let _ = engine.handle_key_event(key, KeyEventKind::Down);
        }
        for key in [0x20, VK_LCONTROL] {
            let _ = engine.handle_key_event(key, KeyEventKind::Up);
        }
        let _ = engine.handle_key_event(VK_LCONTROL, KeyEventKind::Down);
        let stop = engine.handle_key_event(0x20, KeyEventKind::Down);

        assert_eq!(
            stop.actions,
            vec![HotkeyAction::Stop {
                source: HotkeySource::HandsFree
            }]
        );
    }

    #[test]
    fn rejected_toggle_start_can_be_forced_off_without_losing_chord_release() {
        let push = default_windows_push_to_talk_shortcut();
        let hands = default_windows_hands_free_shortcut();
        let mut engine = HotkeyEngine::new(push, hands);

        for key in [VK_LCONTROL, 0x20] {
            let _ = engine.handle_key_event(key, KeyEventKind::Down);
        }

        engine.force_toggle_off(HotkeySource::HandsFree);

        for key in [0x20, VK_LCONTROL] {
            let _ = engine.handle_key_event(key, KeyEventKind::Up);
        }

        let outcome = engine.handle_key_event(VK_LCONTROL, KeyEventKind::Down);
        assert!(outcome.actions.is_empty());

        let start_again = engine.handle_key_event(0x20, KeyEventKind::Down);
        assert_eq!(
            start_again.actions,
            vec![HotkeyAction::Start {
                source: HotkeySource::HandsFree
            }]
        );
    }

    #[test]
    fn dropping_monitor_clone_does_not_stop_running_monitor() {
        let monitor = HotkeyMonitor::default();
        let (action_tx, _action_rx) = mpsc::channel();

        monitor
            .start(
                default_windows_push_to_talk_shortcut(),
                default_windows_hands_free_shortcut(),
                action_tx,
            )
            .expect("hotkey monitor should start");

        let clone = monitor.clone();
        drop(clone);

        assert!(
            monitor.is_running(),
            "dropping a clone must not stop the shared hotkey monitor"
        );

        monitor.stop();
    }

    #[test]
    fn escape_emits_cancel_only_when_cancellation_is_enabled() {
        let push = default_windows_push_to_talk_shortcut();
        let hands = default_windows_hands_free_shortcut();
        let mut engine = HotkeyEngine::new(push, hands);

        assert!(engine
            .handle_key_event(VK_ESCAPE, KeyEventKind::Down)
            .actions
            .is_empty());
        engine.set_cancellation_enabled(true);
        let outcome = engine.handle_key_event(VK_ESCAPE, KeyEventKind::Down);
        assert_eq!(outcome.actions, vec![HotkeyAction::Cancel]);
        assert!(outcome.consume);
    }

    #[test]
    fn disabled_shortcuts_do_not_trigger() {
        let mut push = default_windows_push_to_talk_shortcut();
        push.enabled = false;
        let hands = default_windows_hands_free_shortcut();
        let mut engine = HotkeyEngine::new(push, hands);

        assert!(engine
            .handle_key_event(VK_RCONTROL, KeyEventKind::Down)
            .actions
            .is_empty());
    }

    #[test]
    fn duplicate_shortcut_detection_uses_windows_virtual_keys() {
        let push = default_windows_push_to_talk_shortcut();
        let duplicate = shortcut("Ctrl", ShortcutMode::Toggle, vec![VK_CONTROL]);

        assert!(shortcuts_conflict(&push, &duplicate));
        assert!(validate_shortcut_pair(&push, &duplicate).is_err());
    }

    #[test]
    fn different_windows_default_shortcuts_are_valid() {
        assert!(validate_shortcut_pair(
            &default_windows_push_to_talk_shortcut(),
            &default_windows_hands_free_shortcut()
        )
        .is_ok());
    }

    #[test]
    fn toggle_shortcut_rejects_modifier_only_chord() {
        let toggle = shortcut("Right Ctrl", ShortcutMode::Toggle, vec![VK_RCONTROL]);

        assert!(validate_shortcut(&toggle).is_err());
    }

    #[test]
    fn disabling_cancellation_resets_stale_pressed_keys_and_chords() {
        let push = default_windows_push_to_talk_shortcut();
        let hands = default_windows_hands_free_shortcut();
        let mut engine = HotkeyEngine::new(push, hands);

        let first = engine.handle_key_event(VK_RCONTROL, KeyEventKind::Down);
        assert_eq!(
            first.actions,
            vec![HotkeyAction::Start {
                source: HotkeySource::PushToTalk
            }]
        );

        engine.set_cancellation_enabled(false);

        let second = engine.handle_key_event(VK_RCONTROL, KeyEventKind::Down);
        assert_eq!(
            second.actions,
            vec![HotkeyAction::Start {
                source: HotkeySource::PushToTalk
            }]
        );
    }
}
