use super::backend::choose_backend;
use super::engine::HotkeyEngine;
use super::keys::{
    is_modifier_key, normalize_configured_key, XK_ALT_L, XK_CONTROL_L, XK_ESCAPE, XK_META_L,
    XK_SHIFT_L,
};
use super::types::{HotkeyAction, HotkeySource, KeyEventKind};
use crate::platform::{desktop_supports_compositor_commands, evdev_hotkeys, wayland_hotkey_message, LinuxEnvironment, LinuxHotkeyBackend, LinuxSession};
use anyhow::anyhow;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::{pin_mut, StreamExt};
use parrot_protocol::{ShortcutKey, ShortcutModifier, ShortcutSettings};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tokio::sync::mpsc;
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{ConnectionExt, GrabMode, ModMask},
        Event,
    },
    rust_connection::RustConnection,
};

pub(super) struct PlatformHook {
    running: Option<Arc<AtomicBool>>,
    join: Mutex<Option<thread::JoinHandle<()>>>,
    engine: Option<Arc<Mutex<HotkeyEngine>>>,
    portal_state: Option<Arc<Mutex<PortalState>>>,
    evdev: Option<evdev_hotkeys::EvdevHotkeyHook>,
}

impl PlatformHook {
    pub(super) fn start(
        engine: HotkeyEngine,
        action_tx: mpsc::UnboundedSender<HotkeyAction>,
        environment: LinuxEnvironment,
        push_to_talk: ShortcutSettings,
        hands_free: ShortcutSettings,
    ) -> anyhow::Result<Self> {
        let backend = choose_backend(environment);

        match backend {
            LinuxHotkeyBackend::CompositorCommand => {
                return Ok(Self {
                    running: None,
                    join: Mutex::new(None),
                    engine: Some(Arc::new(Mutex::new(engine))),
                    portal_state: None,
                    evdev: None,
                });
            }
            LinuxHotkeyBackend::Evdev => {
                return Ok(Self {
                    running: None,
                    join: Mutex::new(None),
                    engine: None,
                    portal_state: None,
                    evdev: Some(evdev_hotkeys::EvdevHotkeyHook::start(engine, action_tx)?),
                });
            }
            LinuxHotkeyBackend::NeedsSetup => {
                return Err(needs_setup_error(environment));
            }
            LinuxHotkeyBackend::X11 | LinuxHotkeyBackend::Portal => {}
        }

        let session = match backend {
            LinuxHotkeyBackend::X11 => LinuxSession::X11,
            LinuxHotkeyBackend::Portal => LinuxSession::Wayland,
            LinuxHotkeyBackend::CompositorCommand
            | LinuxHotkeyBackend::Evdev
            | LinuxHotkeyBackend::NeedsSetup => unreachable!("handled above"),
        };
        let running = Arc::new(AtomicBool::new(true));
        let engine = Arc::new(Mutex::new(engine));
        let portal_state = (session == LinuxSession::Wayland)
            .then(|| Arc::new(Mutex::new(PortalState::default())));
        let thread_running = Arc::clone(&running);
        let thread_engine = Arc::clone(&engine);
        let thread_portal_state = portal_state.clone();
        let (setup_tx, setup_rx) = std_mpsc::channel();
        let join = thread::Builder::new()
            .name("Parrot Linux Hotkey Monitor".into())
            .spawn(move || {
                let result = match session {
                    LinuxSession::X11 => {
                        x11_hotkey_loop(thread_running, thread_engine, action_tx, setup_tx)
                    }
                    LinuxSession::Wayland => wayland_hotkey_loop(
                        thread_running,
                        thread_portal_state.expect("missing portal state"),
                        action_tx,
                        setup_tx,
                        push_to_talk,
                        hands_free,
                    ),
                    LinuxSession::Unsupported => Err(anyhow!(
                        "Global shortcut dictation needs a desktop session."
                    )),
                };

                if let Err(error) = result {
                    eprintln!("Linux hotkey monitor failed: {error}");
                }
            })?;

        let setup_timeout = match session {
            LinuxSession::Wayland => Duration::from_secs(120),
            LinuxSession::X11 | LinuxSession::Unsupported => Duration::from_secs(3),
        };
        match setup_rx.recv_timeout(setup_timeout) {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                running.store(false, Ordering::SeqCst);
                let _ = join.join();
                return Err(anyhow!(message));
            }
            Err(error) => {
                running.store(false, Ordering::SeqCst);
                let _ = join.join();
                return Err(anyhow!("Timed out starting Linux hotkey monitor: {error}"));
            }
        }

