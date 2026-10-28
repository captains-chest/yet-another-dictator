//! What the app needs from Windows, behind one facade. On other systems (development on a Mac)
//! everything reports that it is unavailable, so the app still builds and its logic can be tested.

#[cfg(not(windows))]
use dictator_core::{PortError, PortResult, StopSignal};
use dictator_core::{PrecedingTextReading, TextInserting};

use crate::hook::{RawInput, Verdict};

/// Keeps the keyboard and mouse hooks installed while alive.
pub struct HookGuard {
    #[cfg(windows)]
    _hook: crate::hook::InputHook,
}

/// Starts watching all keyboard and mouse input. `handler` runs on the hook thread for every event
/// and must return quickly.
pub fn install_hook(
    handler: impl FnMut(RawInput) -> Verdict + Send + 'static,
) -> Result<HookGuard, String> {
    #[cfg(windows)]
    {
        crate::hook::InputHook::install(Box::new(handler))
            .map(|hook| HookGuard { _hook: hook })
            .map_err(|error| format!("Windows refused to watch the keyboard: {error}"))
    }
    #[cfg(not(windows))]
    {
        drop(handler);
        Err(UNSUPPORTED.into())
    }
}

/// The character the key types on the current layout, for keys without a fixed label.
pub fn typed_character(vk: u32) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_CHAR, MapVirtualKeyW};
        // SAFETY: a plain table lookup.
        let value = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) };
        // The high bit marks a dead key; the character is in the low word either way.
        char::from_u32(value & 0xFFFF)
            .filter(|c| !c.is_control())
            .map(String::from)
    }
    #[cfg(not(windows))]
    {
        let _ = vk;
        None
    }
}

/// Brings the app last dictated into back to the front, before typing from the tray menu.
pub fn restore_target() {
    #[cfg(windows)]
    crate::hook::restore_target();
}

pub fn text_inserter() -> Box<dyn TextInserting + Send> {
    #[cfg(windows)]
    {
        Box::new(crate::inserter::SendInputTextInserter)
    }
    #[cfg(not(windows))]
    {
        Box::new(Unsupported)
    }
}

pub fn preceding_text_reader() -> Box<dyn PrecedingTextReading + Send> {
    #[cfg(windows)]
    {
        Box::new(crate::uia::UiaPrecedingTextReader)
    }
    #[cfg(not(windows))]
    {
        Box::new(Unsupported)
    }
}

#[cfg(not(windows))]
const UNSUPPORTED: &str = "Dictator types only on Windows.";

#[cfg(not(windows))]
struct Unsupported;

#[cfg(not(windows))]
impl TextInserting for Unsupported {
    fn preflight(&mut self) -> PortResult<()> {
        Err(PortError::Failed(UNSUPPORTED.into()))
    }
    fn insert(&mut self, _: &str, _: &StopSignal) -> PortResult<()> {
        self.preflight()
    }
    fn delete_backward(&mut self, _: usize, _: &StopSignal) -> PortResult<()> {
        self.preflight()
    }
}

#[cfg(not(windows))]
impl PrecedingTextReading for Unsupported {
    fn text_before_cursor(&mut self, _: usize) -> Option<String> {
        None
    }
}
