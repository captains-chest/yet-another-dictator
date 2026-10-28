//! Port of `CaptureBuffer.swift`.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::audio_ring_buffer::AudioRingBuffer;

type LevelCallback = Arc<dyn Fn(f32) + Send + Sync>;
type CapacityCallback = Arc<dyn Fn() + Send + Sync>;

/// The level meter shows RMS scaled up, since speech RMS sits far below full scale.
const LEVEL_GAIN: f32 = 8.0;

struct State {
    is_capturing: bool,
    samples: Vec<f32>,
    pre_roll: AudioRingBuffer,
    on_level: Option<LevelCallback>,
    on_capacity_reached: Option<CapacityCallback>,
    /// Whether this capture already hit the capacity (and reported it).
    reached_capacity: bool,
}

/// Thread-safe sample store shared between the realtime audio thread and the UI thread.
///
/// While a dictation is being captured, samples accumulate up to `capacity`. Otherwise only the
/// newest pre-roll samples are kept, and they become the start of the next capture.
pub struct CaptureBuffer {
    state: Mutex<State>,
    capacity: usize,
}

impl CaptureBuffer {
    pub fn new(capacity: usize, pre_roll: usize) -> Self {
        Self {
            state: Mutex::new(State {
                is_capturing: false,
                samples: Vec::new(),
                pre_roll: AudioRingBuffer::new(pre_roll),
                on_level: None,
                on_capacity_reached: None,
                reached_capacity: false,
            }),
            capacity,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Called once per capture, on the first chunk that doesn't fit. Runs on the thread calling
    /// `append` (the realtime audio thread), outside the lock, so it must return immediately.
    pub fn set_on_capacity_reached(&self, callback: Option<Box<dyn Fn() + Send + Sync>>) {
        self.lock().on_capacity_reached = callback.map(Arc::from);
    }

    pub fn is_capturing(&self) -> bool {
        self.lock().is_capturing
    }

    pub fn begin(&self, on_level: Box<dyn Fn(f32) + Send + Sync>) {
        let mut state = self.lock();
        state.samples = state.pre_roll.samples();
        state.pre_roll.remove_all();
        state.on_level = Some(Arc::from(on_level));
        state.is_capturing = true;
        state.reached_capacity = false;
    }

    pub fn end(&self) -> Vec<f32> {
        let mut state = self.lock();
        state.is_capturing = false;
        state.on_level = None;
        std::mem::take(&mut state.samples)
    }

    pub fn clear_pre_roll(&self) {
        self.lock().pre_roll.remove_all();
    }

    pub fn append(&self, chunk: &[f32]) {
        let (on_level, on_capacity_reached) = {
            let mut state = self.lock();
            if !state.is_capturing {
                state.pre_roll.append(chunk);
                return;
            }
            let room = self.capacity.saturating_sub(state.samples.len());
            state
                .samples
                .extend_from_slice(&chunk[..chunk.len().min(room)]);
            let mut capacity_callback = None;
            if chunk.len() > room && !state.reached_capacity {
                state.reached_capacity = true;
                capacity_callback = state.on_capacity_reached.clone();
            }
            (state.on_level.clone(), capacity_callback)
        };
        if let Some(callback) = on_capacity_reached {
            callback();
        }
        let Some(on_level) = on_level else { return };
        let sum_of_squares: f32 = chunk.iter().map(|s| s * s).sum();
        let rms = if chunk.is_empty() {
            0.0
        } else {
            (sum_of_squares / chunk.len() as f32).sqrt()
        };
        on_level((rms * LEVEL_GAIN).min(1.0));
    }
}
