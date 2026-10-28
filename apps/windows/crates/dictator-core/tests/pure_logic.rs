//! Port of PureLogicTests.swift. Shortcut, key-label and chord tests use Windows virtual-key codes.

use dictator_core::audio_ring_buffer::AudioRingBuffer;
use dictator_core::chord::{Action, ChordMatcher, ChordRecorder, KeyCode, KeyEvent, KeySet};
use dictator_core::dictation_history::DictationHistory;
use dictator_core::dictation_language::DictationLanguage;
use dictator_core::shortcut::{Key, PassThroughEffect, Shortcut, ValidationError};
use dictator_core::{
    dictation_prompt, key_labels, smart_join, speech_activity, text_chunker, transcript_cleaner,
};

mod transcript_cleaner_tests {
    use super::*;

    #[test]
    fn cleans() {
        let cases = [
            ("  Hello world. ", "Hello world."),
            ("[BLANK_AUDIO]", ""),
            ("Hi [Music] there", "Hi there"),
            ("<|en|><|transcribe|> Hej med dig", "Hej med dig"),
            ("line one\nline two\tend", "line one line two end"),
            ("bell\u{07}escape\u{1B}", "bellescape"),
            ("👩‍💻 works", "👩‍💻 works"),
        ];
        for (raw, expected) in cases {
            assert_eq!(transcript_cleaner::clean(raw), expected, "{raw:?}");
        }
    }
}

mod text_chunker_tests {
    use super::*;

    #[test]
    fn splits_at_limit() {
        let chunks = text_chunker::chunks(&"a".repeat(45), text_chunker::MAX_UTF16_PER_EVENT);
        assert_eq!(
            chunks.iter().map(String::len).collect::<Vec<_>>(),
            [20, 20, 5]
        );
    }

    #[test]
    fn never_splits_grapheme_clusters() {
        let family = "👩‍👩‍👧‍👦"; // 11 UTF-16 units
        let text = family.repeat(5);
        let chunks = text_chunker::chunks(&text, text_chunker::MAX_UTF16_PER_EVENT);
        assert_eq!(chunks.concat(), text);
        assert!(
            chunks
                .iter()
                .all(|c| c.encode_utf16().count() <= text_chunker::MAX_UTF16_PER_EVENT)
        );
        assert!(chunks.iter().all(|c| c == family));
    }

    #[test]
    fn empty_text_has_no_chunks() {
        assert!(text_chunker::chunks("", text_chunker::MAX_UTF16_PER_EVENT).is_empty());
    }
}

mod speech_activity_tests {
    use super::*;
    use std::f32::consts::PI;

    const RATE: f64 = 16_000.0;

    fn tone(amplitude: f32, seconds: f64) -> Vec<f32> {
        (0..(seconds * RATE) as usize)
            .map(|i| amplitude * (i as f32 * 2.0 * PI * 220.0 / 16_000.0).sin())
            .collect()
    }

    fn silence(seconds: f64, noise: f32) -> Vec<f32> {
        let mut seed: u32 = 1;
        (0..(seconds * RATE) as usize)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); // deterministic noise
                noise * ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn digital_silence_has_no_speech() {
        assert!(!speech_activity::contains_speech(&silence(2.0, 0.0)));
    }

    #[test]
    fn quiet_room_noise_has_no_speech() {
        // Uniform noise at ±0.003 ≈ −55 dBFS RMS.
        assert!(!speech_activity::contains_speech(&silence(3.0, 0.003)));
    }

    #[test]
    fn a_key_click_is_not_speech() {
        let click = [silence(1.0, 0.002), tone(0.5, 0.05), silence(0.25, 0.002)].concat();
        assert!(!speech_activity::contains_speech(&click));
    }

    #[test]
    fn normal_speech_level_counts() {
        assert!(speech_activity::contains_speech(
            &[silence(0.5, 0.0), tone(0.05, 0.4)].concat()
        ));
    }

    #[test]
    fn quiet_speech_level_counts() {
        // RMS ≈ 0.0085 (≈ −41 dBFS) for 150 ms, like a short quiet word.
        assert!(speech_activity::contains_speech(&tone(0.012, 0.15)));
    }

