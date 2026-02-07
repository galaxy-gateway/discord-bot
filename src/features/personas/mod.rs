//! # Personas Feature
//!
//! Multi-personality AI response system with 17 distinct personas.
//!
//! - **Version**: 1.3.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.3.0: Add prompt_builder module for fluent system prompt construction
//! - 1.2.0: Add shared choices module for slash commands
//! - 1.1.0: Add apply_paragraph_limit() for max_paragraphs response control
//! - 1.0.0: Initial release

pub mod choices;
pub mod manager;
pub mod prompt_builder;

pub use choices::{add_persona_choices, is_valid_persona, PERSONA_CHOICES};
pub use manager::{apply_paragraph_limit, Persona, PersonaManager};
pub use prompt_builder::PromptBuilder;
