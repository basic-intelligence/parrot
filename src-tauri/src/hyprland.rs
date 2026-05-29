use std::{fs, path::PathBuf, process::Command as StdCommand};

#[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
pub const RECORDING_OVERLAY_WIDTH: u32 = 148;
#[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
pub const RECORDING_OVERLAY_HEIGHT: u32 = 36;
#[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
pub const RECORDING_OVERLAY_BOTTOM_MARGIN: i32 = 96;
#[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
pub const RECORDING_OVERLAY_TITLE: &str = "Parrot Recording";
pub const HYPRLAND_SOURCE_LINE: &str = "source = ~/.config/hypr/parrot.conf";

#[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
pub fn hyprctl_command() -> StdCommand {
    let mut command = StdCommand::new("hyprctl");
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
    if let Some(runtime_dir) = runtime_dir.as_ref() {
        command.env("XDG_RUNTIME_DIR", runtime_dir);
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        if let Some(signature) = runtime_dir
            .as_ref()
            .and_then(|runtime_dir| hyprland_instance_signature(PathBuf::from(runtime_dir)))
        {
            command.env("HYPRLAND_INSTANCE_SIGNATURE", signature);
        }
    }
    command
}

#[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
pub fn hyprland_instance_signature(runtime_dir: PathBuf) -> Option<String> {
    let hypr_dir = runtime_dir.join("hypr");
    fs::read_dir(hypr_dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        })
        .and_then(|entry| entry.file_name().into_string().ok())
}