    #[test]
    fn too_short_for_a_frame_has_no_speech() {
        assert!(!speech_activity::contains_speech(&tone(0.5, 0.01)));
    }
}

mod dictation_prompt_tests {
    use super::*;

    #[test]
    fn prompt_is_punctuated_in_both_languages() {
        let prompt = dictation_prompt::TEXT;
        assert!(prompt.contains('?') && prompt.contains('.'));
        assert!(prompt.contains("tak") && prompt.contains("thanks"));
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn detects_prompt_echo() {
        for transcript in [
            dictation_prompt::TEXT,
            "Hej, hvordan går det? Det går godt, tak. Hello, how are you?",
            "hello how are you i'm fine thanks",
            " Det går godt, tak. Hello, how are you? I'm fine, thanks. ",
        ] {
            assert!(dictation_prompt::is_echo(transcript), "{transcript:?}");
        }
    }

    #[test]
    fn keeps_real_dictation() {
        for transcript in [
            "",
            "Tak.",
            "Hello, how are you?",
            "Det går godt, tak.",
            "Hello, how are you? I'm fine, thanks. And you?",
            "Please send the report by Friday.",
            "Hej, hvordan går det med projektet i dag? Det går godt nu.",
        ] {
            assert!(!dictation_prompt::is_echo(transcript), "{transcript:?}");
        }
    }
}

mod dictation_language_tests {
    use super::*;

    #[test]
    fn maps_to_whisper_codes() {
        assert_eq!(DictationLanguage::Automatic.whisper_code(), None);
        assert_eq!(DictationLanguage::English.whisper_code(), Some("en"));
    }

    #[test]
    fn unknown_stored_value_falls_back_to_automatic() {
        assert_eq!(
            DictationLanguage::from_stored(None),
            DictationLanguage::Automatic
        );
        assert_eq!(
            DictationLanguage::from_stored(Some("klingon")),
            DictationLanguage::Automatic
        );
        assert_eq!(
            DictationLanguage::from_stored(Some("danish")),
            DictationLanguage::Automatic
        );
        assert_eq!(
            DictationLanguage::from_stored(Some("english")),
            DictationLanguage::English
        );
    }
}

const VK_A: KeyCode = KeyCode(0x41);
const VK_C: KeyCode = KeyCode(0x43);
const VK_D: KeyCode = KeyCode(0x44);
const VK_S: KeyCode = KeyCode(0x53);
const VK_SPACE: KeyCode = KeyCode(0x20);
const VK_ESCAPE: KeyCode = KeyCode(0x1B);
const VK_UP: KeyCode = KeyCode(0x26);
const VK_F1: KeyCode = KeyCode(0x70);
const VK_F5: KeyCode = KeyCode(0x74);
const VK_F12: KeyCode = KeyCode(0x7B);

fn shortcut(codes: &[KeyCode]) -> Shortcut {
    Shortcut::new(codes.iter().map(|&c| Key::new(c, "")).collect())
}

mod shortcut_tests {
    use super::*;

    #[test]
    fn default_is_valid_and_readable() {
        assert_eq!(Shortcut::default().validate(), Ok(()));
        assert_eq!(Shortcut::default().display_string(), "Ctrl+Alt+D");
    }

    #[test]
    fn display_uses_windows_modifier_order() {
        let shortcut = Shortcut::new(vec![
            Key::new(VK_SPACE, "Space"),
            Key::new(KeyCode::RIGHT_WIN, "Win"),
            Key::new(KeyCode::LEFT_SHIFT, "Shift"),
            Key::new(KeyCode::LEFT_ALT, "Alt"),
            Key::new(KeyCode::LEFT_CONTROL, "Ctrl"),
        ]);
        assert_eq!(shortcut.display_string(), "Ctrl+Alt+Shift+Win+Space");
    }

    #[test]
    fn modifier_only_shortcuts_name_the_side() {
        assert_eq!(
            shortcut(&[KeyCode::RIGHT_ALT]).display_string(),
            "Right Alt"
        );
        assert_eq!(
            shortcut(&[KeyCode::LEFT_CONTROL, KeyCode::LEFT_SHIFT]).display_string(),
            "Left Ctrl + Left Shift"
        );
    }

