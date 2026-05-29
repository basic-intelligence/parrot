#[cfg(target_os = "linux")]
mod imp {
    use crate::platform::{
        hotkeys::{
            HotkeyAction, HotkeyEngine, HotkeySource, KeyEventKind, XK_ALT_L, XK_ALT_R,
            XK_CONTROL_L, XK_CONTROL_R, XK_DELETE, XK_DOWN, XK_ESCAPE, XK_F1, XK_LEFT, XK_META_L,
            XK_META_R, XK_RETURN, XK_RIGHT, XK_SHIFT_L, XK_SHIFT_R, XK_SPACE, XK_TAB, XK_UP,
        },
        shortcut_capture::{shortcut_from_linux_key_codes, CaptureState},
    };
    use anyhow::{anyhow, Context};
    use evdev::{Device, EventSummary, KeyCode};
    use parrot_protocol::{ShortcutMode, ShortcutSettings};
    use std::{
        io::ErrorKind,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };
    use tokio::sync::mpsc;

    const EVDEV_PERMISSION_MESSAGE: &str =
        "No readable Linux keyboard input devices were found. Add your user to the input group, log out, and log back in.";

    pub struct EvdevHotkeyHook {
        running: Arc<AtomicBool>,
        joins: Vec<thread::JoinHandle<()>>,
        engine: Arc<Mutex<HotkeyEngine>>,
    }

    impl EvdevHotkeyHook {
        pub fn start(
            engine: HotkeyEngine,
            action_tx: mpsc::UnboundedSender<HotkeyAction>,
        ) -> anyhow::Result<Self> {
            let devices = keyboard_event_devices()?;
            if devices.is_empty() {
                return Err(anyhow!(EVDEV_PERMISSION_MESSAGE));
            }

            let running = Arc::new(AtomicBool::new(true));
            let engine = Arc::new(Mutex::new(engine));
            let mut joins = Vec::new();

            for path in devices {
                let running = Arc::clone(&running);
                let engine = Arc::clone(&engine);
                let action_tx = action_tx.clone();

                joins.push(thread::spawn(move || {
                    if let Err(error) = device_loop(path, running, engine, action_tx) {
                        eprintln!("evdev hotkey device loop failed: {error}");
                    }
                }));
            }

            Ok(Self {
                running,
                joins,
                engine,
            })
        }

        pub fn stop(self) {
            self.running.store(false, Ordering::SeqCst);
            for join in self.joins {
                let _ = join.join();
            }
        }

        pub fn set_cancellation_enabled(&self, enabled: bool) {
            self.engine
                .lock()
                .expect("evdev hotkey engine poisoned")
                .set_cancellation_enabled(enabled);
        }

        pub fn force_toggle_off(&self, source: HotkeySource) {
            self.engine
                .lock()
                .expect("evdev hotkey engine poisoned")
                .force_toggle_off(source);
        }
    }

    pub fn can_open_any_keyboard() -> bool {
        keyboard_event_devices()
            .map(|devices| !devices.is_empty())
            .unwrap_or(false)
    }

    pub fn capture_shortcut(mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
        let paths = keyboard_event_devices()?;
        if paths.is_empty() {
            return Err(anyhow!(EVDEV_PERMISSION_MESSAGE));
        }

        let mut devices = open_capture_devices(paths)?;
        if devices.is_empty() {
            return Err(anyhow!(EVDEV_PERMISSION_MESSAGE));
        }

        capture_shortcut_from_devices(&mut devices, mode, Duration::from_secs(15))
    }

    fn open_capture_devices(paths: Vec<PathBuf>) -> anyhow::Result<Vec<(PathBuf, Device)>> {
        let mut devices = Vec::new();

        for path in paths {
            match Device::open(&path) {
                Ok(mut device) => {
                    device
                        .set_nonblocking(true)
                        .with_context(|| format!("could not set {} nonblocking", path.display()))?;
                    devices.push((path, device));
                }
                Err(error) => {
                    eprintln!("could not open evdev capture device: {error}");
                }
            }
        }

        Ok(devices)
    }

