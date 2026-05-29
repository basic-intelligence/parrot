use std::collections::HashSet;

pub const XK_SPACE: u32 = 0x20;
pub const XK_TAB: u32 = 0xff09;
pub const XK_RETURN: u32 = 0xff0d;
pub const XK_ESCAPE: u32 = 0xff1b;
pub const XK_LEFT: u32 = 0xff51;
pub const XK_UP: u32 = 0xff52;
pub const XK_RIGHT: u32 = 0xff53;
pub const XK_DOWN: u32 = 0xff54;
pub const XK_SHIFT_L: u32 = 0xffe1;
pub const XK_SHIFT_R: u32 = 0xffe2;
pub const XK_CONTROL_L: u32 = 0xffe3;
pub const XK_CONTROL_R: u32 = 0xffe4;
pub const XK_ALT_L: u32 = 0xffe9;
pub const XK_ALT_R: u32 = 0xffea;
pub const XK_META_L: u32 = 0xffeb;
pub const XK_META_R: u32 = 0xffec;
pub const XK_DELETE: u32 = 0xffff;
pub const XK_F1: u32 = 0xffbe;
pub const XK_F24: u32 = 0xffd5;
pub(super) fn required_keys_active(required_keys: &[u32], active_keys: &HashSet<u32>) -> bool {
    required_keys.iter().all(|required| {
        if active_keys.contains(required) {
            return true;
        }

        match *required {
            XK_CONTROL_L => active_keys.contains(&XK_CONTROL_R),
            XK_ALT_L => active_keys.contains(&XK_ALT_R),
            XK_SHIFT_L => active_keys.contains(&XK_SHIFT_R),
            XK_META_L => active_keys.contains(&XK_META_R),
            _ => false,
        }
    })
}

pub(super) fn active_linux_keys(pressed_actual_keys: &HashSet<u32>) -> HashSet<u32> {
    let mut active = pressed_actual_keys.clone();
    if active.contains(&XK_CONTROL_L) || active.contains(&XK_CONTROL_R) {
        active.insert(XK_CONTROL_L);
    }
    if active.contains(&XK_ALT_L) || active.contains(&XK_ALT_R) {
        active.insert(XK_ALT_L);
    }
    if active.contains(&XK_SHIFT_L) || active.contains(&XK_SHIFT_R) {
        active.insert(XK_SHIFT_L);
    }
    if active.contains(&XK_META_L) || active.contains(&XK_META_R) {
        active.insert(XK_META_L);
    }
    active
}

pub(super) fn normalize_configured_key(key: u32) -> u32 {
    match key {
        XK_CONTROL_R => XK_CONTROL_L,
        XK_ALT_R => XK_ALT_L,
        XK_SHIFT_R => XK_SHIFT_L,
        XK_META_R => XK_META_L,
        key if (b'A' as u32..=b'Z' as u32).contains(&key) => key + 32,
        _ => key,
    }
}

pub(super) fn normalize_observed_key(key: u32) -> u32 {
    normalize_configured_key(key)
}

pub(super) fn linux_keys_overlap(a: u32, b: u32) -> bool {
    normalize_configured_key(a) == normalize_configured_key(b)
}

pub fn is_modifier_key(key: u32) -> bool {
    matches!(
        key,
        XK_CONTROL_L
            | XK_CONTROL_R
            | XK_ALT_L
            | XK_ALT_R
            | XK_SHIFT_L
            | XK_SHIFT_R
            | XK_META_L
            | XK_META_R
    )
}

pub fn linux_key_sort_key(key: u32) -> (u8, u32) {
    let rank = match normalize_configured_key(key) {
        XK_CONTROL_L => 0,
        XK_ALT_L => 1,
        XK_SHIFT_L => 2,
        XK_META_L => 3,
        _ => 4,
    };
    (rank, key)
}

pub fn normalize_configured_key_for_capture(key: u32) -> u32 {
    normalize_configured_key(key)
}

pub fn linux_key_label(key: u32) -> String {
    match normalize_configured_key(key) {
        XK_SPACE => "Space".into(),
        XK_TAB => "Tab".into(),
        XK_RETURN => "Enter".into(),
        XK_LEFT => "Left Arrow".into(),
        XK_UP => "Up Arrow".into(),
        XK_RIGHT => "Right Arrow".into(),
        XK_DOWN => "Down Arrow".into(),
        XK_DELETE => "Delete".into(),
        XK_SHIFT_L => "Shift".into(),
        XK_CONTROL_L => "Ctrl".into(),
        XK_ALT_L => "Alt".into(),
        XK_META_L => "Meta".into(),
        key if (XK_F1..=XK_F24).contains(&key) => format!("F{}", key - XK_F1 + 1),
        key if is_ascii_alphanumeric_key(key) => char::from_u32(key)
            .unwrap_or('?')
            .to_ascii_uppercase()
            .to_string(),
        key => format!("Key {key:#x}"),
    }
}

pub fn is_ascii_alphanumeric_key(key: u32) -> bool {
    (b'0' as u32..=b'9' as u32).contains(&key)
        || (b'a' as u32..=b'z' as u32).contains(&key)
        || (b'A' as u32..=b'Z' as u32).contains(&key)
}
