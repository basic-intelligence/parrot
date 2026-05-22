use parrot_protocol::ShortcutKey;

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_ALT: u16 = 0x12;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_0: u16 = 0x30;
pub const VK_9: u16 = 0x39;
pub const VK_A: u16 = 0x41;
pub const VK_Z: u16 = 0x5A;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_F1: u16 = 0x70;
pub const VK_F24: u16 = 0x87;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LALT: u16 = 0xA4;
pub const VK_RALT: u16 = 0xA5;

const MODIFIER_ORDER: [u16; 11] = [
    VK_CONTROL,
    VK_LCONTROL,
    VK_RCONTROL,
    VK_ALT,
    VK_LALT,
    VK_RALT,
    VK_SHIFT,
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LWIN,
    VK_RWIN,
];

pub fn is_modifier_virtual_key(vk: u16) -> bool {
    matches!(
        vk,
        VK_LCONTROL | VK_RCONTROL | VK_LALT | VK_RALT | VK_LSHIFT | VK_RSHIFT | VK_LWIN | VK_RWIN
    )
}

pub fn is_generic_modifier(vk: u16) -> bool {
    matches!(vk, VK_CONTROL | VK_ALT | VK_SHIFT)
}

pub fn generic_modifier_for(vk: u16) -> Option<u16> {
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(VK_CONTROL),
        VK_ALT | VK_LALT | VK_RALT => Some(VK_ALT),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some(VK_SHIFT),
        VK_LWIN | VK_RWIN => Some(VK_LWIN),
        _ => None,
    }
}

pub fn virtual_key_sort_key(vk: u16) -> (usize, u16) {
    if let Some(index) = MODIFIER_ORDER.iter().position(|candidate| *candidate == vk) {
        (index, vk)
    } else {
        (MODIFIER_ORDER.len(), vk)
    }
}

pub fn display_label_for_vk(vk: u16) -> String {
    match vk {
        VK_CONTROL => "Ctrl".into(),
        VK_LCONTROL => "Left Ctrl".into(),
        VK_RCONTROL => "Right Ctrl".into(),
        VK_ALT => "Alt".into(),
        VK_LALT => "Left Alt".into(),
        VK_RALT => "Right Alt".into(),
        VK_SHIFT => "Shift".into(),
        VK_LSHIFT => "Left Shift".into(),
        VK_RSHIFT => "Right Shift".into(),
        VK_LWIN => "Windows".into(),
        VK_RWIN => "Right Windows".into(),
        VK_SPACE => "Space".into(),
        VK_RETURN => "Enter".into(),
        VK_TAB => "Tab".into(),
        VK_ESCAPE => "Escape".into(),
        VK_BACK | VK_DELETE => "Delete".into(),
        VK_LEFT => "Left Arrow".into(),
        VK_RIGHT => "Right Arrow".into(),
        VK_UP => "Up Arrow".into(),
        VK_DOWN => "Down Arrow".into(),
        VK_0..=VK_9 => char::from_u32(u32::from(vk)).unwrap_or('?').to_string(),
        VK_A..=VK_Z => char::from_u32(u32::from(vk)).unwrap_or('?').to_string(),
        VK_F1..=VK_F24 => format!("F{}", vk - VK_F1 + 1),
        _ => format!("VK {vk}"),
    }
}

pub fn shortcut_key_for_vk(vk: u16) -> Option<ShortcutKey> {
    match vk {
        VK_SPACE => Some(ShortcutKey::Space),
        VK_RETURN => Some(ShortcutKey::Return),
        VK_TAB => Some(ShortcutKey::Tab),
        VK_ESCAPE => Some(ShortcutKey::Escape),
        VK_BACK | VK_DELETE => Some(ShortcutKey::Delete),
        VK_LEFT => Some(ShortcutKey::ArrowLeft),
        VK_RIGHT => Some(ShortcutKey::ArrowRight),
        VK_UP => Some(ShortcutKey::ArrowUp),
        VK_DOWN => Some(ShortcutKey::ArrowDown),
        VK_0..=VK_9 | VK_A..=VK_Z => Some(ShortcutKey::Character(
            char::from_u32(u32::from(vk))?
                .to_ascii_lowercase()
                .to_string(),
        )),
        VK_F1..=VK_F24 => Some(ShortcutKey::Function((vk - VK_F1 + 1) as u8)),
        _ => None,
    }
}
