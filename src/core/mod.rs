//! # Core Module
//!
//! Core domain types, configuration, and error handling for the persona bot.
//!
//! - **Version**: 1.3.0
//! - **Since**: 0.7.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.3.0: Add embeds module with shared persona embed builders
//! - 1.2.0: Add file_utils module with download, content detection, and file utilities
//! - 1.1.0: Add response module with Discord message chunking utilities
//! - 1.0.0: Initial creation with config module

pub mod config;
pub mod embeds;
pub mod file_utils;
pub mod response;

// Re-export commonly used items
pub use config::Config;
pub use embeds::{continuation_embed, persona_embed};
pub use file_utils::{
    detect_content_kind, download_file, extract_filename, format_file_size, is_within_upload_limit,
    max_upload_size, sanitize_filename, ContentKind, DownloadedFile,
};
pub use response::{
    chunk_for_embed, chunk_for_message, chunk_text, truncate_for_embed, truncate_for_message,
    EMBED_LIMIT, MESSAGE_LIMIT,
};
