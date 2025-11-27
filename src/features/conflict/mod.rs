//! # Conflict Feature
//!
//! Detects heated discussions and provides Obi-Wan themed mediation.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true

pub mod detector;
pub mod mediator;

pub use detector::ConflictDetector;
pub use mediator::ConflictMediator;
