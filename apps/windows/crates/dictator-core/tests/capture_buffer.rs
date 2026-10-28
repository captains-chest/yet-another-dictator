//! Port of CaptureBufferTests.swift.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dictator_core::capture_buffer::CaptureBuffer;

/// Counts calls from callbacks.
#[derive(Clone, Default)]
struct Counter(Arc<AtomicUsize>);

impl Counter {
    fn increment(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
    fn callback(&self) -> Option<Box<dyn Fn() + Send + Sync>> {
        let counter = self.clone();
        Some(Box::new(move || counter.increment()))
    }
}

fn ignore_level() -> Box<dyn Fn(f32) + Send + Sync> {
    Box::new(|_| {})
}

#[test]
fn pre_roll_keeps_the_newest_samples_and_is_prepended_on_begin() {
    let buffer = CaptureBuffer::new(100, 3);
    buffer.append(&[1.0, 2.0, 3.0, 4.0, 5.0]);
    buffer.begin(ignore_level());
    buffer.append(&[6.0, 7.0]);
    assert_eq!(buffer.end(), [3.0, 4.0, 5.0, 6.0, 7.0]);
}

#[test]
fn cleared_pre_roll_is_not_prepended() {
    let buffer = CaptureBuffer::new(100, 3);
    buffer.append(&[1.0, 2.0]);
    buffer.clear_pre_roll();
    buffer.begin(ignore_level());
    buffer.append(&[3.0]);
    assert_eq!(buffer.end(), [3.0]);
}

#[test]
fn end_returns_and_clears() {
    let buffer = CaptureBuffer::new(100, 0);
    buffer.begin(ignore_level());
    assert!(buffer.is_capturing());
    buffer.append(&[1.0, 2.0]);
    assert_eq!(buffer.end(), [1.0, 2.0]);
    assert!(!buffer.is_capturing());
    assert!(buffer.end().is_empty());
}

#[test]
fn truncates_exactly_at_capacity() {
    let buffer = CaptureBuffer::new(4, 0);
    let reached = Counter::default();
    buffer.set_on_capacity_reached(reached.callback());
    buffer.begin(ignore_level());
    buffer.append(&[1.0, 2.0, 3.0, 4.0]); // fills it exactly: nothing lost yet
    assert_eq!(reached.count(), 0);
    buffer.append(&[5.0]);
    assert_eq!(buffer.end(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(reached.count(), 1);
}

#[test]
fn truncates_a_chunk_that_overflows() {
    let buffer = CaptureBuffer::new(3, 0);
    buffer.begin(ignore_level());
    buffer.append(&[1.0, 2.0]);
    buffer.append(&[3.0, 4.0, 5.0]);
    assert_eq!(buffer.end(), [1.0, 2.0, 3.0]);
}

#[test]
fn capacity_callback_fires_once_per_capture_and_again_after_begin() {
    let buffer = CaptureBuffer::new(2, 0);
    let reached = Counter::default();
    buffer.set_on_capacity_reached(reached.callback());

    buffer.begin(ignore_level());
    buffer.append(&[1.0, 2.0, 3.0]);
    buffer.append(&[4.0]);
    buffer.append(&[5.0, 6.0]);
    assert_eq!(reached.count(), 1);
    buffer.end();

    buffer.begin(ignore_level());
    buffer.append(&[1.0]);
    assert_eq!(reached.count(), 1);
    buffer.append(&[2.0, 3.0]);
    assert_eq!(reached.count(), 2);
}

#[test]
fn no_capacity_callback_outside_a_capture() {
    let buffer = CaptureBuffer::new(1, 2);
    let reached = Counter::default();
    buffer.set_on_capacity_reached(reached.callback());
    buffer.append(&[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(reached.count(), 0);
}

#[test]
fn on_level_only_during_a_capture() {
    let buffer = CaptureBuffer::new(100, 4);
    let levels = Counter::default();
    buffer.append(&[0.5]);
    let counter = levels.clone();
    buffer.begin(Box::new(move |_| counter.increment()));
    buffer.append(&[0.5, 0.5]);
    assert_eq!(levels.count(), 1);
    buffer.end();
    buffer.append(&[0.5]);
    assert_eq!(levels.count(), 1);
}

#[test]
fn is_shareable_across_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CaptureBuffer>();
}
