//! Splits a recording longer than Whisper's 30 s window into pieces that are each transcribed whole.
//! A port of the macOS `LongAudioSplitter`.
//!
//! Whisper hears at most 30 s at a time. Given longer audio, it is cut into back-to-back 30 s
//! windows: a word that crosses a window boundary is split in two, and a very short last window
//! is skipped, so the end of the dictation goes missing.
//!
//! Cutting at the quietest moment in the second half of each window keeps words whole, and keeping
//! every piece at least `min_length` long means none is skipped. Audio of up to 30 s is one piece.

use std::ops::Range;

/// Whisper's window: 30 s at 16 kHz.
pub const MAX_LENGTH: usize = 480_000;
/// Shortest piece a split may leave: 2 s, comfortably above a 1 s clip.
pub const MIN_LENGTH: usize = 32_000;
/// The stretch whose loudness is compared when looking for a pause: 0.2 s.
const QUIET_WINDOW: usize = 3_200;
/// Distance between candidate cut points: 10 ms.
const STEP: usize = 160;

/// Consecutive ranges that cover `samples` exactly, each at most `max_length` long. When the audio
/// is split, every piece is at least `min_length` long.
pub fn ranges(samples: &[f32], max_length: usize, min_length: usize) -> Vec<Range<usize>> {
    assert!(
        min_length > 0 && min_length * 2 <= max_length,
        "min_length must be at most half of max_length"
    );
    if samples.len() <= max_length {
        #[allow(clippy::single_range_in_vec_init)] // one piece: the whole recording
        return vec![0..samples.len()];
    }

    let energy = PrefixEnergy::new(samples);
    let mut pieces = Vec::new();
    let mut start = 0;
    while samples.len() - start > max_length {
        // Cut in the second half of the window, and leave at least min_length for what follows.
        let earliest = start + max_length / 2;
        let latest = (start + max_length).min(samples.len() - min_length);
        let cut = quietest_point(earliest, latest, &energy);
        pieces.push(start..cut);
        start = cut;
    }
    pieces.push(start..samples.len());
    pieces
}

/// The candidate point in `lower..=upper` whose surrounding `QUIET_WINDOW` holds the least
/// energy. On a tie the later point wins, so pieces stay close to the full window.
fn quietest_point(lower: usize, upper: usize, energy: &PrefixEnergy) -> usize {
    let mut best = lower;
    let mut best_energy = f64::INFINITY;
    let mut point = lower;
    loop {
        let value = energy.sum(
            point.saturating_sub(QUIET_WINDOW / 2),
            point + QUIET_WINDOW / 2,
        );
        if value <= best_energy {
            best = point;
            best_energy = value;
        }
        if point == upper {
            break;
        }
        point = (point + STEP).min(upper);
    }
    best
}

/// Running sum of squared samples, so any stretch's energy costs two lookups.
struct PrefixEnergy(Vec<f64>);

impl PrefixEnergy {
    fn new(samples: &[f32]) -> Self {
        let mut sums = Vec::with_capacity(samples.len() + 1);
        sums.push(0.0);
        let mut total = 0.0;
        for &sample in samples {
            total += f64::from(sample) * f64::from(sample);
            sums.push(total);
        }
        Self(sums)
    }

    /// Energy of `lower..upper`, clamped to the recording.
    fn sum(&self, lower: usize, upper: usize) -> f64 {
        let last = self.0.len() - 1;
        let lower = lower.min(last);
        let upper = upper.clamp(lower, last);
        self.0[upper] - self.0[lower]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 16_000;

    /// A steady tone, loud enough to count as speech everywhere.
    fn tone(seconds: f64) -> Vec<f32> {
        (0..(seconds * RATE as f64) as usize)
            .map(|index| 0.3 * (index as f64 * 0.2).sin() as f32)
            .collect()
    }

    fn split(samples: &[f32]) -> Vec<Range<usize>> {
        ranges(samples, MAX_LENGTH, MIN_LENGTH)
    }

    fn expect_covers(pieces: &[Range<usize>], count: usize) {
        assert_eq!(pieces.first().map(|piece| piece.start), Some(0));
        assert_eq!(pieces.last().map(|piece| piece.end), Some(count));
        for pair in pieces.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        for piece in pieces {
            assert!(piece.len() <= MAX_LENGTH, "{piece:?}");
            if pieces.len() > 1 {
                assert!(piece.len() >= MIN_LENGTH, "{piece:?}");
            }
        }
    }

    #[test]
    fn up_to_thirty_seconds_is_one_piece() {
        for count in [0, 1, 16_000, 479_999, 480_000] {
            let samples = vec![0.1; count];
            #[allow(clippy::single_range_in_vec_init)]
            let whole = [0..count];
            assert_eq!(split(&samples), whole);
        }
    }

    #[test]
    fn cuts_in_the_pause() {
        // 40 s of "speech" with a 0.3 s pause at 25 s.
        let mut samples = tone(40.0);
        let pause = 25 * RATE..(25 * RATE + 4_800);
        samples[pause.clone()].fill(0.0);

        let pieces = split(&samples);

        assert_eq!(pieces.len(), 2);
        let cut = pieces[0].end;
        assert!(pause.contains(&cut), "cut at {cut}");
        expect_covers(&pieces, samples.len());
    }

    /// Just over 30 s used to lose the last half second: a window under 1 s is skipped.
    #[test]
    fn just_over_thirty_seconds_keeps_a_long_enough_tail() {
        let samples = tone(30.5);

        let pieces = split(&samples);

        assert_eq!(pieces.len(), 2);
        assert!(pieces[1].len() >= MIN_LENGTH);
        expect_covers(&pieces, samples.len());
    }

    #[test]
    fn without_a_pause_cuts_as_late_as_allowed() {
        // Equally loud everywhere (±0.5 squares exactly, so every stretch has the same energy).
        let samples: Vec<f32> = (0..70 * RATE)
            .map(|index| if index % 2 == 0 { 0.5 } else { -0.5 })
            .collect();

        let pieces = split(&samples);

        assert_eq!(
            pieces
                .iter()
                .map(ExactSizeIterator::len)
                .collect::<Vec<_>>(),
            [480_000, 480_000, 160_000]
        );
        expect_covers(&pieces, samples.len());
    }

    #[test]
    fn pieces_cover_the_recording() {
        for seconds in [30.01, 31.0, 45.5, 60.0, 61.9, 90.0, 600.0] {
            let mut samples = tone(seconds);
            // Some pauses at irregular places.
            let mut start = 7 * RATE;
            while start < samples.len() - 3_200 {
                samples[start..start + 3_200].fill(0.001);
                start += 11 * RATE + 1_234;
            }

            expect_covers(&split(&samples), samples.len());
        }
    }
}
