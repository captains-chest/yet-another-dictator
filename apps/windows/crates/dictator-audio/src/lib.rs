//! Microphone capture converted to 16 kHz mono f32.

pub mod recorder;
pub mod resample;

pub use recorder::{InputDevice, MicrophoneRecorder, input_devices};
