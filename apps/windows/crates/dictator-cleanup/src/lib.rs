//! Transcript cleanup (ADR 0011), ported from the macOS `TranscriptCleanup` target: S1-mini's
//! prompt format and chunking, and [`CleaningTranscriber`], the decorator that cleans each
//! transcript and falls back to the raw text. Pure: the llama.cpp adapter is in dictator-engines.

pub mod cleaning_transcriber;
pub mod s1_mini_format;

pub use cleaning_transcriber::{CleaningTranscriber, Outcome};
