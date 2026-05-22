use crate::platform::windows_keys::{
    display_label_for_vk, is_generic_modifier, is_modifier_virtual_key, shortcut_key_for_vk,
    virtual_key_sort_key, VK_0, VK_9, VK_A, VK_ALT, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN,
    VK_ESCAPE, VK_F1, VK_F24, VK_LALT, VK_LCONTROL, VK_LEFT, VK_LSHIFT, VK_LWIN, VK_RALT,
    VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VK_Z,
};
use anyhow::anyhow;
use parrot_protocol::{
    ShortcutChord, ShortcutMode, ShortcutModifier, ShortcutPlatformCodes, ShortcutSettings,
};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutCaptureTarget {
    PushToTalk,
    HandsFree,
}

impl ShortcutCaptureTarget {
    pub fn mode(self) -> ShortcutMode {
        match self {
            Self::PushToTalk => ShortcutMode::Hold,
            Self::HandsFree => ShortcutMode::Toggle,
        }
    }
}

impl TryFrom<&str> for ShortcutCaptureTarget {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pushToTalkShortcut" => Ok(Self::PushToTalk),
            "handsFreeShortcut" => Ok(Self::HandsFree),
            _ => Err(anyhow!("Unknown shortcut capture target: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureKeyKind {
    Modifier,
    Function,
    Other,
}

#[derive(Debug)]
pub struct CaptureState {
    mode: ShortcutMode,
    current_modifiers: HashSet<u16>,
    max_modifier_count_during_chord: usize,
    saw_non_modifier_during_chord: bool,
}

impl CaptureState {
    pub fn new(mode: ShortcutMode) -> Self {
        Self {
            mode,
            current_modifiers: HashSet::new(),
            max_modifier_count_during_chord: 0,
            saw_non_modifier_during_chord: false,
        }
    }

    pub fn handle_key_down(&mut self, vk: u16) -> anyhow::Result<Option<ShortcutSettings>> {
        let vk = normalize_capture_virtual_key(vk);
        if vk == VK_ESCAPE {
            return Err(anyhow!("Shortcut capture cancelled."));
        }

        if is_capture_modifier(vk) {
            self.current_modifiers.insert(vk);
            self.max_modifier_count_during_chord = self
                .max_modifier_count_during_chord
                .max(self.current_modifiers.len());
            return Ok(None);
        }

        self.saw_non_modifier_during_chord = true;
        let key_kind = supported_key_kind(vk)
            .ok_or_else(|| anyhow!("That key is not supported yet. Try a letter, number, Space, arrow key, or function key."))?;

        if self.current_modifiers.is_empty() && key_kind != CaptureKeyKind::Function {
            return Err(anyhow!(
                "Use at least one modifier, like Ctrl, Alt, Shift, or Windows."
            ));
        }

        let mut keys = ordered_modifiers(&self.current_modifiers);
        keys.push(vk);
        shortcut_from_windows_virtual_keys(keys, self.mode.clone()).map(Some)
    }

    pub fn handle_key_up(&mut self, vk: u16) -> anyhow::Result<Option<ShortcutSettings>> {
        let vk = normalize_capture_virtual_key(vk);
        if !is_capture_modifier(vk) {
            return Ok(None);
        }

        let previous_modifiers = self.current_modifiers.clone();
        self.current_modifiers.remove(&vk);

        if self.mode == ShortcutMode::Hold
            && previous_modifiers.len() == 1
            && self.current_modifiers.is_empty()
            && self.max_modifier_count_during_chord == 1
            && !self.saw_non_modifier_during_chord
        {
            return shortcut_from_windows_virtual_keys(vec![vk], self.mode.clone()).map(Some);
        }

        if self.current_modifiers.is_empty() {
            self.max_modifier_count_during_chord = 0;
            self.saw_non_modifier_during_chord = false;
        }

        Ok(None)
    }
}

pub fn capture(target: ShortcutCaptureTarget) -> anyhow::Result<ShortcutSettings> {
    platform_capture(target.mode())
}

pub fn shortcut_from_windows_virtual_keys(
    mut keys: Vec<u16>,
    mode: ShortcutMode,
) -> anyhow::Result<ShortcutSettings> {
    keys.retain(|key| *key != 0);
    if keys.is_empty() {
        return Err(anyhow!("Shortcut is missing Windows virtual keys."));
    }

    for key in &mut keys {
        *key = normalize_capture_virtual_key(*key);
    }
    keys.sort_by_key(|key| virtual_key_sort_key(*key));
    keys.dedup();

    let modifier_keys = keys
        .iter()
        .copied()
        .filter(|key| is_capture_modifier(*key))
        .collect::<Vec<_>>();
    let non_modifier_keys = keys
        .iter()
        .copied()
        .filter(|key| !is_capture_modifier(*key))
        .collect::<Vec<_>>();

    if non_modifier_keys.len() > 1 {
        return Err(anyhow!("Choose one non-modifier key for the shortcut."));
    }
    if non_modifier_keys.is_empty() && modifier_keys.len() != 1 {
        return Err(anyhow!(
            "Choose a single modifier, or hold a modifier and press another key."
        ));
    }
    if mode == ShortcutMode::Toggle && non_modifier_keys.is_empty() {
        return Err(anyhow!(
            "Hands-free mode needs a modifier plus key or a function key."
        ));
    }

    let key_kind = non_modifier_keys
        .first()
        .and_then(|key| supported_key_kind(*key));
    if non_modifier_keys.is_empty() || key_kind == Some(CaptureKeyKind::Function) {
        // Single function keys are valid; single modifiers are valid only for hold mode.
    } else if modifier_keys.is_empty() {
        return Err(anyhow!(
            "Use at least one modifier, like Ctrl, Alt, Shift, or Windows."
        ));
    }

    let display_name = display_name_for_keys(&keys);
    let chord = ShortcutChord {
        modifiers: chord_modifiers_for_keys(&modifier_keys),
        key: non_modifier_keys
            .first()
            .and_then(|key| shortcut_key_for_vk(*key)),
    };

    Ok(ShortcutSettings {
        display_name,
        mode,
        enabled: true,
        double_tap_toggle: false,
        chord: Some(chord),
        platform_codes: ShortcutPlatformCodes {
            macos_key_codes: None,
            windows_virtual_keys: Some(keys),
            linux_key_codes: None,
        },
    })
}

fn normalize_capture_virtual_key(vk: u16) -> u16 {
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => vk,
        VK_ALT | VK_LALT | VK_RALT => vk,
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => vk,
        _ => vk,
    }
}

fn is_capture_modifier(vk: u16) -> bool {
    is_modifier_virtual_key(vk) || is_generic_modifier(vk)
}

fn supported_key_kind(vk: u16) -> Option<CaptureKeyKind> {
    if is_capture_modifier(vk) {
        return Some(CaptureKeyKind::Modifier);
    }
    match vk {
        VK_SPACE | VK_RETURN | VK_TAB | VK_BACK | VK_DELETE | VK_LEFT | VK_RIGHT | VK_UP
        | VK_DOWN => Some(CaptureKeyKind::Other),
        VK_0..=VK_9 | VK_A..=VK_Z => Some(CaptureKeyKind::Other),
        VK_F1..=VK_F24 => Some(CaptureKeyKind::Function),
        _ => None,
    }
}

fn ordered_modifiers(modifiers: &HashSet<u16>) -> Vec<u16> {
    let mut keys = modifiers.iter().copied().collect::<Vec<_>>();
    keys.sort_by_key(|key| virtual_key_sort_key(*key));
    keys
}

fn display_name_for_keys(keys: &[u16]) -> String {
    keys.iter()
        .map(|key| display_label_for_vk(*key))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn chord_modifiers_for_keys(keys: &[u16]) -> Vec<ShortcutModifier> {
    let mut modifiers = Vec::new();
    for key in keys {
        let modifier = match *key {
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => ShortcutModifier::Control,
            VK_ALT | VK_LALT | VK_RALT => ShortcutModifier::Alt,
            VK_SHIFT | VK_LSHIFT | VK_RSHIFT => ShortcutModifier::Shift,
            VK_LWIN | VK_RWIN => ShortcutModifier::Meta,
            _ => continue,
        };
        if !modifiers.contains(&modifier) {
            modifiers.push(modifier);
        }
    }
    modifiers
}

#[cfg(target_os = "windows")]
fn platform_capture(mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
    platform::capture(mode)
}

#[cfg(not(target_os = "windows"))]
fn platform_capture(_mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
    Err(anyhow!(
        "Windows shortcut capture is unavailable on this platform."
    ))
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use crate::platform::hotkeys::KeyEventKind;
    use anyhow::Context;
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU32, Ordering},
            mpsc, Arc, Mutex, OnceLock, Weak,
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

    static ACTIVE_CAPTURE: OnceLock<Mutex<Option<Weak<CaptureShared>>>> = OnceLock::new();

    struct CaptureShared {
        state: Mutex<CaptureState>,
        result_tx: Mutex<Option<mpsc::Sender<anyhow::Result<ShortcutSettings>>>>,
        thread_id: AtomicU32,
        completed: AtomicBool,
    }

    pub(super) fn capture(mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
        let shared = Arc::new(CaptureShared {
            state: Mutex::new(CaptureState::new(mode)),
            result_tx: Mutex::new(None),
            thread_id: AtomicU32::new(0),
            completed: AtomicBool::new(false),
        });
        let (result_tx, result_rx) = mpsc::channel();
        *shared.result_tx.lock().expect("shortcut capture poisoned") = Some(result_tx);
        let (startup_tx, startup_rx) = mpsc::channel();
        let thread_shared = Arc::clone(&shared);

        let join = thread::Builder::new()
            .name("Parrot Windows Shortcut Capture".into())
            .spawn(move || capture_thread_main(thread_shared, startup_tx))
            .context("failed to spawn Windows shortcut capture thread")?;

        match startup_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = join.join();
                return Err(error);
            }
            Err(_) => {
                finish_capture(
                    &shared,
                    Err(anyhow!("Timed out while starting shortcut capture.")),
                );
                let _ = join.join();
                return Err(anyhow!("Timed out while starting shortcut capture."));
            }
        }

