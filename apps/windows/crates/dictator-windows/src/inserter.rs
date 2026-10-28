//! Types text into the focused field with SendInput Unicode key events.
//!
//! Deliberately avoids the clipboard: pasting would expose every dictation to clipboard managers
//! and other apps reading it, and would clobber the user's clipboard.
//!
//! Text is sent as small chunks with a short pause between them, as on macOS (issue #43): a long
//! burst lets some apps drop or reorder keys. Line breaks and tabs are sent as the Enter and Tab
//! keys, since many apps ignore them as Unicode characters.

use std::time::Duration;

use dictator_core::text_chunker;

/// What gets sent for one character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keystroke {
    Unicode(u16),
    Enter,
    Tab,
    Backspace,
}

/// UTF-16 units per chunk, as on macOS.
pub const MAX_UTF16_PER_CHUNK: usize = 20;

/// Gap between two chunks: about 4 ms per 20 characters, so a 1,000-character dictation takes a
/// fifth of a second longer to type and every chunk arrives in order.
pub const CHUNK_INTERVAL: Duration = Duration::from_millis(4);

/// Splits `text` into paced chunks of keystrokes, never splitting a grapheme cluster.
pub fn plan(text: &str) -> Vec<Vec<Keystroke>> {
    text_chunker::chunks(text, MAX_UTF16_PER_CHUNK)
        .iter()
        .map(|chunk| {
            let normalized = chunk.replace("\r\n", "\n");
            let mut keys = Vec::new();
            for character in normalized.chars() {
                match character {
                    '\n' | '\r' => keys.push(Keystroke::Enter),
                    '\t' => keys.push(Keystroke::Tab),
                    _ => keys.extend(
                        character
                            .encode_utf16(&mut [0; 2])
                            .iter()
                            .map(|&unit| Keystroke::Unicode(unit)),
                    ),
                }
            }
            keys
        })
        .collect()
}

/// Backspaces in chunks of the same size, so deleting a long dictation is paced too.
pub fn plan_backspaces(count: usize) -> Vec<Vec<Keystroke>> {
    let all = vec![Keystroke::Backspace; count];
    all.chunks(MAX_UTF16_PER_CHUNK)
        .map(<[Keystroke]>::to_vec)
        .collect()
}

#[cfg(windows)]
pub use win::SendInputTextInserter;

#[cfg(windows)]
mod win {
    use dictator_core::{PortError, PortResult, StopSignal, TextInserting};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_RETURN, VK_TAB,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    use super::{CHUNK_INTERVAL, Keystroke, plan, plan_backspaces};
    use crate::hook::OWN_INPUT_TAG;

    #[derive(Default)]
    pub struct SendInputTextInserter;

    impl TextInserting for SendInputTextInserter {
        fn preflight(&mut self) -> PortResult<()> {
            // Windows silently drops input sent to an app running as administrator (UIPI).
            if foreground_is_elevated() && !process_is_elevated(current_process()) {
                return Err(PortError::Failed(
                    "Dictator can't type into apps running as administrator. Switch to another app, \
                     or run that app without administrator rights."
                        .into(),
                ));
            }
            Ok(())
        }

        fn insert(&mut self, text: &str, stop: &StopSignal) -> PortResult<()> {
            self.preflight()?;
            send_paced(&plan(text), stop)
        }

        fn delete_backward(&mut self, count: usize, stop: &StopSignal) -> PortResult<()> {
            self.preflight()?;
            send_paced(&plan_backspaces(count), stop)
        }
    }

    fn send_paced(chunks: &[Vec<Keystroke>], stop: &StopSignal) -> PortResult<()> {
        for (index, chunk) in chunks.iter().enumerate() {
            if index > 0 {
                std::thread::sleep(CHUNK_INTERVAL);
            }
            stop.check()?;
            let inputs: Vec<INPUT> = chunk.iter().flat_map(|&key| key_pair(key)).collect();
            // SAFETY: a valid INPUT slice of the stated element size.
            let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
            if sent as usize != inputs.len() {
                return Err(PortError::Failed(
                    "Windows blocked typing into this app. Click into a text field and try again."
                        .into(),
                ));
            }
        }
        Ok(())
    }

    fn key_pair(key: Keystroke) -> [INPUT; 2] {
        let (vk, scan, flags) = match key {
            Keystroke::Unicode(unit) => (VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE),
            Keystroke::Enter => (VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)),
            Keystroke::Tab => (VK_TAB, 0, KEYBD_EVENT_FLAGS(0)),
            Keystroke::Backspace => (VK_BACK, 0, KEYBD_EVENT_FLAGS(0)),
        };
        let input = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: OWN_INPUT_TAG,
                },
            },
        };
        [input(flags), input(flags | KEYEVENTF_KEYUP)]
    }

    fn current_process() -> HANDLE {
        // SAFETY: a pseudo handle that needs no closing.
        unsafe { GetCurrentProcess() }
    }

    fn foreground_is_elevated() -> bool {
        let mut process_id = 0;
        // SAFETY: plain queries; a null window yields process id 0.
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut process_id)) };
        if process_id == 0 {
            return false;
        }
        // SAFETY: the handle is closed below.
        let Ok(process) =
            (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
        else {
            return false;
        };
        let elevated = process_is_elevated(process);
        // SAFETY: opened above.
        let _ = unsafe { CloseHandle(process) };
        elevated
    }

    /// Unknown counts as not elevated, so a failed check never blocks typing.
    fn process_is_elevated(process: HANDLE) -> bool {
        let mut token = HANDLE::default();
        // SAFETY: the token is closed below.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0;
        // SAFETY: the buffer is a TOKEN_ELEVATION of the stated size.
        let queried = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(std::ptr::from_mut(&mut elevation).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        };
        // SAFETY: opened above.
        let _ = unsafe { CloseHandle(token) };
        queried.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_becomes_unicode_units() {
        assert_eq!(
            plan("hé"),
            vec![vec![
                Keystroke::Unicode(u16::from(b'h')),
                Keystroke::Unicode(0xE9)
            ]]
        );
    }

    #[test]
    fn characters_outside_the_bmp_become_surrogate_pairs() {
        assert_eq!(
            plan("😀"),
            vec![vec![Keystroke::Unicode(0xD83D), Keystroke::Unicode(0xDE00)]]
        );
    }

    #[test]
    fn line_breaks_and_tabs_become_keys() {
        assert_eq!(
            plan("a\r\nb\tc\n"),
            vec![vec![
                Keystroke::Unicode(u16::from(b'a')),
                Keystroke::Enter,
                Keystroke::Unicode(u16::from(b'b')),
                Keystroke::Tab,
                Keystroke::Unicode(u16::from(b'c')),
                Keystroke::Enter,
            ]]
        );
    }

    #[test]
    fn long_text_is_chunked() {
        let chunks = plan(&"x".repeat(45));
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![20, 20, 5]
        );
    }

    #[test]
    fn backspaces_are_chunked() {
        assert_eq!(
            plan_backspaces(25).iter().map(Vec::len).collect::<Vec<_>>(),
            vec![20, 5]
        );
        assert!(plan_backspaces(0).is_empty());
    }
}
