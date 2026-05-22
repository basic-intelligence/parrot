use anyhow::anyhow;
use serde::Serialize;

#[cfg(target_os = "windows")]
const CONTEXT_LIMIT: usize = 120;
#[cfg(target_os = "windows")]
const CLIPBOARD_RESTORE_DELAY: std::time::Duration = std::time::Duration::from_millis(800);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteTarget {
    pub hwnd: isize,
    pub process_id: u32,
    pub thread_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardBackup {
    Text(String),
    NonTextOrEmpty,
}

impl ClipboardBackup {
    pub fn text_to_restore(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::NonTextOrEmpty => None,
        }
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn capture_current_target() -> Option<PasteTarget> {
    platform_capture_current_target()
}

pub fn paste_text(text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    platform_paste_text(text, target)
}

pub fn format_for_paste(text: &str, preceding_context: Option<&str>) -> String {
    parrot_paste::format_contextual_paste(text, preceding_context)
}

#[cfg(target_os = "windows")]
fn platform_capture_current_target() -> Option<PasteTarget> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut process_id = 0;
        let thread_id = GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 || thread_id == 0 {
            return None;
        }
        Some(PasteTarget {
            hwnd: hwnd as isize,
            process_id,
            thread_id,
        })
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn platform_capture_current_target() -> Option<PasteTarget> {
    None
}

#[cfg(target_os = "windows")]
fn platform_paste_text(text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()> {
    if let Some(target) = target {
        restore_target_window(target)?;
    }

    let context = crate::platform::focused_text::text_before_caret(CONTEXT_LIMIT);
    let formatted = format_for_paste(text, context.as_deref());
    let backup = write_clipboard_text(&formatted)?;
    send_ctrl_v().map_err(|error| {
        anyhow!(
            "{error} If the target app is running as administrator, Windows may block Parrot from pasting into it."
        )
    })?;

    if let Some(previous_text) = backup.text_to_restore().map(str::to_string) {
        std::thread::spawn(move || {
            std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
            let _ = set_clipboard_text(&previous_text);
        });
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn platform_paste_text(_text: &str, _target: Option<&PasteTarget>) -> anyhow::Result<()> {
    Err(anyhow!("Windows paste is unavailable on this platform."))
}

#[cfg(target_os = "windows")]
fn restore_target_window(target: &PasteTarget) -> anyhow::Result<()> {
    use windows_sys::Win32::{
        Foundation::HWND,
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
        },
    };

    unsafe {
        let hwnd = target.hwnd as HWND;
        if hwnd.is_null() {
            return Err(anyhow!("Could not restore the paste target window."));
        }
        if SetForegroundWindow(hwnd) != 0 {
            std::thread::sleep(std::time::Duration::from_millis(80));
            return Ok(());
        }

        let current_thread_id = GetCurrentThreadId();
        let foreground = GetForegroundWindow();
        let mut foreground_process_id = 0;
        let foreground_thread_id = if foreground.is_null() {
            0
        } else {
            GetWindowThreadProcessId(foreground, &mut foreground_process_id)
        };

        let attached_target =
            target.thread_id != 0 && AttachThreadInput(current_thread_id, target.thread_id, 1) != 0;
        let attached_foreground = foreground_thread_id != 0
            && foreground_thread_id != current_thread_id
            && AttachThreadInput(current_thread_id, foreground_thread_id, 1) != 0;

        let restored = SetForegroundWindow(hwnd) != 0;

        if attached_foreground {
            let _ = AttachThreadInput(current_thread_id, foreground_thread_id, 0);
        }
        if attached_target {
            let _ = AttachThreadInput(current_thread_id, target.thread_id, 0);
        }

        if restored {
            std::thread::sleep(std::time::Duration::from_millis(80));
            Ok(())
        } else {
            Err(anyhow!(
                "Windows refused to restore the original paste target window."
            ))
        }
    }
}

#[cfg(target_os = "windows")]
fn write_clipboard_text(text: &str) -> anyhow::Result<ClipboardBackup> {
    let _clipboard = ClipboardGuard::open()?;
    let backup = read_clipboard_text().unwrap_or(ClipboardBackup::NonTextOrEmpty);
    set_clipboard_text_while_open(text)?;
    Ok(backup)
}

#[cfg(target_os = "windows")]
fn set_clipboard_text(text: &str) -> anyhow::Result<()> {
    let _clipboard = ClipboardGuard::open()?;
    set_clipboard_text_while_open(text)
}

#[cfg(target_os = "windows")]
fn read_clipboard_text() -> anyhow::Result<ClipboardBackup> {
    use windows_sys::Win32::System::{
        DataExchange::{CountClipboardFormats, GetClipboardData, IsClipboardFormatAvailable},
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    };

    unsafe {
        if CountClipboardFormats() == 0 || IsClipboardFormatAvailable(CF_UNICODETEXT as u32) == 0 {
            return Ok(ClipboardBackup::NonTextOrEmpty);
        }
        let handle = GetClipboardData(CF_UNICODETEXT as u32);
        if handle.is_null() {
            return Ok(ClipboardBackup::NonTextOrEmpty);
        }
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            return Ok(ClipboardBackup::NonTextOrEmpty);
        }
        let len = GlobalSize(handle) / std::mem::size_of::<u16>();
        let slice = std::slice::from_raw_parts(ptr, len);
        let nul = slice
            .iter()
            .position(|code| *code == 0)
            .unwrap_or(slice.len());
        let text = String::from_utf16_lossy(&slice[..nul]);
        let _ = GlobalUnlock(handle);
        Ok(ClipboardBackup::Text(text))
    }
}

#[cfg(target_os = "windows")]
fn set_clipboard_text_while_open(text: &str) -> anyhow::Result<()> {
    use windows_sys::Win32::System::{
        DataExchange::{EmptyClipboard, SetClipboardData},
        Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        Ole::CF_UNICODETEXT,
    };

    let mut wide = text.encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            return Err(anyhow!("Could not allocate Windows clipboard memory."));
        }
        let ptr = GlobalLock(handle) as *mut u16;
        if ptr.is_null() {
            return Err(anyhow!("Could not lock Windows clipboard memory."));
        }
        ptr.copy_from_nonoverlapping(wide.as_ptr(), wide.len());
        let _ = GlobalUnlock(handle);

        if EmptyClipboard() == 0 {
            return Err(anyhow!("Could not clear the Windows clipboard."));
        }
        if SetClipboardData(CF_UNICODETEXT as u32, handle).is_null() {
            return Err(anyhow!("Could not put text on the Windows clipboard."));
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn send_ctrl_v() -> anyhow::Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
    };

    const VK_V: u16 = 0x56;

    fn input(vk: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let inputs = [
        input(VK_CONTROL, 0),
        input(VK_V, 0),
        input(VK_V, KEYEVENTF_KEYUP),
        input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(anyhow!("Windows blocked the synthetic paste keystroke."));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
struct ClipboardGuard;

#[cfg(target_os = "windows")]
impl ClipboardGuard {
    fn open() -> anyhow::Result<Self> {
        use windows_sys::Win32::System::DataExchange::OpenClipboard;

        if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
            return Err(anyhow!("Could not open the Windows clipboard."));
        }
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::DataExchange::CloseClipboard();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ContextualPasteFixture {
        input: String,
        preceding_context: Option<String>,
        expected: String,
    }

    #[test]
    fn paste_target_serializes_window_process_and_thread() {
        let target = PasteTarget {
            hwnd: 0x1234,
            process_id: 42,
            thread_id: 84,
        };
        let value = serde_json::to_value(target).unwrap();

        assert_eq!(value["hwnd"], 0x1234);
        assert_eq!(value["processId"], 42);
        assert_eq!(value["threadId"], 84);
    }

    #[test]
    fn contextual_paste_uses_shared_formatter() {
        assert_eq!(format_for_paste("world.", Some("Hello")), " world.");
        assert_eq!(format_for_paste("hello.", Some("Done.")), " Hello.");
    }

    #[test]
    fn contextual_paste_matches_shared_fixtures() {
        let fixtures: Vec<ContextualPasteFixture> = serde_json::from_str(include_str!(
            "../../../shared/test-fixtures/contextual-paste.json"
        ))
        .unwrap();

        for fixture in fixtures {
            assert_eq!(
                format_for_paste(&fixture.input, fixture.preceding_context.as_deref()),
                fixture.expected
            );
        }
    }

    #[test]
    fn non_text_clipboard_backup_is_noop_restore() {
        assert_eq!(ClipboardBackup::NonTextOrEmpty.text_to_restore(), None);
        assert_eq!(
            ClipboardBackup::Text("previous".into()).text_to_restore(),
            Some("previous")
        );
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn paste_reports_platform_unavailable_on_non_windows() {
        let error = paste_text("hello", None).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Windows paste is unavailable on this platform."
        );
    }
}
