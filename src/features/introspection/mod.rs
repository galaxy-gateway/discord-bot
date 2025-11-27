//! # Introspection Feature
//!
//! Bot can explain its own internals and architecture.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false

pub mod service;

pub use service::get_component_snippet;
