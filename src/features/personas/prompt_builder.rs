//! Unified system prompt construction
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Consolidated prompt building into fluent builder API

use super::PersonaManager;

/// Builder for constructing system prompts with modifiers and verbosity
///
/// Provides a fluent API for building system prompts with various options:
/// - Persona selection
/// - Optional modifiers (explain, simple, steps, recipe)
/// - Verbosity levels (concise, normal, detailed)
/// - Max paragraph limits
///
/// # Example
///
/// ```ignore
/// let prompt = PromptBuilder::new(&persona_manager, "obi")
///     .with_modifier(Some("explain"))
///     .with_verbosity("detailed")
///     .with_max_paragraphs(Some(3))
///     .build();
/// ```
pub struct PromptBuilder<'a> {
    persona_manager: &'a PersonaManager,
    persona_id: String,
    modifier: Option<String>,
    verbosity: String,
    max_paragraphs: Option<u32>,
}

impl<'a> PromptBuilder<'a> {
    /// Create a new PromptBuilder for a given persona
    pub fn new(persona_manager: &'a PersonaManager, persona_id: &str) -> Self {
        Self {
            persona_manager,
            persona_id: persona_id.to_string(),
            modifier: None,
            verbosity: "normal".to_string(),
            max_paragraphs: None,
        }
    }

    /// Set an optional modifier (explain, simple, steps, recipe)
    pub fn with_modifier(mut self, modifier: Option<&str>) -> Self {
        self.modifier = modifier.map(String::from);
        self
    }

    /// Set the verbosity level (concise, normal, detailed)
    pub fn with_verbosity(mut self, verbosity: &str) -> Self {
        self.verbosity = verbosity.to_string();
        self
    }

    /// Set maximum paragraphs for the response
    pub fn with_max_paragraphs(mut self, max: Option<u32>) -> Self {
        self.max_paragraphs = max;
        self
    }

    /// Build the final system prompt
    pub fn build(self) -> String {
        let mut prompt = self.persona_manager.get_system_prompt_with_verbosity(
            &self.persona_id,
            self.modifier.as_deref(),
            &self.verbosity,
        );

        if let Some(max) = self.max_paragraphs {
            if max > 0 {
                prompt.push_str(&format!(
                    "\n\nIMPORTANT: Limit your response to {max} paragraph(s) maximum."
                ));
            }
        }
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_builder_basic() {
        let manager = PersonaManager::new();
        let prompt = PromptBuilder::new(&manager, "obi").build();
        assert!(!prompt.is_empty());
    }

    #[test]
    fn test_prompt_builder_with_modifier() {
        let manager = PersonaManager::new();
        let prompt = PromptBuilder::new(&manager, "teacher")
            .with_modifier(Some("explain"))
            .build();
        assert!(prompt.contains("clear explanations"));
    }

    #[test]
    fn test_prompt_builder_with_verbosity() {
        let manager = PersonaManager::new();
        let prompt = PromptBuilder::new(&manager, "analyst")
            .with_verbosity("concise")
            .build();
        assert!(prompt.contains("brief and to the point"));
    }

    #[test]
    fn test_prompt_builder_with_max_paragraphs() {
        let manager = PersonaManager::new();
        let prompt = PromptBuilder::new(&manager, "chef")
            .with_max_paragraphs(Some(3))
            .build();
        assert!(prompt.contains("3 paragraph(s) maximum"));
    }

    #[test]
    fn test_prompt_builder_zero_paragraphs_ignored() {
        let manager = PersonaManager::new();
        let prompt = PromptBuilder::new(&manager, "chef")
            .with_max_paragraphs(Some(0))
            .build();
        assert!(!prompt.contains("paragraph(s) maximum"));
    }

    #[test]
    fn test_prompt_builder_chained() {
        let manager = PersonaManager::new();
        let prompt = PromptBuilder::new(&manager, "scientist")
            .with_modifier(Some("simple"))
            .with_verbosity("detailed")
            .with_max_paragraphs(Some(5))
            .build();

        assert!(prompt.contains("simple and concise"));
        assert!(prompt.contains("comprehensive, detailed"));
        assert!(prompt.contains("5 paragraph(s) maximum"));
    }
}
