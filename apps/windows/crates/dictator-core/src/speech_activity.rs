//! Port of `SpeechActivity.swift`.
//!
//! A cheap loudness check that tells clearly silent recordings apart from ones that may hold
//! speech. Whisper invents text for silence and faint noise, so a clip with no speech must never
//! reach it. Deliberately conservative: it only rejects audio that stays below a quiet-room level
//! for almost all of its length.

/// Analysis frame: 30 ms at 16 kHz.
pub const FRAME_LENGTH: usize = 480;
/// Frame RMS that counts as "something audible": about −48 dBFS, well below quiet speech on a
/// laptop microphone (around −40 dBFS) and above a typical quiet-room noise floor (−60 to −55).
pub const LEVEL_THRESHOLD: f32 = 0.004;
/// Audible audio needed in total. Shorter bursts (a key click, a tap on the desk) don't count.
pub const MINIMUM_AUDIBLE_SECONDS: f64 = 0.1;

pub fn contains_speech(samples: &[f32]) -> bool {
    contains_speech_at(samples, f64::from(crate::SAMPLE_RATE_HZ))
}

pub fn contains_speech_at(samples: &[f32], sample_rate: f64) -> bool {
    let needed = (MINIMUM_AUDIBLE_SECONDS * sample_rate / FRAME_LENGTH as f64).ceil() as usize;
    let threshold = LEVEL_THRESHOLD * LEVEL_THRESHOLD * FRAME_LENGTH as f32;
    let mut audible = 0;
    for frame in samples.as_chunks::<FRAME_LENGTH>().0 {
        let sum_of_squares: f32 = frame.iter().fold(0.0, |sum, s| sum + s * s);
        if sum_of_squares >= threshold {
            audible += 1;
            if audible >= needed {
                return true;
            }
        }
    }
    false
}
