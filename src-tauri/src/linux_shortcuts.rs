use parrot_settings::{AppSettings, ShortcutKey, ShortcutModifier, ShortcutSettings};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::hyprland::HYPRLAND_SOURCE_LINE;

#[cfg_attr(test, allow(dead_code))]
pub fn install_hyprland_shortcuts(settings: &AppSettings) -> anyhow::Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;

    install_hyprland_shortcuts_in_home(settings, &home)
}

fn install_hyprland_shortcuts_in_home(settings: &AppSettings, home: &Path) -> anyhow::Result<()> {
    let hypr_dir = home.join(".config").join("hypr");
    fs::create_dir_all(&hypr_dir)?;

    let parrot_conf = hypr_dir.join("parrot.conf");
    let hyprland_conf = hypr_dir.join("hyprland.conf");

    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "parrot".to_string());

    let exec = |action: &str| -> String {
        format!(
            "sh -lc 'exec \"{}\" record {}'",
            exe.replace('\'', "'\\''"),
            action
        )
    };

    let hands_free = hyprland_bind_for_shortcut(&settings.hands_free_shortcut, &exec("toggle"))
        .unwrap_or_else(|| format!("bind = CTRL, SPACE, exec, {}", exec("toggle")));

    let push_start = hyprland_bind_for_shortcut(&settings.push_to_talk_shortcut, &exec("start"))
        .unwrap_or_else(|| format!("bind = , F9, exec, {}", exec("start")));

    let push_stop =
        hyprland_bind_release_for_shortcut(&settings.push_to_talk_shortcut, &exec("stop"))
            .unwrap_or_else(|| format!("bindr = , F9, exec, {}", exec("stop")));

    let contents = format!(
        "\
# Managed by Parrot. Re-run Parrot's Linux shortcut setup after changing shortcuts.
# Hands-free dictation: {}
{}
# Push-to-talk dictation: {}
{}
{}

# Optional cancel binding. Avoid binding plain Escape globally.
bind = CTRL ALT, ESCAPE, exec, {}

{}
",
        settings.hands_free_shortcut.display_name,
        hands_free,
        settings.push_to_talk_shortcut.display_name,
        push_start,
        push_stop,
        exec("cancel"),
        hyprland_recording_overlay_rules(),
    );

    fs::write(&parrot_conf, contents)?;

    if hyprland_conf.exists() {
        let current = fs::read_to_string(&hyprland_conf)?;
        if !current.contains(HYPRLAND_SOURCE_LINE) {
            let mut file = fs::OpenOptions::new().append(true).open(&hyprland_conf)?;
            writeln!(
                file,
                "\n# Parrot dictation shortcuts\n{HYPRLAND_SOURCE_LINE}"
            )?;
        }
    }

    let _ = std::process::Command::new("hyprctl").arg("reload").status();

    Ok(())
}

fn hyprland_recording_overlay_rules() -> &'static str {
    r#"
# Parrot recording overlay.
# Keep the small recording pill floating at the bottom center.
windowrule = float on, match:title ^Parrot Recording$
windowrule = pin on, match:title ^Parrot Recording$
windowrule = no_initial_focus on, match:title ^Parrot Recording$
windowrule = no_focus on, match:title ^Parrot Recording$
windowrule = border_size 0, match:title ^Parrot Recording$
windowrule = no_shadow on, match:title ^Parrot Recording$
windowrule = no_anim on, match:title ^Parrot Recording$
windowrule = size 148 36, match:title ^Parrot Recording$
windowrule = move 50%-74 100%-132, match:title ^Parrot Recording$
"#
}

fn hyprland_bind_for_shortcut(shortcut: &ShortcutSettings, command: &str) -> Option<String> {
    let chord = shortcut.chord.as_ref()?;
    let key = hyprland_key_name(chord.key.as_ref()?)?;
    let mods = hyprland_modifiers(&chord.modifiers);
    Some(format!("bind = {mods}, {key}, exec, {command}"))
}

fn hyprland_bind_release_for_shortcut(
    shortcut: &ShortcutSettings,
    command: &str,
) -> Option<String> {
    let chord = shortcut.chord.as_ref()?;
    let key = hyprland_key_name(chord.key.as_ref()?)?;
    let mods = hyprland_modifiers(&chord.modifiers);
    Some(format!("bindr = {mods}, {key}, exec, {command}"))
}

