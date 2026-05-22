const DEFAULT_MAX_CONTEXT_CHARS: usize = 120;

pub fn text_before_caret(max_characters: usize) -> Option<String> {
    let max_characters = max_characters.min(DEFAULT_MAX_CONTEXT_CHARS);
    if max_characters == 0 {
        return None;
    }
    platform_text_before_caret(max_characters)
}

pub fn bounded_suffix(text: &str, max_characters: usize) -> String {
    if text.chars().count() <= max_characters {
        return text.to_string();
    }

    text.chars()
        .rev()
        .take(max_characters)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(target_os = "windows")]
fn platform_text_before_caret(max_characters: usize) -> Option<String> {
    windows_uia::text_before_caret(max_characters)
        .ok()
        .flatten()
}

#[cfg(not(target_os = "windows"))]
fn platform_text_before_caret(_max_characters: usize) -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
mod windows_uia {
    use super::bounded_suffix;
    use windows::{
        core::BSTR,
        Win32::{
            Foundation::BOOL,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            },
            UI::Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
                IUIAutomationTextPattern2, IUIAutomationTextRange, TextPatternRangeEndpoint_End,
                TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_TextPattern2Id,
                UIA_TextPatternId,
            },
        },
    };

    pub(super) fn text_before_caret(
        max_characters: usize,
    ) -> windows::core::Result<Option<String>> {
        let _com = ComApartment::initialize();
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
        let element = unsafe { automation.GetFocusedElement()? };

        if let Some(text) = text_before_caret_with_text_pattern2(&element, max_characters)? {
            return Ok(Some(text));
        }

        text_before_caret_with_text_pattern(&element, max_characters)
    }

    fn text_before_caret_with_text_pattern2(
        element: &IUIAutomationElement,
        max_characters: usize,
    ) -> windows::core::Result<Option<String>> {
        let pattern: IUIAutomationTextPattern2 =
            unsafe { element.GetCurrentPatternAs(UIA_TextPattern2Id)? };
        let mut active = BOOL(0);
        let caret = unsafe { pattern.GetCaretRange(&mut active)? };
        if !active.as_bool() {
            return Ok(None);
        }
        let document = unsafe { pattern.DocumentRange()? };
        Ok(text_before_range(&document, &caret, max_characters))
    }

    fn text_before_caret_with_text_pattern(
        element: &IUIAutomationElement,
        max_characters: usize,
    ) -> windows::core::Result<Option<String>> {
        let pattern: IUIAutomationTextPattern =
            unsafe { element.GetCurrentPatternAs(UIA_TextPatternId)? };
        let selection = unsafe { pattern.GetSelection()? };
        if unsafe { selection.Length()? } <= 0 {
            return Ok(None);
        }
        let range = unsafe { selection.GetElement(0)? };
        let document = unsafe { pattern.DocumentRange()? };
        Ok(text_before_range(&document, &range, max_characters))
    }

    fn text_before_range(
        document: &IUIAutomationTextRange,
        range: &IUIAutomationTextRange,
        max_characters: usize,
    ) -> Option<String> {
        let before = unsafe { document.Clone().ok()? };
        unsafe {
            before
                .MoveEndpointByRange(
                    TextPatternRangeEndpoint_End,
                    range,
                    TextPatternRangeEndpoint_Start,
                )
                .ok()?;
            let moved = before
                .MoveEndpointByUnit(
                    TextPatternRangeEndpoint_Start,
                    TextUnit_Character,
                    -(max_characters as i32),
                )
                .ok()?;
            if moved == 0 {
                return None;
            }
            let text = bstr_to_string(before.GetText(max_characters as i32).ok()?);
            let text = bounded_suffix(&text, max_characters);
            (!text.is_empty()).then_some(text)
        }
    }

    fn bstr_to_string(value: BSTR) -> String {
        String::from_utf16_lossy(value.as_wide())
    }

    struct ComApartment {
        initialized: bool,
    }

    impl ComApartment {
        fn initialize() -> Self {
            let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
            Self { initialized }
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.initialized {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_suffix_limits_by_characters() {
        assert_eq!(bounded_suffix("abcdef", 3), "def");
        assert_eq!(bounded_suffix("åß∂ƒ", 2), "∂ƒ");
    }

    #[test]
    fn zero_context_limit_returns_none() {
        assert_eq!(text_before_caret(0), None);
    }
}