    fn capture_shortcut_from_devices(
        devices: &mut [(PathBuf, Device)],
        mode: ShortcutMode,
        timeout: Duration,
    ) -> anyhow::Result<ShortcutSettings> {
        let deadline = Instant::now() + timeout;
        let mut state = CaptureState::new(mode);

        while Instant::now() < deadline {
            for (path, device) in devices.iter_mut() {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            let EventSummary::Key(_, key, value) = event.destructure() else {
                                continue;
                            };
                            let Some(linux_key) = evdev_key_to_linux_key(key) else {
                                continue;
                            };

                            match value {
                                0 => {
                                    if let Some(shortcut) = state.handle_key_up(linux_key)? {
                                        return Ok(shortcut);
                                    }
                                }
                                1 => {
                                    if let Some(shortcut) = state.handle_key_down(linux_key)? {
                                        return Ok(shortcut);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                    Err(error) => {
                        eprintln!("failed reading {}: {error}", path.display());
                    }
                }
            }

            thread::sleep(Duration::from_millis(10));
        }

        Err(anyhow!(
            "Shortcut capture timed out. Press a shortcut within 15 seconds, or use Linux shortcut setup."
        ))
    }

    pub fn wait_for_modifiers_released(timeout: Duration) -> anyhow::Result<()> {
        let devices = keyboard_event_devices()?;
        if devices.is_empty() {
            return Ok(());
        }

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut modifiers_down = false;

            for path in &devices {
                let device = match Device::open(path) {
                    Ok(device) => device,
                    Err(_) => continue,
                };
                let Ok(state) = device.get_key_state() else {
                    continue;
                };

                if modifier_keys().iter().any(|key| state.contains(*key)) {
                    modifiers_down = true;
                    break;
                }
            }

            if !modifiers_down {
                return Ok(());
            }

            thread::sleep(Duration::from_millis(25));
        }

        Err(anyhow!(
            "modifier keys were still held after dictation finished"
        ))
    }

    fn keyboard_event_devices() -> anyhow::Result<Vec<PathBuf>> {
        let mut paths = Vec::new();

        for entry in std::fs::read_dir("/dev/input").context("failed to read /dev/input")? {
            let entry = entry?;
            let path = entry.path();

            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("event"))
                .unwrap_or(false)
            {
                continue;
            }

            if is_keyboard_device(&path).unwrap_or(false) {
                paths.push(path);
            }
        }

        Ok(paths)
    }

    fn is_keyboard_device(path: &Path) -> anyhow::Result<bool> {
        let device =
            Device::open(path).with_context(|| format!("could not open {}", path.display()))?;
        let Some(keys) = device.supported_keys() else {
            return Ok(false);
        };

        Ok(keys.contains(KeyCode::KEY_SPACE)
            && (keys.contains(KeyCode::KEY_A) || keys.contains(KeyCode::KEY_F9)))
    }

    fn device_loop(
        path: PathBuf,
        running: Arc<AtomicBool>,
        engine: Arc<Mutex<HotkeyEngine>>,
        action_tx: mpsc::UnboundedSender<HotkeyAction>,
    ) -> anyhow::Result<()> {
        let mut device =
            Device::open(&path).with_context(|| format!("could not open {}", path.display()))?;
        device
            .set_nonblocking(true)
            .with_context(|| format!("could not set {} nonblocking", path.display()))?;

        while running.load(Ordering::SeqCst) {
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        let EventSummary::Key(_, key, value) = event.destructure() else {
                            continue;
                        };

                        let kind = match value {
                            0 => KeyEventKind::Up,
                            1 => KeyEventKind::Down,
                            _ => continue,
                        };

                        let Some(linux_key) = evdev_key_to_linux_key(key) else {
                            continue;
                        };

                        let outcome = engine
                            .lock()
                            .expect("evdev hotkey engine poisoned")
                            .handle_key_event(linux_key, kind);

                        for action in outcome.actions {
                            let _ = action_tx.send(action);
                        }
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    eprintln!("evdev fetch_events failed for {}: {error}", path.display());
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }

        Ok(())
    }

