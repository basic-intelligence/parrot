use crate::platform::{
    detect_session, evdev_hotkeys,
    hotkeys::{
        is_ascii_alphanumeric_key, is_modifier_key, linux_key_label, linux_key_sort_key,
        normalize_configured_key_for_capture, XK_ALT_L, XK_ALT_R, XK_CONTROL_L, XK_CONTROL_R,
        XK_DELETE, XK_DOWN, XK_ESCAPE, XK_F1, XK_F24, XK_LEFT, XK_META_L, XK_META_R, XK_RETURN,
        XK_RIGHT, XK_SHIFT_L, XK_SHIFT_R, XK_SPACE, XK_TAB, XK_UP,
    },
    LinuxSession,
};
use anyhow::anyhow;
use parrot_protocol::{
    ShortcutChord, ShortcutKey, ShortcutMode, ShortcutModifier, ShortcutPlatformCodes,
    ShortcutSettings,
};
use std::collections::HashSet;

#[cfg(all(target_os = "linux", not(test)))]
const WAYLAND_PORTAL_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

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
    current_modifiers: HashSet<u32>,
}

impl CaptureState {
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn new(mode: ShortcutMode) -> Self {
        Self {
            mode,
            current_modifiers: HashSet::new(),
        }
    }

    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn handle_key_down(&mut self, key: u32) -> anyhow::Result<Option<ShortcutSettings>> {
        let key = normalize_capture_key(key);
        if key == XK_ESCAPE {
            return Err(anyhow!("Shortcut capture cancelled."));
        }

        if is_capture_modifier(key) {
            self.current_modifiers.insert(key);
            return Ok(None);
        }

        let key_kind = supported_key_kind(key)
            .ok_or_else(|| anyhow!("That key is not supported yet. Try a letter, number, Space, arrow key, or function key."))?;

        if self.current_modifiers.is_empty() && key_kind != CaptureKeyKind::Function {
            return Err(anyhow!(
                "Use at least one modifier, like Ctrl, Alt, Shift, or Meta."
            ));
        }

        let mut keys = ordered_modifiers(&self.current_modifiers);
        keys.push(key);
        shortcut_from_linux_key_codes(keys, self.mode.clone()).map(Some)
    }

    #[cfg_attr(any(not(target_os = "linux"), test), allow(dead_code))]
    pub fn handle_key_up(&mut self, key: u32) -> anyhow::Result<Option<ShortcutSettings>> {
        let key = normalize_capture_key(key);
        if !is_capture_modifier(key) {
            return Ok(None);
        }

        self.current_modifiers.remove(&key);

        Ok(None)
    }
}

pub fn capture(target: ShortcutCaptureTarget) -> anyhow::Result<ShortcutSettings> {
    match detect_session() {
        LinuxSession::X11 => platform_capture(target.mode()),
        LinuxSession::Wayland => capture_wayland_layered(target),
        LinuxSession::Unsupported => Err(anyhow!("Shortcut capture needs a desktop session.")),
    }
}

fn capture_wayland_layered(target: ShortcutCaptureTarget) -> anyhow::Result<ShortcutSettings> {
    match platform_capture_wayland(target) {
        Ok(shortcut) => Ok(shortcut),
        Err(portal_error) => {
            if evdev_hotkeys::can_open_any_keyboard() {
                return evdev_hotkeys::capture_shortcut(target.mode());
            }

            Err(anyhow!(
                "{portal_error}\n\nTo change shortcuts on this Wayland desktop, install compositor shortcuts from Parrot or enable kernel-level shortcut capture with:\nsudo usermod -aG input $USER\nThen log out and back in."
            ))
        }
    }
}

pub fn shortcut_from_linux_key_codes(
    mut keys: Vec<u32>,
    mode: ShortcutMode,
) -> anyhow::Result<ShortcutSettings> {
    keys.retain(|key| *key != 0);
    if keys.is_empty() {
        return Err(anyhow!("Shortcut is missing Linux key codes."));
    }

    for key in &mut keys {
        *key = normalize_capture_key(*key);
    }
    keys.sort_by_key(|key| linux_key_sort_key(*key));
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
    if non_modifier_keys.is_empty() {
        return Err(anyhow!(
            "Linux shortcuts need a function key or a modifier plus another key."
        ));
    }

    let key_kind = non_modifier_keys
        .first()
        .and_then(|key| supported_key_kind(*key));
    if key_kind != Some(CaptureKeyKind::Function) && modifier_keys.is_empty() {
        return Err(anyhow!(
            "Use at least one modifier, like Ctrl, Alt, Shift, or Meta."
        ));
    }

    let display_name = display_name_for_keys(&keys);
    let chord = ShortcutChord {
        modifiers: chord_modifiers_for_keys(&modifier_keys),
        key: non_modifier_keys
            .first()
            .and_then(|key| shortcut_key_for_linux_key(*key)),
    };

    Ok(ShortcutSettings {
        display_name,
        mode,
        enabled: true,
        double_tap_toggle: false,
        chord: Some(chord),
        platform_codes: ShortcutPlatformCodes {
            macos_key_codes: None,
            windows_virtual_keys: None,
            linux_key_codes: Some(keys),
        },
    })
}