        Ok(Self {
            running: Some(running),
            join: Mutex::new(Some(join)),
            engine: Some(engine),
            portal_state,
            evdev: None,
        })
    }

    pub(super) fn stop(self) {
        if let Some(running) = self.running {
            running.store(false, Ordering::SeqCst);
        }
        if let Some(join) = self.join.lock().expect("hotkey hook poisoned").take() {
            let _ = join.join();
        }
        if let Some(evdev) = self.evdev {
            evdev.stop();
        }
    }

    pub(super) fn set_cancellation_enabled(&self, enabled: bool) {
        if let Some(evdev) = &self.evdev {
            evdev.set_cancellation_enabled(enabled);
            return;
        }
        if let Some(engine) = &self.engine {
            engine
                .lock()
                .expect("hotkey engine poisoned")
                .set_cancellation_enabled(enabled);
        }
    }

    pub(super) fn force_toggle_off(&self, source: HotkeySource) {
        if let Some(portal_state) = &self.portal_state {
            portal_state
                .lock()
                .expect("portal hotkey state poisoned")
                .force_toggle_off(source);
        }
        if let Some(evdev) = &self.evdev {
            evdev.force_toggle_off(source);
            return;
        }
        if let Some(engine) = &self.engine {
            engine
                .lock()
                .expect("hotkey engine poisoned")
                .force_toggle_off(source);
        }
    }
}

fn needs_setup_error(environment: LinuxEnvironment) -> anyhow::Error {
    if environment.session != LinuxSession::Wayland {
        return anyhow!("Global shortcut dictation needs a desktop session.");
    }

    if desktop_supports_compositor_commands(environment.desktop) {
        anyhow!(
            "Shortcut monitor needs setup on this Wayland compositor. Install compositor shortcuts from Parrot or enable the evdev fallback by adding your user to the input group, then log out and back in."
        )
    } else {
        anyhow!(
            "Shortcut monitor needs setup on this Wayland desktop. Enable the evdev fallback by adding your user to the input group, or configure desktop shortcuts manually."
        )
    }
}

#[derive(Debug, Default)]
struct PortalState {
    hands_free_active: bool,
}

impl PortalState {
    fn force_toggle_off(&mut self, source: HotkeySource) {
        if source == HotkeySource::HandsFree {
            self.hands_free_active = false;
        }
    }
}

fn wayland_hotkey_loop(
    running: Arc<AtomicBool>,
    portal_state: Arc<Mutex<PortalState>>,
    action_tx: mpsc::UnboundedSender<HotkeyAction>,
    setup_tx: std_mpsc::Sender<Result<(), String>>,
    push_to_talk: ShortcutSettings,
    hands_free: ShortcutSettings,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let shortcuts = portal_shortcuts(&push_to_talk, &hands_free);
        if shortcuts.is_empty() {
            let _ = setup_tx.send(Ok(()));
            while running.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            return Ok(());
        }

        let portal = GlobalShortcuts::new().await.map_err(|error| {
            anyhow!(
                "Could not connect to the Wayland global shortcuts portal: {error}. {}",
                wayland_hotkey_message()
            )
        })?;
        let session = portal.create_session().await?;
        let request = portal.bind_shortcuts(&session, &shortcuts, None).await?;
        request.response()?;
        let _ = setup_tx.send(Ok(()));

        let activated = portal.receive_activated().await?;
        let deactivated = portal.receive_deactivated().await?;
        pin_mut!(activated);
        pin_mut!(deactivated);
        let mut poll = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                _ = poll.tick() => {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                }
                Some(event) = activated.next() => {
                    handle_portal_activation(
                        event.shortcut_id(),
                        &portal_state,
                        &action_tx,
                    );
                }
                Some(event) = deactivated.next() => {
                    handle_portal_deactivation(
                        event.shortcut_id(),
                        &action_tx,
                    );
                }
                else => break,
            }
        }

        let _ = session.close().await;
        Ok::<(), anyhow::Error>(())
    })
}

fn portal_shortcuts(
    push_to_talk: &ShortcutSettings,
    hands_free: &ShortcutSettings,
) -> Vec<NewShortcut> {
    let mut shortcuts = Vec::new();
    if push_to_talk.enabled {
        shortcuts.push(portal_shortcut(
            "push-to-talk",
            "Push to talk",
            push_to_talk,
        ));
    }
    if hands_free.enabled {
        shortcuts.push(portal_shortcut(
            "hands-free",
            "Hands-free dictation",
            hands_free,
        ));
    }
    shortcuts
}

