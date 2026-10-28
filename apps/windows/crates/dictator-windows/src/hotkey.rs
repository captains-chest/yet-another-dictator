//! Turns raw keyboard and mouse input into dictation actions: the dictate and undo shortcuts, other
//! input (the cursor may have moved), and recording a new shortcut in Settings. Pure, so it runs
//! inside the hook callback and is tested on any machine.

use dictator_core::chord::{Action, ChordMatcher, ChordRecorder, KeyCode, KeyEvent, KeySet};
use dictator_core::shortcut::Shortcut;

use crate::hook::{RawInput, needs_menu_mask};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    Pressed,
    Released,
    Interrupted,
    Undo,
    /// A key or click that isn't a shortcut. Carries no key or position.
    OtherInput,
    /// While recording: the keys held so far, for the Settings keyboard view.
    RecordingHeld(KeySet),
    /// While recording: every key was released; these were pressed.
    Recorded(KeySet),
}

/// The decision for one input event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub actions: Vec<HotkeyAction>,
    /// Withhold the event from other apps.
    pub swallow: bool,
    /// Tap the menu-mask key so releasing Alt or Win doesn't open a menu.
    pub mask_menu: bool,
}

pub struct HotkeyListener {
    dictate: Option<(Shortcut, ChordMatcher)>,
    undo: Option<(Shortcut, ChordMatcher)>,
    recorder: Option<ChordRecorder>,
}

impl HotkeyListener {
    pub fn new(dictate: Option<Shortcut>, undo: Option<Shortcut>) -> Self {
        let pair = |shortcut: Shortcut| {
            let matcher = ChordMatcher::new(&shortcut);
            (shortcut, matcher)
        };
        Self {
            dictate: dictate.map(pair),
            undo: undo.map(pair),
            recorder: None,
        }
    }

    /// While recording, every key is withheld and no shortcut fires.
    pub fn start_recording(&mut self) {
        self.recorder = Some(ChordRecorder::new());
    }

