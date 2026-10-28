//! Records the microphone through cpal (WASAPI on Windows) into the core's [`CaptureBuffer`].
//!
//! It records from the device chosen in Settings, or the system default input. If the chosen
//! device is unplugged, the next recording uses the system default, and the chosen device again
//! once it is back. With "keep microphone ready" on, the stream keeps running between dictations
//! and feeds the pre-roll, so the first word isn't clipped.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use dictator_core::capture_buffer::CaptureBuffer;
use dictator_core::{AudioRecording, PortError, PortResult};

use crate::resample::{Resampler, TARGET_RATE_HZ, downmix};

/// Hard cap so a forgotten recording can't grow without bound (10 minutes ≈ 38 MB).
const MAX_SECONDS: usize = 10 * 60;
const PRE_ROLL_SECONDS: f64 = 0.4;
/// How long to wait for the audio driver to open the device.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// A microphone the user can pick in Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDevice {
    /// Stable across restarts; what Settings stores.
    pub id: String,
    pub name: String,
}

/// Lists the connected microphones, for Settings.
pub fn input_devices() -> Vec<InputDevice> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device
                .description()
                .map(|d| d.name().to_owned())
                .unwrap_or_else(|_| id.clone());
            Some(InputDevice { id, name })
        })
        .collect()
}

type FailureCallback = Arc<dyn Fn(String) + Send + Sync>;

struct RunningStream {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}

pub struct MicrophoneRecorder {
    buffer: Arc<CaptureBuffer>,
    device_id: Option<String>,
    keeps_ready: bool,
    running: Option<RunningStream>,
    on_failure: FailureCallback,
}

impl MicrophoneRecorder {
    /// `on_failure` runs on an audio thread when the stream dies on its own (device unplugged);
    /// it must return immediately. `on_capacity_reached` likewise, once per recording at the cap.
    pub fn new(
        on_failure: impl Fn(String) + Send + Sync + 'static,
        on_capacity_reached: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let rate = TARGET_RATE_HZ as usize;
        let buffer = CaptureBuffer::new(
            rate * MAX_SECONDS,
            (rate as f64 * PRE_ROLL_SECONDS) as usize,
        );
        buffer.set_on_capacity_reached(Some(Box::new(on_capacity_reached)));
        Self {
            buffer: Arc::new(buffer),
            device_id: None,
            keeps_ready: false,
            running: None,
            on_failure: Arc::new(on_failure),
        }
    }

    /// The device id from [`input_devices`]; `None` follows the system default.
    /// Takes effect immediately when idle, otherwise at the next recording.
    pub fn set_device(&mut self, device_id: Option<String>) {
        if self.device_id == device_id {
            return;
        }
        self.device_id = device_id;
        if !self.buffer.is_capturing() {
            self.stop_stream();
            if self.keeps_ready {
                self.start_stream_reporting_failure();
            }
        }
    }

    /// Keeps the stream running between dictations, feeding the pre-roll buffer.
    pub fn set_keeps_ready(&mut self, on: bool) {
        self.keeps_ready = on;
        if self.buffer.is_capturing() {
            return;
        }
        if on {
            self.start_stream_reporting_failure();
        } else {
            self.stop_stream();
        }
    }

    fn start_stream_reporting_failure(&mut self) {
        if let Err(PortError::Failed(message)) = self.ensure_stream() {
            (self.on_failure)(message);
        }
    }

    fn ensure_stream(&mut self) -> PortResult<()> {
        if self
            .running
            .as_ref()
            .is_some_and(|r| !r.thread.is_finished())
        {
            return Ok(());
        }
        self.stop_stream();
        self.buffer.clear_pre_roll();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let buffer = Arc::clone(&self.buffer);
        let device_id = self.device_id.clone();
        let on_failure = Arc::clone(&self.on_failure);
        // cpal streams aren't Send on every platform, so one thread owns the stream for its life.
        let thread = std::thread::Builder::new()
            .name("dictator-microphone".into())
            .spawn(
                move || match open_stream(device_id.as_deref(), buffer, on_failure) {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(message) => {
                        let _ = ready_tx.send(Err(message));
                    }
                },
            )
            .map_err(|error| {
                PortError::Failed(format!("Couldn't start the microphone thread: {error}"))
            })?;
        let result = ready_rx
            .recv_timeout(OPEN_TIMEOUT)
            .unwrap_or_else(|_| Err("The microphone didn't start in time.".to_owned()));
        self.running = Some(RunningStream {
            stop: stop_tx,
            thread,
        });
        if let Err(message) = result {
            self.stop_stream();
            return Err(PortError::Failed(message));
        }
        Ok(())
    }

    fn stop_stream(&mut self) {
        if let Some(running) = self.running.take() {
            let _ = running.stop.send(());
            let _ = running.thread.join();
        }
    }
}

impl AudioRecording for MicrophoneRecorder {
    fn start(&mut self, on_level: Box<dyn Fn(f32) + Send + Sync>) -> PortResult<()> {
        self.ensure_stream()?;
        self.buffer.begin(on_level);
        Ok(())
    }

    fn stop(&mut self) -> Vec<f32> {
        let samples = self.buffer.end();
        if !self.keeps_ready {
            self.stop_stream();
        }
        samples
    }
}

impl Drop for MicrophoneRecorder {
    fn drop(&mut self) {
        self.stop_stream();
    }
}

fn open_stream(
    device_id: Option<&str>,
    buffer: Arc<CaptureBuffer>,
    on_failure: FailureCallback,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let chosen = device_id
        .and_then(|id| id.parse().ok())
        .and_then(|id| host.device_by_id(&id));
    let device = chosen
        .or_else(|| host.default_input_device())
        .ok_or_else(|| "No microphone is connected.".to_owned())?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("Couldn't read the microphone's format: {error}"))?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, buffer, on_failure),
        SampleFormat::I16 => build::<i16>(&device, config, buffer, on_failure),
        SampleFormat::I32 => build::<i32>(&device, config, buffer, on_failure),
        SampleFormat::U16 => build::<u16>(&device, config, buffer, on_failure),
        SampleFormat::U8 => build::<u8>(&device, config, buffer, on_failure),
        SampleFormat::F64 => build::<f64>(&device, config, buffer, on_failure),
        other => {
            return Err(format!(
                "The microphone's sample format {other} isn't supported."
            ));
        }
    }
    .map_err(|error| format!("Couldn't open the microphone: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("Couldn't start the microphone: {error}"))?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    buffer: Arc<CaptureBuffer>,
    on_failure: FailureCallback,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut resampler = Resampler::new(config.sample_rate);
    let mut converted = Vec::new();
    device.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], _| {
            converted.clear();
            converted.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
            buffer.append(&resampler.push(&downmix(&converted, channels)));
        },
        move |error| on_failure(format!("The microphone stopped: {error}")),
        None,
    )
}
