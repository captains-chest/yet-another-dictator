//! Port of `AudioRingBuffer.swift`.

/// Keeps only the most recent `capacity` samples.
///
/// Used for the microphone pre-roll: while the microphone is kept ready, the last fraction of a
/// second is held here so a dictation can include the syllable spoken just before the shortcut
/// registered. Older audio is overwritten and never leaves memory.
#[derive(Debug, Clone)]
pub struct AudioRingBuffer {
    capacity: usize,
    storage: Vec<f32>,
    start: usize,
    count: usize,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            storage: vec![0.0; capacity],
            start: 0,
            count: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn append(&mut self, samples: &[f32]) {
        if self.capacity == 0 {
            return;
        }
        // Only the newest `capacity` samples can survive.
        let newest = &samples[samples.len().saturating_sub(self.capacity)..];
        for &sample in newest {
            let end = (self.start + self.count) % self.capacity;
            self.storage[end] = sample;
            if self.count < self.capacity {
                self.count += 1;
            } else {
                self.start = (self.start + 1) % self.capacity;
            }
        }
    }

    /// The held samples, oldest first.
    pub fn samples(&self) -> Vec<f32> {
        (0..self.count)
            .map(|i| self.storage[(self.start + i) % self.capacity])
            .collect()
    }

    pub fn remove_all(&mut self) {
        self.start = 0;
        self.count = 0;
    }
}
