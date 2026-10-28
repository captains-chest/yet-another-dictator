//! System-wide keyboard and mouse watching through low-level hooks, so any key or chord can be the
//! shortcut: modifier-only (e.g. Right Alt), bare keys, several keys at once.
//!
//! The hooks run on their own thread with a message loop, never the UI thread: every keystroke on
//! the machine waits for our callback, and Windows silently removes a hook that is too slow. The
//! handler decides synchronously whether a key is withheld from other apps; it should only match
//! the chord and hand actions to another thread.
//!
//! Events are only matched in memory; nothing is stored or logged.

/// What the hook reports. Key codes are Windows virtual-key codes, left and right modifiers
/// distinct (VK_LCONTROL, VK_RMENU, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInput {
    KeyDown(u16),
    KeyUp(u16),
    /// A mouse button went down outside Dictator, so the text cursor may have moved.
    /// Carries no position.
    MouseDown,
}

/// Tags our own SendInput events ("DICT"), so the hook ignores the text we type.
pub const OWN_INPUT_TAG: usize = 0x4449_4354;

/// An unassigned virtual key. Pressing it masks a lone Alt or Win release, which would otherwise
/// open the focused app's menu bar or the Start menu after a withheld chord key.
pub const MENU_MASK_KEY: u16 = 0xE8;

pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;

/// AltGr arrives as a synthetic Left Ctrl with this scan code, then Right Alt. Ignoring the fake
/// Ctrl keeps AltGr (needed to type @, € and more on many layouts) from matching Ctrl+Alt chords.
const ALT_GR_FAKE_CONTROL_SCAN_CODE: u32 = 0x21D;

/// Turns one low-level keyboard event into a [`RawInput`], or `None` when it must be ignored.
pub fn classify_key(vk: u32, scan_code: u32, is_up: bool, extra_info: usize) -> Option<RawInput> {
    if extra_info == OWN_INPUT_TAG {
        return None;
    }
    let vk = u16::try_from(vk).ok()?;
    if vk == VK_LCONTROL && scan_code == ALT_GR_FAKE_CONTROL_SCAN_CODE {
        return None;
    }
    Some(if is_up {
        RawInput::KeyUp(vk)
    } else {
        RawInput::KeyDown(vk)
    })
}

/// Whether releasing these held keys would open a menu, unless masked.
pub fn needs_menu_mask(held: impl IntoIterator<Item = u16>) -> bool {
    held.into_iter()
        .any(|vk| matches!(vk, VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN))
}

/// What the hook does with one event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Withhold the event from other apps (only key events can be withheld).
    pub swallow: bool,
    /// Tap [`MENU_MASK_KEY`] first.
    pub mask_menu: bool,
}

