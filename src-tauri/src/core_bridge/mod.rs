#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "windows", test))]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod unsupported;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub use unsupported::*;
