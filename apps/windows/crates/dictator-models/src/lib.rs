//! The pinned model manifest, the speech-model catalog, and the verified, resumable installer.
//! Ports of the macOS `ModelManifest`, `ModelInstaller` and `SpeechModel`.

pub mod catalog;
pub mod installer;
pub mod manifest;
pub mod ranged_transport;

pub use catalog::{CLEANUP_MODEL_ID, SpeechModel};
pub use installer::{InstallError, ModelInstaller, ModelTransport, Progress, default_root};
pub use manifest::{Entry, ModelManifest, ParseError};
pub use ranged_transport::RangedTransport;
