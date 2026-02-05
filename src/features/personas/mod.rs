//! # Personas Feature
//!
//! Multi-personality AI response system with 5 distinct personas.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Add apply_paragraph_limit() for max_paragraphs response control
//! - 1.0.0: Initial release

pub mod manager;

pub use manager::{apply_paragraph_limit, Persona, PersonaManager};