        let result = result_rx
            .recv()
            .context("shortcut capture channel closed before a shortcut was captured")?;
        let _ = join.join();
        result
    }

    fn capture_thread_main(
        shared: Arc<CaptureShared>,
        startup_tx: mpsc::Sender<anyhow::Result<()>>,
    ) {
        shared.thread_id.store(
            unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
            Ordering::SeqCst,
        );

        let registry = ACTIVE_CAPTURE.get_or_init(|| Mutex::new(None));
        *registry.lock().expect("shortcut capture registry poisoned") =
            Some(Arc::downgrade(&shared));

        let hook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(capture_hook), std::ptr::null_mut(), 0)
        };
        if hook.is_null() {
            let error = anyhow!("Could not start shortcut capture.");
            let _ = startup_tx.send(Err(error));
            *registry.lock().expect("shortcut capture registry poisoned") = None;
            return;
        }

        let _ = startup_tx.send(Ok(()));
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while !shared.completed.load(Ordering::SeqCst)
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
        *registry.lock().expect("shortcut capture registry poisoned") = None;
        shared.thread_id.store(0, Ordering::SeqCst);
    }

    unsafe extern "system" fn capture_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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

        let shared = ACTIVE_CAPTURE
            .get()
            .and_then(|registry| registry.lock().ok()?.as_ref()?.upgrade());
        let Some(shared) = shared else {
            return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
        };

        let result = {
            let mut state = shared
                .state
                .lock()
                .expect("shortcut capture state poisoned");
            match kind {
                KeyEventKind::Down => state.handle_key_down(event.vkCode as u16),
                KeyEventKind::Up => state.handle_key_up(event.vkCode as u16),
            }
        };

        match result {
            Ok(Some(shortcut)) => finish_capture(&shared, Ok(shortcut)),
            Ok(None) => {}
            Err(error) => finish_capture(&shared, Err(error)),
        }

        1
    }

    fn finish_capture(shared: &Arc<CaptureShared>, result: anyhow::Result<ShortcutSettings>) {
        if shared.completed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(tx) = shared
            .result_tx
            .lock()
            .expect("shortcut capture poisoned")
            .take()
        {
            let _ = tx.send(result);
        }
        let thread_id = shared.thread_id.load(Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::ShortcutKey;

    #[test]
    fn builds_single_modifier_hold_shortcut_with_windows_virtual_keys() {
        let shortcut =
            shortcut_from_windows_virtual_keys(vec![VK_RCONTROL], ShortcutMode::Hold).unwrap();

        assert_eq!(shortcut.display_name, "Right Ctrl");
        assert_eq!(shortcut.mode, ShortcutMode::Hold);
        assert!(shortcut.enabled);
        assert!(!shortcut.double_tap_toggle);
        assert_eq!(shortcut.windows_virtual_keys(), &[VK_RCONTROL]);
        assert_eq!(
            shortcut.chord.as_ref().unwrap().modifiers,
            vec![ShortcutModifier::Control]
        );
        assert_eq!(shortcut.chord.as_ref().unwrap().key, None);
    }

    #[test]
    fn builds_modifier_key_chord() {
        let shortcut = shortcut_from_windows_virtual_keys(
            vec![VK_LCONTROL, VK_LALT, VK_SPACE],
            ShortcutMode::Toggle,
        )
        .unwrap();

        assert_eq!(shortcut.display_name, "Left Ctrl + Left Alt + Space");
        assert_eq!(
            shortcut.chord.as_ref().unwrap().modifiers,
            vec![ShortcutModifier::Control, ShortcutModifier::Alt]
        );
        assert_eq!(
            shortcut.chord.as_ref().unwrap().key,
            Some(ShortcutKey::Space)
        );
        assert_eq!(
            shortcut.windows_virtual_keys(),
            &[VK_LCONTROL, VK_LALT, VK_SPACE]
        );
    }

    #[test]
    fn builds_function_key_capture_without_modifier() {
        let shortcut = shortcut_from_windows_virtual_keys(vec![VK_F1 + 4], ShortcutMode::Hold)
            .expect("function key shortcut should be valid");

        assert_eq!(shortcut.display_name, "F5");
        assert_eq!(
            shortcut.chord.as_ref().unwrap().key,
            Some(ShortcutKey::Function(5))
        );
    }

    #[test]
    fn rejects_toggle_modifier_only_capture() {
        let error = shortcut_from_windows_virtual_keys(vec![VK_RCONTROL], ShortcutMode::Toggle)
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("Hands-free mode needs a modifier plus key"));
    }

    #[test]
    fn capture_state_cancels_on_escape() {
        let mut state = CaptureState::new(ShortcutMode::Hold);
        let error = state.handle_key_down(VK_ESCAPE).unwrap_err();

        assert_eq!(error.to_string(), "Shortcut capture cancelled.");
    }

    #[test]
    fn capture_state_saves_single_modifier_on_release_for_hold_mode() {
        let mut state = CaptureState::new(ShortcutMode::Hold);

        assert!(state.handle_key_down(VK_RCONTROL).unwrap().is_none());
        let shortcut = state.handle_key_up(VK_RCONTROL).unwrap().unwrap();

        assert_eq!(shortcut.display_name, "Right Ctrl");
        assert_eq!(shortcut.windows_virtual_keys(), &[VK_RCONTROL]);
    }

    #[test]
    fn capture_state_saves_modifier_key_chord_on_key_down() {
        let mut state = CaptureState::new(ShortcutMode::Toggle);

        assert!(state.handle_key_down(VK_LCONTROL).unwrap().is_none());
        assert!(state.handle_key_down(VK_LALT).unwrap().is_none());
        let shortcut = state.handle_key_down(VK_SPACE).unwrap().unwrap();

        assert_eq!(shortcut.display_name, "Left Ctrl + Left Alt + Space");
        assert_eq!(
            shortcut.windows_virtual_keys(),
            &[VK_LCONTROL, VK_LALT, VK_SPACE]
        );
    }
}