    #[test]
    fn multiple_regular_keys_are_joined() {
        let shortcut = Shortcut::new(vec![
            Key::new(VK_S, "S"),
            Key::new(VK_A, "A"),
            Key::new(KeyCode::LEFT_WIN, "Win"),
        ]);
        assert_eq!(shortcut.display_string(), "Win+A+S");
    }

    #[test]
    fn pass_through_effect() {
        use PassThroughEffect::*;
        let cases: &[(&[KeyCode], PassThroughEffect)] = &[
            (&[KeyCode::RIGHT_ALT], Harmless),
            (&[KeyCode::CAPS_LOCK], Harmless),
            (&[VK_F5], Harmless),
            (&[KeyCode::LEFT_SHIFT, VK_F12], Harmless),
            (
                &[KeyCode::LEFT_CONTROL, KeyCode::LEFT_ALT, VK_D],
                AppShortcut,
            ), // the default
            (&[KeyCode::RIGHT_WIN, VK_SPACE], AppShortcut),
            (&[VK_A], Typing),
            (&[VK_SPACE], Typing),
            (&[VK_ESCAPE], Typing),
            (&[KeyCode::LEFT_ALT, VK_D], Typing),
            (&[KeyCode::LEFT_SHIFT, VK_UP], Typing), // selects text
            (&[VK_A, VK_S], Typing),
        ];
        for (codes, expected) in cases {
            assert_eq!(
                shortcut(codes).pass_through_effect(),
                *expected,
                "{codes:?}"
            );
        }
    }

    #[test]
    fn default_shortcut_does_not_type() {
        assert_ne!(
            Shortcut::default().pass_through_effect(),
            PassThroughEffect::Typing
        );
    }

    #[test]
    fn any_single_key_is_valid() {
        assert_eq!(
            Shortcut::new(vec![Key::new(VK_F5, "F5")]).validate(),
            Ok(())
        );
        assert_eq!(
            Shortcut::new(vec![Key::new(KeyCode::CAPS_LOCK, "Caps Lock")]).validate(),
            Ok(())
        );
    }

    #[test]
    fn empty_is_invalid() {
        assert_eq!(
            Shortcut::new(vec![]).validate(),
            Err(ValidationError::Empty)
        );
    }

    #[test]
    fn round_trips_through_its_keys() {
        let default = Shortcut::default();
        assert_eq!(Shortcut::new(default.keys().to_vec()), default);
    }

    #[test]
    fn drops_repeated_keys() {
        assert_eq!(shortcut(&[VK_A, VK_A]).keys().len(), 1);
    }
}

mod key_labels_tests {
    use super::*;

    #[test]
    fn special_keys_have_fixed_labels() {
        assert_eq!(key_labels::label(VK_SPACE, Some(" ")), "Space");
        assert_eq!(key_labels::label(VK_F1, None), "F1");
        assert_eq!(key_labels::label(KeyCode(0x87), None), "F24");
        assert_eq!(key_labels::label(KeyCode::RIGHT_WIN, None), "Win");
        assert_eq!(key_labels::label(KeyCode(0x35), None), "5");
        assert_eq!(key_labels::label(KeyCode(0x65), None), "Num 5");
        assert_eq!(key_labels::sided_name(KeyCode::RIGHT_ALT), "Right Alt");
        assert_eq!(key_labels::sided_name(KeyCode::LEFT_CONTROL), "Left Ctrl");
    }

    #[test]
    fn regular_keys_use_typed_character() {
        assert_eq!(key_labels::label(VK_D, Some("d")), "D");
        assert_eq!(key_labels::label(KeyCode(0xBA), Some("ø")), "Ø"); // OEM key, layout dependent
        assert_eq!(key_labels::label(KeyCode(0x99), None), "Key 153");
    }
}

fn set(codes: &[KeyCode]) -> KeySet {
    codes.iter().copied().collect()
}

mod chord_matcher_tests {
    use super::*;
    use KeyEvent::{Down, FlagsChanged, Up};