fn hyprland_modifiers(modifiers: &[ShortcutModifier]) -> String {
    modifiers
        .iter()
        .filter_map(|modifier| match modifier {
            ShortcutModifier::Control => Some("CTRL"),
            ShortcutModifier::Alt | ShortcutModifier::Option => Some("ALT"),
            ShortcutModifier::Shift => Some("SHIFT"),
            ShortcutModifier::Meta | ShortcutModifier::Command => Some("SUPER"),
            ShortcutModifier::Fn => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn hyprland_key_name(key: &ShortcutKey) -> Option<String> {
    match key {
        ShortcutKey::Space => Some("SPACE".into()),
        ShortcutKey::Return => Some("RETURN".into()),
        ShortcutKey::Tab => Some("TAB".into()),
        ShortcutKey::Delete => Some("DELETE".into()),
        ShortcutKey::Escape => Some("ESCAPE".into()),
        ShortcutKey::ArrowLeft => Some("LEFT".into()),
        ShortcutKey::ArrowRight => Some("RIGHT".into()),
        ShortcutKey::ArrowUp => Some("UP".into()),
        ShortcutKey::ArrowDown => Some("DOWN".into()),
        ShortcutKey::Function(number) => Some(format!("F{number}")),
        ShortcutKey::Character(value) => value
            .chars()
            .next()
            .map(|value| value.to_ascii_uppercase().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_settings::{
        default_settings_for_platform, SettingsPlatform, ShortcutChord, ShortcutMode,
        ShortcutPlatformCodes,
    };

    #[test]
    fn hyprland_bind_uses_ctrl_space() {
        let settings = default_settings_for_platform(SettingsPlatform::Linux);

        assert_eq!(
            hyprland_bind_for_shortcut(&settings.hands_free_shortcut, "parrot record toggle"),
            Some("bind = CTRL, SPACE, exec, parrot record toggle".into())
        );
    }

    #[test]
    fn install_writes_source_line_when_hyprland_config_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let hypr_dir = temp_dir.path().join(".config").join("hypr");
        fs::create_dir_all(&hypr_dir).unwrap();
        fs::write(hypr_dir.join("hyprland.conf"), "# test config\n").unwrap();
        let settings = default_settings_for_platform(SettingsPlatform::Linux);

        install_hyprland_shortcuts_in_home(&settings, temp_dir.path()).unwrap();

        let hyprland_conf = fs::read_to_string(hypr_dir.join("hyprland.conf")).unwrap();
        assert!(hyprland_conf.contains(HYPRLAND_SOURCE_LINE));
        let parrot_conf = fs::read_to_string(hypr_dir.join("parrot.conf")).unwrap();
        assert!(parrot_conf.contains("Hands-free dictation: Ctrl + Space"));
        assert!(parrot_conf.contains("bind = CTRL, SPACE, exec,"));
        assert!(parrot_conf.contains("bind = , F9, exec,"));
        assert!(parrot_conf.contains("bindr = , F9, exec,"));
        assert!(parrot_conf.contains("windowrule = border_size 0, match:title ^Parrot Recording$"));
        assert!(parrot_conf
            .contains("windowrule = move 50%-74 100%-132, match:title ^Parrot Recording$"));
    }

    #[test]
    fn function_key_shortcut_has_no_modifiers() {
        let shortcut = ShortcutSettings {
            display_name: "F9".into(),
            mode: ShortcutMode::Hold,
            enabled: true,
            double_tap_toggle: false,
            chord: Some(ShortcutChord {
                modifiers: vec![],
                key: Some(ShortcutKey::Function(9)),
            }),
            platform_codes: ShortcutPlatformCodes {
                macos_key_codes: None,
                windows_virtual_keys: None,
                linux_key_codes: Some(vec![0xffc6]),
            },
        };

        assert_eq!(
            hyprland_bind_release_for_shortcut(&shortcut, "parrot record stop"),
            Some("bindr = , F9, exec, parrot record stop".into())
        );
    }
}
