//! # Core Module
//!
//! Core domain types, configuration, and error handling for the persona bot.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.7.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Add response module with Discord message chunking utilities
//! - 1.0.0: Initial creation with config module

pub mod config;
pub mod response;

// Re-export commonly used items
pub use config::Config;
pub use response::{
    chunk_for_embed, chunk_for_message, chunk_text, truncate_for_embed, truncate_for_message,
    EMBED_LIMIT, MESSAGE_LIMIT,
};