fn portal_shortcut(
    id: &'static str,
    description: &'static str,
    shortcut: &ShortcutSettings,
) -> NewShortcut {
    let mut portal_shortcut = NewShortcut::new(id, description);
    if let Some(preferred_trigger) = portal_preferred_trigger(shortcut) {
        portal_shortcut = portal_shortcut.preferred_trigger(Some(preferred_trigger.as_str()));
    }
    portal_shortcut
}

fn portal_preferred_trigger(shortcut: &ShortcutSettings) -> Option<String> {
    let chord = shortcut.chord.as_ref()?;
    let mut trigger = String::new();
    for modifier in &chord.modifiers {
        let label = match modifier {
            ShortcutModifier::Control => "Control",
            ShortcutModifier::Alt => "Alt",
            ShortcutModifier::Shift => "Shift",
            ShortcutModifier::Meta => "Super",
            ShortcutModifier::Command => "Super",
            ShortcutModifier::Option => "Alt",
            ShortcutModifier::Fn => continue,
        };
        trigger.push('<');
        trigger.push_str(label);
        trigger.push('>');
    }

    if let Some(key) = &chord.key {
        trigger.push_str(&portal_key_name(key)?);
    }

    (!trigger.is_empty()).then_some(trigger)
}

fn portal_key_name(key: &ShortcutKey) -> Option<String> {
    match key {
        ShortcutKey::Space => Some("space".into()),
        ShortcutKey::Return => Some("Return".into()),
        ShortcutKey::Tab => Some("Tab".into()),
        ShortcutKey::Delete => Some("Delete".into()),
        ShortcutKey::ArrowLeft => Some("Left".into()),
        ShortcutKey::ArrowRight => Some("Right".into()),
        ShortcutKey::ArrowUp => Some("Up".into()),
        ShortcutKey::ArrowDown => Some("Down".into()),
        ShortcutKey::Function(number) => Some(format!("F{number}")),
        ShortcutKey::Character(value) => value.chars().next().map(|value| value.to_string()),
        ShortcutKey::Escape => None,
    }
}

fn handle_portal_activation(
    shortcut_id: &str,
    portal_state: &Arc<Mutex<PortalState>>,
    action_tx: &mpsc::UnboundedSender<HotkeyAction>,
) {
    match shortcut_id {
        "push-to-talk" => {
            let _ = action_tx.send(HotkeyAction::Start {
                source: HotkeySource::PushToTalk,
            });
        }
        "hands-free" => {
            let mut portal_state = portal_state.lock().expect("portal hotkey state poisoned");
            let action = if portal_state.hands_free_active {
                portal_state.hands_free_active = false;
                HotkeyAction::Stop {
                    source: HotkeySource::HandsFree,
                }
            } else {
                portal_state.hands_free_active = true;
                HotkeyAction::Start {
                    source: HotkeySource::HandsFree,
                }
            };
            let _ = action_tx.send(action);
        }
        _ => {}
    }
}

fn handle_portal_deactivation(
    shortcut_id: &str,
    action_tx: &mpsc::UnboundedSender<HotkeyAction>,
) {
    if shortcut_id == "push-to-talk" {
        let _ = action_tx.send(HotkeyAction::Stop {
            source: HotkeySource::PushToTalk,
        });
    }
}