    fn matcher(codes: &[KeyCode]) -> ChordMatcher {
        ChordMatcher::new(&shortcut(codes))
    }

    #[test]
    fn modifiers_plus_key_press_and_release() {
        let mut m = matcher(&[KeyCode::LEFT_CONTROL, KeyCode::LEFT_ALT, VK_D]);
        assert_eq!(
            m.handle(&FlagsChanged(
                KeyCode::LEFT_CONTROL,
                set(&[KeyCode::LEFT_CONTROL])
            ))
            .0,
            []
        );
        let both = set(&[KeyCode::LEFT_CONTROL, KeyCode::LEFT_ALT]);
        assert_eq!(m.handle(&FlagsChanged(KeyCode::LEFT_ALT, both)).0, []);
        assert_eq!(m.handle(&Down(VK_D)), (vec![Action::Pressed], true));
        assert_eq!(m.handle(&Down(VK_D)), (vec![], true)); // key repeat
        assert_eq!(m.handle(&Up(VK_D)), (vec![Action::Released], true));
    }

    #[test]
    fn modifiers_as_plain_key_events_work_too() {
        let mut m = matcher(&[KeyCode::LEFT_CONTROL, KeyCode::LEFT_ALT, VK_D]);
        assert_eq!(m.handle(&Down(KeyCode::LEFT_CONTROL)), (vec![], false));
        assert_eq!(m.handle(&Down(KeyCode::RIGHT_ALT)), (vec![], false));
        assert_eq!(m.handle(&Down(VK_D)), (vec![Action::Pressed], true));
        assert_eq!(
            m.handle(&Up(KeyCode::RIGHT_ALT)),
            (vec![Action::Released], false)
        );
        assert_eq!(m.handle(&Up(VK_D)), (vec![], true));
    }

    #[test]
    fn modifier_side_does_not_matter_with_regular_key() {
        let mut m = matcher(&[KeyCode::LEFT_CONTROL, VK_D]);
        m.handle(&FlagsChanged(
            KeyCode::RIGHT_CONTROL,
            set(&[KeyCode::RIGHT_CONTROL]),
        ));
        assert_eq!(m.handle(&Down(VK_D)).0, [Action::Pressed]);
    }

    #[test]
    fn extra_modifier_does_not_match_and_key_passes_through() {
        let mut m = matcher(&[KeyCode::LEFT_CONTROL, VK_D]);
        m.handle(&FlagsChanged(
            KeyCode::LEFT_CONTROL,
            set(&[KeyCode::LEFT_CONTROL]),
        ));
        m.handle(&FlagsChanged(
            KeyCode::LEFT_WIN,
            set(&[KeyCode::LEFT_CONTROL, KeyCode::LEFT_WIN]),
        ));
        assert_eq!(m.handle(&Down(VK_D)), (vec![], false));
        assert_eq!(m.handle(&Up(VK_D)), (vec![], false));
    }

    #[test]
    fn releasing_modifier_first_still_swallows_key_up() {
        let mut m = matcher(&[KeyCode::LEFT_CONTROL, VK_D]);
        m.handle(&FlagsChanged(
            KeyCode::LEFT_CONTROL,
            set(&[KeyCode::LEFT_CONTROL]),
        ));
        m.handle(&Down(VK_D));
        assert_eq!(
            m.handle(&FlagsChanged(KeyCode::LEFT_CONTROL, set(&[]))),
            (vec![Action::Released], false)
        );
        assert_eq!(m.handle(&Up(VK_D)), (vec![], true));
    }

    #[test]
    fn bare_key_works() {
        let mut m = matcher(&[VK_F5]);
        assert_eq!(m.handle(&Down(VK_F5)), (vec![Action::Pressed], true));
        assert_eq!(m.handle(&Up(VK_F5)), (vec![Action::Released], true));
        assert_eq!(m.handle(&Down(VK_A)), (vec![], false));
    }

