use super::keys::{is_modifier_key, linux_key_sort_key, linux_keys_overlap, normalize_configured_key, XK_ESCAPE};
use anyhow::anyhow;
use parrot_protocol::ShortcutSettings;

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
        .ok_or_else(|| anyhow!("Shortcut is missing Linux key codes."))?;
    if keys.is_empty() {
        return Err(anyhow!("Shortcut is missing Linux key codes."));
    }
    if keys.iter().any(|key| *key == 0) {
        return Err(anyhow!("Shortcut contains an invalid Linux key code."));
    }
    if keys.iter().any(|key| *key == XK_ESCAPE) {
        return Err(anyhow!("Escape is reserved for cancelling dictation."));
    }
    if keys.iter().all(|key| is_modifier_key(*key)) {
        return Err(anyhow!(
            "Linux shortcuts need a function key or a modifier plus another key."
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
            .any(|b_key| linux_keys_overlap(*a_key, *b_key))
    }) && b_keys.iter().all(|b_key| {
        a_keys
            .iter()
            .any(|a_key| linux_keys_overlap(*a_key, *b_key))
    })
}

pub fn shortcut_required_keys(shortcut: &ShortcutSettings) -> Option<Vec<u32>> {
    let mut keys = shortcut
        .linux_key_codes()
        .iter()
        .copied()
        .map(normalize_configured_key)
        .filter(|key| *key != 0)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return None;
    }

    keys.sort_by_key(|key| linux_key_sort_key(*key));
    keys.dedup();
    Some(keys)
}
