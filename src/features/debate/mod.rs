//! # Debate Feature
//!
//! Orchestrates threaded debates between two personas on a given topic.
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.27.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.1.0: Added continue debate button and state management
//! - 1.0.0: Initial implementation with threaded debates

pub mod orchestrator;

pub use orchestrator::{DebateOrchestrator, DebateState, get_active_debates, CONTINUE_ROUNDS};