    #[test]
    fn multi_key_chord() {
        let mut m = matcher(&[VK_A, VK_S]);
        assert_eq!(m.handle(&Down(VK_A)), (vec![], false));
        assert_eq!(m.handle(&Down(VK_S)), (vec![Action::Pressed], true));
        assert_eq!(m.handle(&Up(VK_A)).0, [Action::Released]);
    }

    #[test]
    fn modifier_only_is_side_specific() {
        let mut m = matcher(&[KeyCode::RIGHT_ALT]);
        assert_eq!(
            m.handle(&FlagsChanged(KeyCode::LEFT_ALT, set(&[KeyCode::LEFT_ALT])))
                .0,
            []
        );
        m.handle(&FlagsChanged(KeyCode::LEFT_ALT, set(&[])));
        assert_eq!(
            m.handle(&FlagsChanged(
                KeyCode::RIGHT_ALT,
                set(&[KeyCode::RIGHT_ALT])
            ))
            .0,
            [Action::Pressed]
        );
        assert_eq!(
            m.handle(&FlagsChanged(KeyCode::RIGHT_ALT, set(&[]))).0,
            [Action::Released]
        );
    }

    #[test]
    fn modifier_only_is_interrupted_by_another_key() {
        let mut m = matcher(&[KeyCode::LEFT_WIN]);
        assert_eq!(
            m.handle(&FlagsChanged(KeyCode::LEFT_WIN, set(&[KeyCode::LEFT_WIN])))
                .0,
            [Action::Pressed]
        );
        assert_eq!(m.handle(&Down(VK_C)), (vec![Action::Interrupted], false));
    }

    #[test]
    fn caps_lock_is_a_normal_key_on_windows() {
        let mut m = matcher(&[KeyCode::CAPS_LOCK]);
        assert_eq!(
            m.handle(&Down(KeyCode::CAPS_LOCK)),
            (vec![Action::Pressed], true)
        );
        assert_eq!(
            m.handle(&Up(KeyCode::CAPS_LOCK)),
            (vec![Action::Released], true)
        );
    }

    #[test]
    fn flags_recover_from_missed_modifier_release() {
        let mut m = matcher(&[KeyCode::LEFT_CONTROL, VK_D]);
        m.handle(&FlagsChanged(KeyCode::LEFT_WIN, set(&[KeyCode::LEFT_WIN]))); // Win release missed
        m.handle(&FlagsChanged(
            KeyCode::LEFT_CONTROL,
            set(&[KeyCode::LEFT_CONTROL]),
        ));
        assert_eq!(m.handle(&Down(VK_D)).0, [Action::Pressed]);
    }
}

mod chord_recorder_tests {
    use super::*;
    use KeyEvent::{Down, FlagsChanged, Up};

    #[test]
    fn records_everything_held_until_all_released() {
        let mut r = ChordRecorder::new();
        assert_eq!(
            r.apply(&FlagsChanged(
                KeyCode::LEFT_CONTROL,
                set(&[KeyCode::LEFT_CONTROL])
            )),
            None
        );
        assert_eq!(r.apply(&Down(VK_D)), None);
        assert_eq!(*r.held(), set(&[KeyCode::LEFT_CONTROL, VK_D]));
        assert_eq!(r.apply(&Up(VK_D)), None);
        assert_eq!(
            r.apply(&FlagsChanged(KeyCode::LEFT_CONTROL, set(&[]))),
            Some(set(&[KeyCode::LEFT_CONTROL, VK_D]))
        );
        assert!(r.held().is_empty());
    }

    #[test]
    fn records_caps_lock() {
        let mut r = ChordRecorder::new();
        assert_eq!(r.apply(&Down(KeyCode::CAPS_LOCK)), None);
        assert_eq!(
            r.apply(&Up(KeyCode::CAPS_LOCK)),
            Some(set(&[KeyCode::CAPS_LOCK]))
        );
    }
}

mod smart_join_tests {
    use super::*;

    fn adjust(text: &str, after: Option<&str>) -> String {
        smart_join::adjust(text, after)
    }

    #[test]
    fn unknown_context_leaves_text_alone() {
        assert_eq!(adjust("Hello there.", None), "Hello there.");
    }

