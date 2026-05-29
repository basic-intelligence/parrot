#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "windows", target_os = "linux", test))]
mod sidecar;

#[cfg(target_os = "linux")]
mod overlay_linux;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(target_os = "linux")
))]
mod unsupported;

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(target_os = "linux")
))]
pub use unsupported::*;