    fn modifier_keys() -> &'static [KeyCode] {
        &[
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_RIGHTCTRL,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_RIGHTALT,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_RIGHTSHIFT,
            KeyCode::KEY_LEFTMETA,
            KeyCode::KEY_RIGHTMETA,
        ]
    }

    pub fn evdev_key_to_linux_key(key: KeyCode) -> Option<u32> {
        match key {
            KeyCode::KEY_SPACE => Some(XK_SPACE),
            KeyCode::KEY_TAB => Some(XK_TAB),
            KeyCode::KEY_ENTER => Some(XK_RETURN),
            KeyCode::KEY_ESC => Some(XK_ESCAPE),
            KeyCode::KEY_DELETE => Some(XK_DELETE),
            KeyCode::KEY_LEFT => Some(XK_LEFT),
            KeyCode::KEY_RIGHT => Some(XK_RIGHT),
            KeyCode::KEY_UP => Some(XK_UP),
            KeyCode::KEY_DOWN => Some(XK_DOWN),
            KeyCode::KEY_LEFTCTRL => Some(XK_CONTROL_L),
            KeyCode::KEY_RIGHTCTRL => Some(XK_CONTROL_R),
            KeyCode::KEY_LEFTALT => Some(XK_ALT_L),
            KeyCode::KEY_RIGHTALT => Some(XK_ALT_R),
            KeyCode::KEY_LEFTSHIFT => Some(XK_SHIFT_L),
            KeyCode::KEY_RIGHTSHIFT => Some(XK_SHIFT_R),
            KeyCode::KEY_LEFTMETA => Some(XK_META_L),
            KeyCode::KEY_RIGHTMETA => Some(XK_META_R),
            KeyCode::KEY_F1 => Some(XK_F1),
            KeyCode::KEY_F2 => Some(XK_F1 + 1),
            KeyCode::KEY_F3 => Some(XK_F1 + 2),
            KeyCode::KEY_F4 => Some(XK_F1 + 3),
            KeyCode::KEY_F5 => Some(XK_F1 + 4),
            KeyCode::KEY_F6 => Some(XK_F1 + 5),
            KeyCode::KEY_F7 => Some(XK_F1 + 6),
            KeyCode::KEY_F8 => Some(XK_F1 + 7),
            KeyCode::KEY_F9 => Some(XK_F1 + 8),
            KeyCode::KEY_F10 => Some(XK_F1 + 9),
            KeyCode::KEY_F11 => Some(XK_F1 + 10),
            KeyCode::KEY_F12 => Some(XK_F1 + 11),
            KeyCode::KEY_A => Some('a' as u32),
            KeyCode::KEY_B => Some('b' as u32),
            KeyCode::KEY_C => Some('c' as u32),
            KeyCode::KEY_D => Some('d' as u32),
            KeyCode::KEY_E => Some('e' as u32),
            KeyCode::KEY_F => Some('f' as u32),
            KeyCode::KEY_G => Some('g' as u32),
            KeyCode::KEY_H => Some('h' as u32),
            KeyCode::KEY_I => Some('i' as u32),
            KeyCode::KEY_J => Some('j' as u32),
            KeyCode::KEY_K => Some('k' as u32),
            KeyCode::KEY_L => Some('l' as u32),
            KeyCode::KEY_M => Some('m' as u32),
            KeyCode::KEY_N => Some('n' as u32),
            KeyCode::KEY_O => Some('o' as u32),
            KeyCode::KEY_P => Some('p' as u32),
            KeyCode::KEY_Q => Some('q' as u32),
            KeyCode::KEY_R => Some('r' as u32),
            KeyCode::KEY_S => Some('s' as u32),
            KeyCode::KEY_T => Some('t' as u32),
            KeyCode::KEY_U => Some('u' as u32),
            KeyCode::KEY_V => Some('v' as u32),
            KeyCode::KEY_W => Some('w' as u32),
            KeyCode::KEY_X => Some('x' as u32),
            KeyCode::KEY_Y => Some('y' as u32),
            KeyCode::KEY_Z => Some('z' as u32),
            KeyCode::KEY_0 => Some('0' as u32),
            KeyCode::KEY_1 => Some('1' as u32),
            KeyCode::KEY_2 => Some('2' as u32),
            KeyCode::KEY_3 => Some('3' as u32),
            KeyCode::KEY_4 => Some('4' as u32),
            KeyCode::KEY_5 => Some('5' as u32),
            KeyCode::KEY_6 => Some('6' as u32),
            KeyCode::KEY_7 => Some('7' as u32),
            KeyCode::KEY_8 => Some('8' as u32),
            KeyCode::KEY_9 => Some('9' as u32),
            _ => None,
        }
    }

    pub fn shortcut_from_evdev_keys(
        keys: Vec<KeyCode>,
        mode: ShortcutMode,
    ) -> anyhow::Result<ShortcutSettings> {
        let linux_keys = keys
            .into_iter()
            .filter_map(evdev_key_to_linux_key)
            .collect::<Vec<_>>();
        shortcut_from_linux_key_codes(linux_keys, mode)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_ctrl_space() {
            assert_eq!(
                evdev_key_to_linux_key(KeyCode::KEY_LEFTCTRL),
                Some(XK_CONTROL_L)
            );
            assert_eq!(evdev_key_to_linux_key(KeyCode::KEY_SPACE), Some(XK_SPACE));
        }

        #[test]
        fn maps_f9() {
            assert_eq!(evdev_key_to_linux_key(KeyCode::KEY_F9), Some(XK_F1 + 8));
        }

        #[test]
        fn shortcut_from_evdev_keys_uses_linux_key_codes() {
            let shortcut = shortcut_from_evdev_keys(
                vec![KeyCode::KEY_LEFTCTRL, KeyCode::KEY_SPACE],
                ShortcutMode::Toggle,
            )
            .unwrap();

            assert_eq!(shortcut.display_name, "Ctrl + Space");
            assert_eq!(shortcut.linux_key_codes(), &[XK_CONTROL_L, XK_SPACE]);
        }

        #[test]
        fn capture_shortcut_times_out() {
            let mut devices = Vec::new();
            let error = capture_shortcut_from_devices(
                &mut devices,
                ShortcutMode::Toggle,
                Duration::from_millis(0),
            )
            .unwrap_err();

            assert!(error.to_string().contains("Shortcut capture timed out"));
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    #![allow(dead_code)]

    use crate::platform::hotkeys::{HotkeyAction, HotkeyEngine, HotkeySource};
    use anyhow::anyhow;
    use parrot_protocol::{ShortcutMode, ShortcutSettings};
    use std::time::Duration;
    use tokio::sync::mpsc;

    pub struct EvdevHotkeyHook;

    impl EvdevHotkeyHook {
        pub fn start(
            _engine: HotkeyEngine,
            _action_tx: mpsc::UnboundedSender<HotkeyAction>,
        ) -> anyhow::Result<Self> {
            Err(anyhow!("evdev hotkeys are only available on Linux"))
        }

        pub fn stop(self) {}

        pub fn set_cancellation_enabled(&self, _enabled: bool) {}

        pub fn force_toggle_off(&self, _source: HotkeySource) {}
    }

    pub fn can_open_any_keyboard() -> bool {
        false
    }

    pub fn capture_shortcut(_mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
        Err(anyhow!("evdev shortcut capture is only available on Linux"))
    }

    pub fn wait_for_modifiers_released(_timeout: Duration) -> anyhow::Result<()> {
        Ok(())
    }
}

pub use imp::*;