fn normalize_capture_key(key: u32) -> u32 {
    normalize_configured_key_for_capture(key)
}

fn unsupported_wayland_shortcut_configuration_message(portal_version: u32) -> String {
    format!(
        "This Wayland desktop has GlobalShortcuts portal version {portal_version}. It can run registered shortcuts, but it cannot show Parrot's shortcut picker. Use Parrot's Linux shortcut setup for your compositor, or enable the evdev fallback."
    )
}

fn is_capture_modifier(key: u32) -> bool {
    is_modifier_key(key)
}

fn supported_key_kind(key: u32) -> Option<CaptureKeyKind> {
    if is_capture_modifier(key) {
        return Some(CaptureKeyKind::Modifier);
    }
    match key {
        XK_SPACE | XK_RETURN | XK_TAB | XK_DELETE | XK_LEFT | XK_RIGHT | XK_UP | XK_DOWN => {
            Some(CaptureKeyKind::Other)
        }
        key if is_ascii_alphanumeric_key(key) => Some(CaptureKeyKind::Other),
        XK_F1..=XK_F24 => Some(CaptureKeyKind::Function),
        _ => None,
    }
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn ordered_modifiers(modifiers: &HashSet<u32>) -> Vec<u32> {
    let mut keys = modifiers.iter().copied().collect::<Vec<_>>();
    keys.sort_by_key(|key| linux_key_sort_key(*key));
    keys
}

fn display_name_for_keys(keys: &[u32]) -> String {
    keys.iter()
        .map(|key| linux_key_label(*key))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn chord_modifiers_for_keys(keys: &[u32]) -> Vec<ShortcutModifier> {
    let mut modifiers = Vec::new();
    for key in keys {
        let modifier = match *key {
            XK_CONTROL_L | XK_CONTROL_R => ShortcutModifier::Control,
            XK_ALT_L | XK_ALT_R => ShortcutModifier::Alt,
            XK_SHIFT_L | XK_SHIFT_R => ShortcutModifier::Shift,
            XK_META_L | XK_META_R => ShortcutModifier::Meta,
            _ => continue,
        };
        if !modifiers.contains(&modifier) {
            modifiers.push(modifier);
        }
    }
    modifiers
}

fn shortcut_key_for_linux_key(key: u32) -> Option<ShortcutKey> {
    match key {
        XK_SPACE => Some(ShortcutKey::Space),
        XK_RETURN => Some(ShortcutKey::Return),
        XK_TAB => Some(ShortcutKey::Tab),
        XK_DELETE => Some(ShortcutKey::Delete),
        XK_LEFT => Some(ShortcutKey::ArrowLeft),
        XK_RIGHT => Some(ShortcutKey::ArrowRight),
        XK_UP => Some(ShortcutKey::ArrowUp),
        XK_DOWN => Some(ShortcutKey::ArrowDown),
        key if (XK_F1..=XK_F24).contains(&key) => {
            Some(ShortcutKey::Function((key - XK_F1 + 1) as u8))
        }
        key if is_ascii_alphanumeric_key(key) => {
            Some(ShortcutKey::Character(char::from_u32(key)?.to_string()))
        }
        _ => None,
    }
}

#[cfg(all(target_os = "linux", not(test)))]
fn platform_capture(mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
    platform::capture(mode)
}

#[cfg(all(target_os = "linux", not(test)))]
fn platform_capture_wayland(target: ShortcutCaptureTarget) -> anyhow::Result<ShortcutSettings> {
    platform::capture_wayland(target)
}

#[cfg(any(not(target_os = "linux"), test))]
fn platform_capture(_mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
    Err(anyhow!(
        "Linux shortcut capture requires a real X11 session."
    ))
}

#[cfg(any(not(target_os = "linux"), test))]
fn platform_capture_wayland(_target: ShortcutCaptureTarget) -> anyhow::Result<ShortcutSettings> {
    Err(anyhow!(
        "Linux shortcut capture requires a real Wayland desktop portal."
    ))
}

#[cfg(all(target_os = "linux", not(test)))]
mod platform {
    use super::*;
    use anyhow::Context;
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut};
    use parrot_protocol::{default_linux_hands_free_shortcut, default_linux_push_to_talk_shortcut};
    use x11rb::{
        connection::Connection,
        protocol::{
            xproto::{ConnectionExt, GrabMode, GrabStatus},
            Event,
        },
        rust_connection::RustConnection,
        CURRENT_TIME,
    };

    pub(super) fn capture_wayland(
        target: ShortcutCaptureTarget,
    ) -> anyhow::Result<ShortcutSettings> {
        std::thread::Builder::new()
            .name("Parrot Wayland Shortcut Capture".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;

                runtime.block_on(capture_wayland_async(target))
            })
            .context("failed to start Wayland shortcut capture thread")?
            .join()
            .map_err(|_| anyhow!("Wayland shortcut capture thread panicked."))?
    }

    async fn capture_wayland_async(
        target: ShortcutCaptureTarget,
    ) -> anyhow::Result<ShortcutSettings> {
        let fallback = default_shortcut_for_target(target);
        let push_to_talk = default_linux_push_to_talk_shortcut();
        let hands_free = default_linux_hands_free_shortcut();
        let portal = tokio::time::timeout(WAYLAND_PORTAL_CONNECT_TIMEOUT, GlobalShortcuts::new())
            .await
            .map_err(|_| anyhow!("Timed out connecting to the Wayland global shortcuts portal"))?
            .context("could not connect to the Wayland global shortcuts portal")?;
        let portal_version = portal
            .get_property::<u32>("version")
            .await
            .context("could not read Wayland global shortcuts portal version")?;
        if portal_version < 2 {
            return Err(anyhow!(unsupported_wayland_shortcut_configuration_message(
                portal_version
            )));
        }

        let session = portal.create_session().await?;
        let shortcuts = [
            portal_shortcut(ShortcutCaptureTarget::PushToTalk, &push_to_talk),
            portal_shortcut(ShortcutCaptureTarget::HandsFree, &hands_free),
        ];
        let request = portal.bind_shortcuts(&session, &shortcuts, None).await?;
        let bind_response = request.response()?;

        if let Err(error) = portal.configure_shortcuts(&session, None, None).await {
            return Err(error).context("could not open the Wayland global shortcuts dialog");
        }

        let display_name = match portal
            .list_shortcuts(&session)
            .await
            .and_then(|request| request.response())
        {
            Ok(list_response) => display_name_from_portal_shortcuts(
                list_response.shortcuts(),
                target,
                fallback.display_name.as_str(),
            ),
            Err(_) => display_name_from_portal_shortcuts(
                bind_response.shortcuts(),
                target,
                fallback.display_name.as_str(),
            ),
        };
        let _ = session.close().await;

        Ok(shortcut_with_display_name(fallback, display_name))
    }

    pub(super) fn capture(mode: ShortcutMode) -> anyhow::Result<ShortcutSettings> {
        let (conn, screen_num) = RustConnection::connect(None)?;
        let root = conn.setup().roots[screen_num].root;
        let reply = conn
            .grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?
            .reply()
            .context("failed to grab X11 keyboard for shortcut capture")?;
        if reply.status != GrabStatus::SUCCESS {
            return Err(anyhow!("Could not start X11 shortcut capture."));
        }

        let mapping = KeyboardMapping::load(&conn)?;
        let mut state = CaptureState::new(mode);
        loop {
            match conn.wait_for_event()? {
                Event::KeyPress(event) => {
                    if let Some(keysym) = mapping.keysym_for_keycode(event.detail) {
                        if let Some(shortcut) = state.handle_key_down(keysym)? {
                            let _ = conn.ungrab_keyboard(CURRENT_TIME);
                            return Ok(shortcut);
                        }
                    }
                }
                Event::KeyRelease(event) => {
                    if let Some(keysym) = mapping.keysym_for_keycode(event.detail) {
                        if let Some(shortcut) = state.handle_key_up(keysym)? {
                            let _ = conn.ungrab_keyboard(CURRENT_TIME);
                            return Ok(shortcut);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn default_shortcut_for_target(target: ShortcutCaptureTarget) -> ShortcutSettings {
        match target {
            ShortcutCaptureTarget::PushToTalk => default_linux_push_to_talk_shortcut(),
            ShortcutCaptureTarget::HandsFree => default_linux_hands_free_shortcut(),
        }
    }

    fn shortcut_with_display_name(
        mut shortcut: ShortcutSettings,
        display_name: String,
    ) -> ShortcutSettings {
        shortcut.display_name = display_name;
        shortcut.enabled = true;
        shortcut
    }

    fn display_name_from_portal_shortcuts(
        shortcuts: &[Shortcut],
        target: ShortcutCaptureTarget,
        fallback: &str,
    ) -> String {
        shortcuts
            .iter()
            .find(|shortcut| shortcut.id() == portal_shortcut_id(target))
            .map(|shortcut| shortcut.trigger_description().trim())
            .filter(|trigger| !trigger.is_empty())
            .unwrap_or(fallback)
            .to_string()
    }

    fn portal_shortcut(target: ShortcutCaptureTarget, shortcut: &ShortcutSettings) -> NewShortcut {
        let mut portal_shortcut = NewShortcut::new(
            portal_shortcut_id(target),
            match target {
                ShortcutCaptureTarget::PushToTalk => "Push to talk",
                ShortcutCaptureTarget::HandsFree => "Hands-free dictation",
            },
        );
        if let Some(preferred_trigger) = portal_preferred_trigger(shortcut) {
            portal_shortcut = portal_shortcut.preferred_trigger(Some(preferred_trigger.as_str()));
        }
        portal_shortcut
    }

    fn portal_shortcut_id(target: ShortcutCaptureTarget) -> &'static str {
        match target {
            ShortcutCaptureTarget::PushToTalk => "push-to-talk",
            ShortcutCaptureTarget::HandsFree => "hands-free",
        }
    }

    fn portal_preferred_trigger(shortcut: &ShortcutSettings) -> Option<String> {
        let chord = shortcut.chord.as_ref()?;
        let mut trigger = String::new();
        for modifier in &chord.modifiers {
            let label = match modifier {
                ShortcutModifier::Command | ShortcutModifier::Meta => "Super",
                ShortcutModifier::Control => "Control",
                ShortcutModifier::Option | ShortcutModifier::Alt => "Alt",
                ShortcutModifier::Shift => "Shift",
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

        fn keysym_for_keycode(&self, keycode: u8) -> Option<u32> {
            let index = keycode.checked_sub(self.min_keycode)? as usize;
            self.keysyms
                .chunks(self.keysyms_per_keycode)
                .nth(index)
                .and_then(|chunk| chunk.iter().copied().find(|keysym| *keysym != 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_function_key_capture_without_modifier() {
        let shortcut = shortcut_from_linux_key_codes(vec![XK_F1 + 4], ShortcutMode::Hold)
            .expect("function key shortcut should be valid");

        assert_eq!(shortcut.display_name, "F5");
        assert_eq!(
            shortcut.chord.as_ref().unwrap().key,
            Some(ShortcutKey::Function(5))
        );
        assert_eq!(shortcut.linux_key_codes(), &[XK_F1 + 4]);
    }

    #[test]
    fn builds_modifier_key_chord() {
        let shortcut = shortcut_from_linux_key_codes(
            vec![XK_CONTROL_L, XK_ALT_L, XK_SPACE],
            ShortcutMode::Toggle,
        )
        .unwrap();

        assert_eq!(shortcut.display_name, "Ctrl + Alt + Space");
        assert_eq!(
            shortcut.chord.as_ref().unwrap().modifiers,
            vec![ShortcutModifier::Control, ShortcutModifier::Alt]
        );
        assert_eq!(
            shortcut.chord.as_ref().unwrap().key,
            Some(ShortcutKey::Space)
        );
        assert_eq!(
            shortcut.linux_key_codes(),
            &[XK_CONTROL_L, XK_ALT_L, XK_SPACE]
        );
    }

    #[test]
    fn wayland_unsupported_configuration_message_names_portal_version() {
        let message = unsupported_wayland_shortcut_configuration_message(1);

        assert!(message.contains("version 1"));
        assert!(message.contains("cannot show Parrot's shortcut picker"));
    }

    #[test]
    fn ctrl_space_capture_produces_linux_key_codes() {
        let shortcut =
            shortcut_from_linux_key_codes(vec![XK_CONTROL_L, XK_SPACE], ShortcutMode::Toggle)
                .unwrap();

        assert_eq!(shortcut.display_name, "Ctrl + Space");
        assert_eq!(shortcut.linux_key_codes(), &[XK_CONTROL_L, XK_SPACE]);
    }

    #[test]
    fn rejects_modifier_only_capture() {
        let error =
            shortcut_from_linux_key_codes(vec![XK_CONTROL_L], ShortcutMode::Hold).unwrap_err();

        assert!(error
            .to_string()
            .contains("Linux shortcuts need a function key"));
    }

    #[test]
    fn capture_state_cancels_on_escape() {
        let mut state = CaptureState::new(ShortcutMode::Hold);
        let error = state.handle_key_down(XK_ESCAPE).unwrap_err();

        assert_eq!(error.to_string(), "Shortcut capture cancelled.");
    }

    #[test]
    fn capture_state_saves_modifier_key_chord_on_key_down() {
        let mut state = CaptureState::new(ShortcutMode::Toggle);

        assert!(state.handle_key_down(XK_CONTROL_L).unwrap().is_none());
        assert!(state.handle_key_down(XK_ALT_L).unwrap().is_none());
        let shortcut = state.handle_key_down(XK_SPACE).unwrap().unwrap();

        assert_eq!(shortcut.display_name, "Ctrl + Alt + Space");
        assert_eq!(
            shortcut.linux_key_codes(),
            &[XK_CONTROL_L, XK_ALT_L, XK_SPACE]
        );
    }
}
