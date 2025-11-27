//! # Audio Feature
//!
//! Whisper-powered audio transcription with configurable output modes.
//!
//! - **Version**: 1.3.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true

pub mod transcriber;

pub use transcriber::{AudioTranscriber, TranscriptionResult};