fn x11_hotkey_loop(
    running: Arc<AtomicBool>,
    engine: Arc<Mutex<HotkeyEngine>>,
    action_tx: mpsc::UnboundedSender<HotkeyAction>,
    setup_tx: std_mpsc::Sender<Result<(), String>>,
) -> anyhow::Result<()> {
    let setup = (|| -> anyhow::Result<(RustConnection, KeyboardMapping)> {
        let (conn, screen_num) = RustConnection::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let mapping = KeyboardMapping::load(&conn)?;
        let grabs = {
            let engine = engine.lock().expect("hotkey engine poisoned");
            engine
                .required_key_sets()
                .filter_map(|required_keys| grab_for_required_keys(required_keys, &mapping))
                .collect::<Vec<_>>()
        };

        for (keycode, modifiers) in grabs {
            for mask in lock_mask_variants(modifiers) {
                conn.grab_key(false, root, mask, keycode, GrabMode::ASYNC, GrabMode::ASYNC)?
                    .check()?;
            }
        }

        if let Some(escape_keycode) = mapping.keycode_for(XK_ESCAPE) {
            conn.grab_key(
                false,
                root,
                ModMask::ANY,
                escape_keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .check()?;
        }
        conn.flush()?;
        Ok((conn, mapping))
    })();

    let (conn, mapping) = match setup {
        Ok(setup) => {
            let _ = setup_tx.send(Ok(()));
            setup
        }
        Err(error) => {
            let _ = setup_tx.send(Err(error.to_string()));
            return Err(error);
        }
    };

    while running.load(Ordering::SeqCst) {
        if let Some(event) = conn.poll_for_event()? {
            match event {
                Event::KeyPress(event) => handle_x11_key(
                    &mapping,
                    &engine,
                    &action_tx,
                    event.detail,
                    event.state.into(),
                    KeyEventKind::Down,
                ),
                Event::KeyRelease(event) => handle_x11_key(
                    &mapping,
                    &engine,
                    &action_tx,
                    event.detail,
                    event.state.into(),
                    KeyEventKind::Up,
                ),
                _ => {}
            }
        } else {
            thread::sleep(Duration::from_millis(10));
        }
    }

    Ok(())
}

fn handle_x11_key(
    mapping: &KeyboardMapping,
    engine: &Arc<Mutex<HotkeyEngine>>,
    action_tx: &mpsc::UnboundedSender<HotkeyAction>,
    keycode: u8,
    state: u16,
    kind: KeyEventKind,
) {
    let Some(keysym) = mapping.keysym_for_keycode(keycode) else {
        return;
    };
    let mut active = active_keys_from_x11_state(state);
    if kind == KeyEventKind::Down {
        active.insert(keysym);
    }
    let outcome = engine
        .lock()
        .expect("hotkey engine poisoned")
        .handle_active_keys(keysym, kind, active);
    for action in outcome.actions {
        let _ = action_tx.send(action);
    }
}

fn grab_for_required_keys(
    required_keys: &[u32],
    mapping: &KeyboardMapping,
) -> Option<(u8, ModMask)> {
    let main = required_keys
        .iter()
        .copied()
        .find(|key| !is_modifier_key(*key))?;
    let mut mask = ModMask::default();
    for key in required_keys
        .iter()
        .copied()
        .filter(|key| is_modifier_key(*key))
    {
        mask |= match normalize_configured_key(key) {
            XK_CONTROL_L => ModMask::CONTROL,
            XK_ALT_L => ModMask::M1,
            XK_SHIFT_L => ModMask::SHIFT,
            XK_META_L => ModMask::M4,
            _ => ModMask::default(),
        };
    }
    mapping.keycode_for(main).map(|keycode| (keycode, mask))
}

fn lock_mask_variants(mask: ModMask) -> Vec<ModMask> {
    [
        ModMask::default(),
        ModMask::LOCK,
        ModMask::M2,
        ModMask::LOCK | ModMask::M2,
    ]
    .into_iter()
    .map(|lock_mask| mask | lock_mask)
    .collect()
}

fn active_keys_from_x11_state(state: u16) -> HashSet<u32> {
    let mut active = HashSet::new();
    if state & u16::from(ModMask::CONTROL) != 0 {
        active.insert(XK_CONTROL_L);
    }
    if state & u16::from(ModMask::M1) != 0 {
        active.insert(XK_ALT_L);
    }
    if state & u16::from(ModMask::SHIFT) != 0 {
        active.insert(XK_SHIFT_L);
    }
    if state & u16::from(ModMask::M4) != 0 {
        active.insert(XK_META_L);
    }
    active
}

struct KeyboardMapping {
    min_keycode: u8,
    keysyms_per_keycode: usize,
    keysyms: Vec<u32>,
}

impl KeyboardMapping {
    fn load(conn: &RustConnection) -> anyhow::Result<Self> {
        let setup = conn.setup();
        let min_keycode = setup.min_keycode;
        let count = setup.max_keycode - setup.min_keycode + 1;
        let reply = conn.get_keyboard_mapping(min_keycode, count)?.reply()?;
        Ok(Self {
            min_keycode,
            keysyms_per_keycode: reply.keysyms_per_keycode as usize,
            keysyms: reply.keysyms,
        })
    }

    fn keycode_for(&self, keysym: u32) -> Option<u8> {
        self.keysyms
            .chunks(self.keysyms_per_keycode)
            .position(|chunk| chunk.iter().any(|candidate| *candidate == keysym))
            .map(|index| self.min_keycode + index as u8)
    }

    fn keysym_for_keycode(&self, keycode: u8) -> Option<u32> {
        let index = keycode.checked_sub(self.min_keycode)? as usize;
        self.keysyms
            .chunks(self.keysyms_per_keycode)
            .nth(index)
            .and_then(|chunk| chunk.iter().copied().find(|keysym| *keysym != 0))
    }
}
