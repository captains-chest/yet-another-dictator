//! Reads the text just before the cursor in the focused field through UI Automation, so a
//! dictation can be spaced and capitalized to fit.
//!
//! Best effort: `None` when the app doesn't expose its text, and the controller falls back to what
//! Dictator typed itself. Password fields are never read. The text is used once and never stored
//! or logged.

#[cfg(windows)]
pub use win::UiaPrecedingTextReader;

#[cfg(windows)]
mod win {
    use dictator_core::PrecedingTextReading;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation2, IUIAutomationTextPattern, IUIAutomationTextPattern2,
        IUIAutomationTextRange, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
        TextUnit_Character, UIA_TextPattern2Id, UIA_TextPatternId,
    };
    use windows::core::{BOOL, Interface};

    /// A hung app must not delay typing for long.
    const TIMEOUT_MS: u32 = 100;

    #[derive(Default)]
    pub struct UiaPrecedingTextReader;

    impl PrecedingTextReading for UiaPrecedingTextReader {
        fn text_before_cursor(&mut self, max_length: usize) -> Option<String> {
            // SAFETY: balanced below; S_FALSE (already initialized) also needs the matching call.
            let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let text = read(max_length);
            if initialized {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            text
        }
    }

    fn read(max_length: usize) -> Option<String> {
        let max_length = i32::try_from(max_length).ok()?;
        // SAFETY: COM is initialized on this thread; every call below is a plain UIA query.
        unsafe {
            let automation: IUIAutomation2 =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).ok()?;
            automation.SetConnectionTimeout(TIMEOUT_MS).ok()?;
            automation.SetTransactionTimeout(TIMEOUT_MS).ok()?;
            let element = automation.GetFocusedElement().ok()?;
            if element.CurrentIsPassword().ok()?.as_bool() {
                return None;
            }
            let cursor = caret_range(&element)?;
            let before = cursor.Clone().ok()?;
            before
                .MoveEndpointByRange(
                    TextPatternRangeEndpoint_End,
                    &cursor,
                    TextPatternRangeEndpoint_Start,
                )
                .ok()?;
            before
                .MoveEndpointByUnit(
                    TextPatternRangeEndpoint_Start,
                    TextUnit_Character,
                    -max_length,
                )
                .ok()?;
            Some(before.GetText(max_length).ok()?.to_string())
        }
    }

    /// The caret, or else the start of the selection.
    unsafe fn caret_range(
        element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    ) -> Option<IUIAutomationTextRange> {
        // SAFETY: plain UIA queries on a live element.
        unsafe {
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
            {
                let mut is_active = BOOL::default();
                if let Ok(range) = pattern.GetCaretRange(&mut is_active) {
                    return Some(range);
                }
            }
            let pattern: IUIAutomationTextPattern =
                element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
            let selection = pattern.GetSelection().ok()?;
            if selection.Length().ok()? < 1 {
                return None;
            }
            selection.GetElement(0).ok()?.cast().ok()
        }
    }
}
