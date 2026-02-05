//! # TUI Module
//!
//! Terminal user interface for controlling and monitoring the Obi bot.
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.17.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial TUI implementation with dashboard, channel watcher, stats, settings

pub mod app;
pub mod event;
pub mod state;
pub mod ui;

pub use app::{App, Screen};
pub use event::{Event, EventHandler};