    #[test]
    fn empty_field_capitalizes_without_space() {
        assert_eq!(adjust("hello there.", Some("")), "Hello there.");
    }

    #[test]
    fn new_sentence_after_full_stop_gets_space_and_capital() {
        assert_eq!(adjust("and then.", Some("It works.")), " And then.");
        assert_eq!(adjust("Really?", Some("Done!")), " Really?");
        assert_eq!(adjust("Yes.", Some("He said \"no.\"")), " Yes.");
    }

    #[test]
    fn mid_sentence_lowercases_sentence_capital() {
        assert_eq!(adjust("How are you?", Some("Hello,")), " how are you?");
        assert_eq!(
            adjust("Og så videre.", Some("Vi tog toget")),
            " og så videre."
        );
    }

    #[test]
    fn mid_sentence_keeps_intentional_capitals() {
        assert_eq!(adjust("I think so.", Some("Well,")), " I think so.");
        assert_eq!(adjust("I'm here.", Some("Well,")), " I'm here.");
        assert_eq!(adjust("NASA said so.", Some("and")), " NASA said so.");
        assert_eq!(adjust("iPhone works.", Some("my")), " iPhone works.");
    }

    #[test]
    fn no_space_after_existing_whitespace_or_opener() {
        assert_eq!(adjust("next one.", Some("First one. ")), "Next one.");
        assert_eq!(adjust("Like this", Some("(")), "like this");
    }

    #[test]
    fn new_line_starts_a_sentence() {
        assert_eq!(
            adjust("second line.", Some("First line.\n")),
            "Second line."
        );
        assert_eq!(adjust("second line.", Some("First line\n")), "Second line.");
    }

    #[test]
    fn punctuation_attaches_to_previous_word() {
        assert_eq!(adjust(", and more.", Some("This")), ", and more.");
        assert_eq!(adjust("?", Some("Really")), "?");
    }

    #[test]
    fn spaces_before_cursor_still_see_the_sentence_end() {
        assert_eq!(adjust("next.", Some("Done.  ")), "Next.");
        assert_eq!(adjust("Next.", Some("and  ")), "next.");
    }

    #[test]
    fn straight_quote_opens_after_space() {
        assert_eq!(adjust("Hello", Some("She said \"")), "hello");
    }
}

mod dictation_history_tests {
    use super::*;

    #[test]
    fn undo_counts_visible_characters() {
        let mut history = DictationHistory::new();
        history.did_insert(" Hej 👋🏽 café");
        assert_eq!(history.take_undo(), Some(11));
        assert_eq!(history.take_undo(), None);
    }

    #[test]
    fn other_input_prevents_undo() {
        let mut history = DictationHistory::new();
        history.did_insert("Hello.");
        history.other_input();
        assert_eq!(history.last_inserted(), None);
        assert_eq!(history.take_undo(), None);
    }
}

mod audio_ring_buffer_tests {
    use super::*;

    #[test]
    fn keeps_everything_until_full() {
        let mut ring = AudioRingBuffer::new(4);
        ring.append(&[1.0, 2.0, 3.0]);
        assert_eq!(ring.samples(), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn keeps_only_the_newest_samples() {
        let mut ring = AudioRingBuffer::new(4);
        ring.append(&[1.0, 2.0, 3.0]);
        ring.append(&[4.0, 5.0, 6.0]);
        assert_eq!(ring.samples(), [3.0, 4.0, 5.0, 6.0]);
        ring.append(&[7.0, 8.0, 9.0, 10.0, 11.0]);
        assert_eq!(ring.samples(), [8.0, 9.0, 10.0, 11.0]);
    }

    #[test]
    fn remove_all_empties_it() {
        let mut ring = AudioRingBuffer::new(2);
        ring.append(&[1.0, 2.0, 3.0]);
        ring.remove_all();
        assert!(ring.samples().is_empty());
        ring.append(&[4.0]);
        assert_eq!(ring.samples(), [4.0]);
    }

    #[test]
    fn zero_capacity_holds_nothing() {
        let mut ring = AudioRingBuffer::new(0);
        ring.append(&[1.0, 2.0]);
        assert!(ring.samples().is_empty());
    }
}