    pub fn stop_recording(&mut self) {
        self.recorder = None;
        // Keys held during recording were never seen by the matchers; start them afresh.
        self.reset();
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    fn reset(&mut self) {
        for (shortcut, matcher) in [&mut self.dictate, &mut self.undo].into_iter().flatten() {
            *matcher = ChordMatcher::new(shortcut);
        }
    }

    pub fn handle(&mut self, input: RawInput) -> Outcome {
        let event = match input {
            RawInput::KeyDown(vk) => KeyEvent::Down(KeyCode(u32::from(vk))),
            RawInput::KeyUp(vk) => KeyEvent::Up(KeyCode(u32::from(vk))),
            RawInput::MouseDown | RawInput::AppSwitched => {
                let actions = if self.recorder.is_some() {
                    vec![]
                } else {
                    vec![HotkeyAction::OtherInput]
                };
                return Outcome {
                    actions,
                    ..Outcome::default()
                };
            }
        };

        if let Some(recorder) = &mut self.recorder {
            let recorded = recorder.apply(&event);
            let held = recorder.held().clone();
            let mut actions = vec![HotkeyAction::RecordingHeld(held.clone())];
            if let Some(keys) = recorded {
                actions.push(HotkeyAction::Recorded(keys));
                self.recorder = None;
                self.reset();
            }
            let mask_menu = matches!(event, KeyEvent::Down(_))
                && needs_menu_mask(held.iter().map(|c| c.0 as u16));
            return Outcome {
                actions,
                swallow: true,
                mask_menu,
            };
        }

        let mut outcome = Outcome::default();
        let mut matched = false;
        if let Some((shortcut, matcher)) = &mut self.dictate {
            let (actions, swallow) = matcher.handle(&event);
            matched |= !actions.is_empty() || swallow;
            outcome.swallow |= swallow;
            for action in actions {
                // A modifier-only shortcut with Alt or Win would open a menu on release.
                if action == Action::Pressed && is_modifier_only(shortcut) {
                    outcome.mask_menu |=
                        needs_menu_mask(shortcut.codes().iter().map(|c| c.0 as u16));
                }
                outcome.actions.push(match action {
                    Action::Pressed => HotkeyAction::Pressed,
                    Action::Released => HotkeyAction::Released,
                    Action::Interrupted => HotkeyAction::Interrupted,
                });
            }
        }
        if let Some((_, matcher)) = &mut self.undo {
            let (actions, swallow) = matcher.handle(&event);
            matched |= !actions.is_empty() || swallow;
            outcome.swallow |= swallow;
            if actions.contains(&Action::Pressed) {
                outcome.actions.push(HotkeyAction::Undo);
            }
        }
        // Modifiers alone don't move the cursor; they may be the start of the undo shortcut.
        if matches!(event, KeyEvent::Down(code) if !code.is_modifier()) && !matched {
            outcome.actions.push(HotkeyAction::OtherInput);
        }
        // A withheld chord key leaves Alt or Win held alone, which would open a menu on release.
        outcome.mask_menu |=
            outcome.swallow && matches!(event, KeyEvent::Down(_)) && self.holds_alt_or_win();
        outcome
    }

    fn holds_alt_or_win(&self) -> bool {
        [&self.dictate, &self.undo]
            .into_iter()
            .flatten()
            .any(|(shortcut, _)| needs_menu_mask(shortcut.codes().iter().map(|c| c.0 as u16)))
    }
}

fn is_modifier_only(shortcut: &Shortcut) -> bool {
    shortcut.codes().iter().all(|code| code.is_modifier())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictator_core::shortcut::Key;

    const D: u16 = 0x44;
    const X: u16 = 0x58;
    const LEFT_CONTROL: u16 = 0xA2;
    const LEFT_ALT: u16 = 0xA4;
    const RIGHT_ALT: u16 = 0xA5;

    fn right_alt() -> Shortcut {
        Shortcut::new(vec![Key::new(KeyCode::RIGHT_ALT, "Right Alt")])
    }

    fn feed(listener: &mut HotkeyListener, inputs: &[RawInput]) -> Vec<Outcome> {
        inputs.iter().map(|&input| listener.handle(input)).collect()
    }

    #[test]
    fn ctrl_alt_d_presses_and_releases_and_withholds_d() {
        let mut listener = HotkeyListener::new(Some(Shortcut::default()), None);
        let outcomes = feed(
            &mut listener,
            &[
                RawInput::KeyDown(LEFT_CONTROL),
                RawInput::KeyDown(LEFT_ALT),
                RawInput::KeyDown(D),
                RawInput::KeyUp(D),
            ],
        );
        assert_eq!(outcomes[2].actions, vec![HotkeyAction::Pressed]);
        assert!(outcomes[2].swallow);
        assert!(outcomes[2].mask_menu, "Alt is held while D is withheld");
        assert_eq!(outcomes[3].actions, vec![HotkeyAction::Released]);
        assert!(outcomes[3].swallow);
    }

    #[test]
    fn modifiers_on_their_own_are_not_other_input_nor_withheld() {
        let mut listener = HotkeyListener::new(Some(Shortcut::default()), None);
        let outcome = listener.handle(RawInput::KeyDown(LEFT_CONTROL));
        assert_eq!(outcome.actions, vec![]);
        assert!(!outcome.swallow);
    }

    #[test]
    fn typing_is_other_input() {
        let mut listener = HotkeyListener::new(Some(Shortcut::default()), None);
        assert_eq!(
            listener.handle(RawInput::KeyDown(X)).actions,
            vec![HotkeyAction::OtherInput]
        );
        assert_eq!(listener.handle(RawInput::KeyUp(X)).actions, vec![]);
        assert_eq!(
            listener.handle(RawInput::MouseDown).actions,
            vec![HotkeyAction::OtherInput]
        );
    }

    #[test]
    fn right_alt_alone_masks_the_menu_and_is_interrupted_by_another_key() {
        let mut listener = HotkeyListener::new(Some(right_alt()), None);
        let pressed = listener.handle(RawInput::KeyDown(RIGHT_ALT));
        assert_eq!(pressed.actions, vec![HotkeyAction::Pressed]);
        assert!(pressed.mask_menu);
        assert!(!pressed.swallow);
        assert_eq!(
            listener.handle(RawInput::KeyDown(X)).actions,
            vec![HotkeyAction::Interrupted]
        );
    }

    #[test]
    fn undo_shortcut_fires() {
        let undo = Shortcut::new(vec![
            Key::new(KeyCode::LEFT_CONTROL, "Ctrl"),
            Key::new(KeyCode(u32::from(X)), "X"),
        ]);
        let mut listener = HotkeyListener::new(Some(Shortcut::default()), Some(undo));
        listener.handle(RawInput::KeyDown(LEFT_CONTROL));
        let outcome = listener.handle(RawInput::KeyDown(X));
        assert_eq!(outcome.actions, vec![HotkeyAction::Undo]);
        assert!(outcome.swallow);
    }

    #[test]
    fn recording_withholds_everything_and_reports_the_chord() {
        let mut listener = HotkeyListener::new(Some(Shortcut::default()), None);
        listener.start_recording();
        let down = listener.handle(RawInput::KeyDown(LEFT_CONTROL));
        assert!(down.swallow);
        listener.handle(RawInput::KeyDown(X));
        listener.handle(RawInput::KeyUp(X));
        let last = listener.handle(RawInput::KeyUp(LEFT_CONTROL));
        let expected: KeySet = [KeyCode::LEFT_CONTROL, KeyCode(u32::from(X))]
            .into_iter()
            .collect();
        assert!(last.actions.contains(&HotkeyAction::Recorded(expected)));
        assert!(!listener.is_recording());
        assert_eq!(
            listener.handle(RawInput::MouseDown).actions,
            vec![HotkeyAction::OtherInput]
        );
    }
}