#[cfg(windows)]
pub use win::InputHook;

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::sync::mpsc;
    use std::thread::JoinHandle;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, GetWindowThreadProcessId, HC_ACTION, KBDLLHOOKSTRUCT,
        LLKHF_UP, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        WH_KEYBOARD_LL, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN,
        WM_XBUTTONDOWN, WindowFromPoint,
    };

    use super::{MENU_MASK_KEY, OWN_INPUT_TAG, RawInput, Verdict, classify_key};

    /// Runs on the hook thread for every event; must return quickly.
    pub type Handler = Box<dyn FnMut(RawInput) -> Verdict + Send>;

    thread_local! {
        // Low-level hook procedures run on the thread that installed them.
        static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
    }

    /// The installed hooks. Dropping it removes them and stops the thread.
    pub struct InputHook {
        thread_id: u32,
        thread: Option<JoinHandle<()>>,
    }

    impl InputHook {
        pub fn install(handler: Handler) -> windows::core::Result<Self> {
            let (ready_tx, ready_rx) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("dictator-input-hook".into())
                .spawn(move || run(handler, ready_tx))
                .map_err(|_| windows::core::Error::from_thread())?;
            let thread_id = ready_rx
                .recv()
                .map_err(|_| windows::core::Error::from_thread())??;
            Ok(Self {
                thread_id,
                thread: Some(thread),
            })
        }
    }

    impl Drop for InputHook {
        fn drop(&mut self) {
            // SAFETY: posting to a thread id we own; failure only means it already exited.
            let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn run(handler: Handler, ready: mpsc::Sender<windows::core::Result<u32>>) {
        HANDLER.with_borrow_mut(|slot| *slot = Some(handler));
        // SAFETY: the procedures are valid for the life of the process; LL hooks need no module.
        let hooks = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0).and_then(|keyboard| {
                Ok((
                    keyboard,
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0)?,
                ))
            })
        };
        let (keyboard, mouse) = match hooks {
            Ok(hooks) => hooks,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        // SAFETY: plain thread id query.
        let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
        let mut message = MSG::default();
        // SAFETY: a standard message loop on this thread; WM_QUIT (from Drop) ends it.
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {}
        // SAFETY: the hooks were installed above on this thread.
        unsafe {
            let _ = UnhookWindowsHookEx(keyboard);
            let _ = UnhookWindowsHookEx(mouse);
        }
        HANDLER.with_borrow_mut(|slot| *slot = None);
    }

    fn handle(input: RawInput) -> Verdict {
        let verdict = HANDLER.with_borrow_mut(|slot| {
            slot.as_mut()
                .map(|handler| handler(input))
                .unwrap_or_default()
        });
        if verdict.mask_menu {
            send_menu_mask();
        }
        verdict
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
            let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let is_up = event.flags.0 & LLKHF_UP.0 != 0;
            if let Some(input) =
                classify_key(event.vkCode, event.scanCode, is_up, event.dwExtraInfo)
                && handle(input).swallow
            {
                return LRESULT(1);
            }
        }
        // SAFETY: forwarding the unchanged arguments.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
            let event = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let is_button_down = matches!(
                wparam.0 as u32,
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
            );
            if is_button_down && event.dwExtraInfo != OWN_INPUT_TAG && !is_own_window_at(event) {
                handle(RawInput::MouseDown);
            }
        }
        // SAFETY: forwarding the unchanged arguments.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Clicks in Dictator's own windows don't move the text cursor in the app being dictated into.
    fn is_own_window_at(event: &MSLLHOOKSTRUCT) -> bool {
        let mut process_id = 0;
        // SAFETY: plain queries; a point over no window yields process id 0.
        unsafe {
            GetWindowThreadProcessId(WindowFromPoint(event.pt), Some(&mut process_id));
            process_id == GetCurrentProcessId()
        }
    }

    /// Taps [`MENU_MASK_KEY`], so releasing a held Alt or Win doesn't open a menu.
    fn send_menu_mask() {
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(MENU_MASK_KEY),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: OWN_INPUT_TAG,
                },
            },
        };
        let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
        // SAFETY: a valid INPUT array of the stated size.
        unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_typing_is_ignored() {
        assert_eq!(classify_key(0x44, 0x20, false, OWN_INPUT_TAG), None);
    }

    #[test]
    fn alt_gr_fake_control_is_ignored_but_real_control_is_not() {
        assert_eq!(
            classify_key(
                u32::from(VK_LCONTROL),
                ALT_GR_FAKE_CONTROL_SCAN_CODE,
                false,
                0
            ),
            None
        );
        assert_eq!(
            classify_key(u32::from(VK_LCONTROL), 0x1D, false, 0),
            Some(RawInput::KeyDown(VK_LCONTROL))
        );
    }

    #[test]
    fn up_and_down_are_reported() {
        assert_eq!(
            classify_key(0x44, 0x20, false, 0),
            Some(RawInput::KeyDown(0x44))
        );
        assert_eq!(
            classify_key(0x44, 0x20, true, 0),
            Some(RawInput::KeyUp(0x44))
        );
    }

    #[test]
    fn alt_and_win_need_masking_but_control_alone_does_not() {
        assert!(needs_menu_mask([VK_LCONTROL, VK_LMENU]));
        assert!(needs_menu_mask([VK_RWIN]));
        assert!(!needs_menu_mask([VK_LCONTROL, 0x10]));
    }
}
