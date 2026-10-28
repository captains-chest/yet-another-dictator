//! Converts microphone audio (any rate, any channel count) to the 16 kHz mono f32 the speech
//! models take. Pure and streaming: chunks can arrive in any size and the result is the same as
//! converting everything at once.

use std::f64::consts::PI;

/// The rate every speech model expects (DictationController.sampleRate on macOS).
pub const TARGET_RATE_HZ: u32 = 16_000;

/// Zero crossings of the sinc on each side of the kernel. More is sharper but slower.
const KERNEL_ZERO_CROSSINGS: f64 = 16.0;

/// The low-pass cutoff as a fraction of the output Nyquist, leaving room for the filter's slope.
const CUTOFF_FRACTION: f64 = 0.95;

/// Averages interleaved frames into one channel.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => interleaved.to_vec(),
        _ => interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect(),
    }
}

/// Streaming windowed-sinc resampler to [`TARGET_RATE_HZ`].
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Low-pass cutoff in cycles per input sample.
    cutoff: f64,
    /// Kernel half-width in input samples.
    half_width: usize,
    /// Input not yet fully used; `history[0]` is input sample number `offset`.
    history: Vec<f32>,
    offset: usize,
    /// Input position (in input samples) of the next output sample.
    next_position: f64,
}

impl Resampler {
    pub fn new(input_rate_hz: u32) -> Self {
        let step = f64::from(input_rate_hz) / f64::from(TARGET_RATE_HZ);
        let cutoff = 0.5 * (1.0 / step).min(1.0) * CUTOFF_FRACTION;
        let half_width = (KERNEL_ZERO_CROSSINGS / (2.0 * cutoff)).ceil() as usize;
        Self {
            step,
            cutoff,
            half_width,
            history: Vec::new(),
            offset: 0,
            next_position: 0.0,
        }
    }

    fn is_passthrough(&self) -> bool {
        self.step == 1.0
    }

    /// Converts the next chunk of mono input. Output lags the input by the kernel half-width
    /// until [`Resampler::finish`].
    pub fn push(&mut self, mono: &[f32]) -> Vec<f32> {
        if self.is_passthrough() {
            return mono.to_vec();
        }
        self.history.extend_from_slice(mono);
        self.drain()
    }

    /// Emits what's left, as if the input ended with silence.
    pub fn finish(&mut self) -> Vec<f32> {
        if self.is_passthrough() {
            return Vec::new();
        }
        let end = self.offset + self.history.len();
        let mut output = Vec::new();
        self.history
            .extend(std::iter::repeat_n(0.0, self.half_width + 1));
        while self.next_position < end as f64 {
            output.push(self.sample_at(self.next_position));
            self.next_position += self.step;
        }
        self.history.clear();
        self.offset = end;
        output
    }

    fn drain(&mut self) -> Vec<f32> {
        let mut output = Vec::new();
        let available = self.offset + self.history.len();
        while (self.next_position.floor() as usize) + self.half_width < available {
            output.push(self.sample_at(self.next_position));
            self.next_position += self.step;
        }
        let keep_from = (self.next_position.floor() as usize).saturating_sub(self.half_width);
        if keep_from > self.offset {
            self.history.drain(..keep_from - self.offset);
            self.offset = keep_from;
        }
        output
    }

    fn sample_at(&self, position: f64) -> f32 {
        let center = position.floor() as isize;
        let half_width = self.half_width as isize;
        let mut sum = 0.0;
        for index in (center - half_width)..=(center + half_width) {
            // Before the first sample is silence.
            let Ok(relative) = usize::try_from(index - self.offset as isize) else {
                continue;
            };
            let Some(&value) = self.history.get(relative) else {
                continue;
            };
            sum += f64::from(value) * self.kernel(position - index as f64);
        }
        sum as f32
    }

    fn kernel(&self, distance: f64) -> f64 {
        let half_width = self.half_width as f64;
        if distance.abs() >= half_width {
            return 0.0;
        }
        let x = 2.0 * self.cutoff * distance;
        let sinc = if x == 0.0 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        };
        // Blackman window over [-half_width, half_width].
        let phase = PI * (distance / half_width + 1.0);
        let window = 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
        2.0 * self.cutoff * sinc * window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate_hz: u32, frequency_hz: f64, seconds: f64) -> Vec<f32> {
        let count = (f64::from(rate_hz) * seconds) as usize;
        (0..count)
            .map(|n| (2.0 * PI * frequency_hz * n as f64 / f64::from(rate_hz)).sin() as f32)
            .collect()
    }

    fn convert(rate_hz: u32, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut resampler = Resampler::new(rate_hz);
        let mut output: Vec<f32> = input
            .chunks(chunk)
            .flat_map(|c| resampler.push(c))
            .collect();
        output.extend(resampler.finish());
        output
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[0.25, 0.75], 1), vec![0.25, 0.75]);
    }

    #[test]
    fn sixteen_khz_passes_through_unchanged() {
        let input = sine(16_000, 440.0, 0.1);
        assert_eq!(convert(16_000, &input, 100), input);
    }

    #[test]
    fn output_length_matches_the_rate_ratio() {
        for rate in [44_100, 48_000, 96_000, 8_000] {
            let input = sine(rate, 440.0, 1.0);
            let output = convert(rate, &input, 480);
            let expected = (input.len() as f64 * 16_000.0 / f64::from(rate)).ceil() as usize;
            assert!(
                output.len().abs_diff(expected) <= 1,
                "{rate}: {} vs {expected}",
                output.len()
            );
        }
    }

    #[test]
    fn speech_band_tone_keeps_its_level() {
        let output = convert(48_000, &sine(48_000, 1_000.0, 1.0), 512);
        // Skip the filter's edges.
        let middle = &output[1_000..output.len() - 1_000];
        assert!(
            (rms(middle) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "{}",
            rms(middle)
        );
    }

    #[test]
    fn tone_above_the_new_nyquist_is_removed() {
        let output = convert(48_000, &sine(48_000, 12_000.0, 1.0), 512);
        let middle = &output[1_000..output.len() - 1_000];
        assert!(rms(middle) < 0.01, "{}", rms(middle));
    }

    #[test]
    fn chunk_size_does_not_change_the_result() {
        let input = sine(44_100, 523.0, 0.5);
        let whole = convert(44_100, &input, input.len());
        let small = convert(44_100, &input, 37);
        assert_eq!(whole.len(), small.len());
        assert!(whole.iter().zip(&small).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
