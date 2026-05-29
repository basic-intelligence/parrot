#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    Start { source: HotkeySource },
    Stop { source: HotkeySource },
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySource {
    PushToTalk,
    HandsFree,
}

impl HotkeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PushToTalk => "pushToTalk",
            Self::HandsFree => "handsFree",
        }
    }
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Down,
    Up,
}
