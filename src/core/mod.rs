//! # Core Module
//!
//! Core domain types, configuration, and error handling for the persona bot.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.7.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial creation with config module

pub mod config;

// Re-export commonly used items
pub use config::Config;
