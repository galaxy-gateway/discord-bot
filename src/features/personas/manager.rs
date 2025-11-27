//! # Feature: Persona System
//!
//! Multi-personality AI responses with 5 distinct personas (obi, muppet, chef, teacher, analyst).
//! Each persona has a unique system prompt loaded from prompt/*.md files at compile time.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial release with 5 personas and verbosity modifiers

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub name: String,
    pub system_prompt: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct PersonaManager {
    personas: HashMap<String, Persona>,
}

impl Default for PersonaManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PersonaManager {
    pub fn new() -> Self {
        let mut personas = HashMap::new();

        // Load all personas with prompts embedded at compile time
        personas.insert("obi".to_string(), Persona {
            name: "Obi-Wan".to_string(),
            system_prompt: include_str!("../../../prompt/obi.md").to_string(),
            description: "A wise Jedi Master who speaks with patience, diplomacy, and philosophical insight".to_string(),
        });

        personas.insert("muppet".to_string(), Persona {
            name: "Muppet Friend".to_string(),
            system_prompt: include_str!("../../../prompt/muppet.md").to_string(),
            description: "A warm, enthusiastic friend who brings Muppet-style joy, humor, and heart to every conversation!".to_string(),
        });

        personas.insert("chef".to_string(), Persona {
            name: "Chef".to_string(),
            system_prompt: include_str!("../../../prompt/chef.md").to_string(),
            description: "A passionate chef who shares recipes and cooking wisdom".to_string(),
        });

        personas.insert("teacher".to_string(), Persona {
            name: "Teacher".to_string(),
            system_prompt: include_str!("../../../prompt/teacher.md").to_string(),
            description: "A patient teacher who explains things clearly".to_string(),
        });

        personas.insert("analyst".to_string(), Persona {
            name: "Step-by-Step Analyst".to_string(),
            system_prompt: include_str!("../../../prompt/analyst.md").to_string(),
            description: "An analyst who breaks things down into clear steps".to_string(),
        });

        PersonaManager { personas }
    }

    pub fn get_persona(&self, name: &str) -> Option<&Persona> {
        self.personas.get(name)
    }

    pub fn list_personas(&self) -> Vec<(&String, &Persona)> {
        self.personas.iter().collect()
    }

    pub fn get_system_prompt(&self, persona_name: &str, modifier: Option<&str>) -> String {
        self.get_system_prompt_with_verbosity(persona_name, modifier, "normal")
    }

    /// Get system prompt with verbosity level applied
    pub fn get_system_prompt_with_verbosity(&self, persona_name: &str, modifier: Option<&str>, verbosity: &str) -> String {
        let base_prompt = self.personas
            .get(persona_name)
            .map(|p| p.system_prompt.clone())
            .unwrap_or_else(|| "You are a helpful assistant.".to_string());

        // Apply modifier first
        let with_modifier = match modifier {
            Some("explain") => format!("{base_prompt} Focus on providing clear explanations."),
            Some("simple") => format!("{base_prompt} Explain in a simple and concise way. Give analogies a beginner might understand."),
            Some("steps") => format!("{base_prompt} Break this out into clear, actionable steps."),
            Some("recipe") => format!("{base_prompt} Respond with a recipe if this prompt has food. If it does not have food, return 'Give me some food to work with'."),
            _ => base_prompt,
        };

        // Apply verbosity suffix
        self.apply_verbosity_suffix(&with_modifier, verbosity)
    }

    /// Apply verbosity suffix to a prompt
    fn apply_verbosity_suffix(&self, prompt: &str, verbosity: &str) -> String {
        let suffix = match verbosity {
            "concise" => "\n\n## Response Style\nKeep responses brief and to the point. Aim for 2-3 sentences unless the topic truly requires more. If more detail might help, end with \"Want me to elaborate?\"",
            "detailed" => "\n\n## Response Style\nProvide comprehensive, detailed explanations. Include examples, context, and thorough coverage of the topic. The user wants depth.",
            _ => "", // "normal" gets no suffix - use base prompt as-is
        };

        if suffix.is_empty() {
            prompt.to_string()
        } else {
            format!("{prompt}{suffix}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persona_manager_creation() {
        let manager = PersonaManager::new();
        assert!(manager.get_persona("obi").is_some());
        assert!(manager.get_persona("muppet").is_some());
        assert!(manager.get_persona("chef").is_some());
        assert!(manager.get_persona("teacher").is_some());
        assert!(manager.get_persona("analyst").is_some());
        assert!(manager.get_persona("nonexistent").is_none());
    }

    #[test]
    fn test_get_system_prompt_with_modifiers() {
        let manager = PersonaManager::new();
        
        let base_prompt = manager.get_system_prompt("muppet", None);
        assert!(base_prompt.contains("warm, enthusiastic friend"));
        
        let explain_prompt = manager.get_system_prompt("muppet", Some("explain"));
        assert!(explain_prompt.contains("clear explanations"));
        
        let simple_prompt = manager.get_system_prompt("muppet", Some("simple"));
        assert!(simple_prompt.contains("analogies"));
        
        let steps_prompt = manager.get_system_prompt("muppet", Some("steps"));
        assert!(steps_prompt.contains("actionable steps"));
        
        let recipe_prompt = manager.get_system_prompt("muppet", Some("recipe"));
        assert!(recipe_prompt.contains("recipe"));
    }

    #[test]
    fn test_persona_descriptions() {
        let manager = PersonaManager::new();
        let personas = manager.list_personas();

        assert!(!personas.is_empty());
        for (_, persona) in personas {
            assert!(!persona.name.is_empty());
            assert!(!persona.description.is_empty());
            assert!(!persona.system_prompt.is_empty());
        }
    }

    #[test]
    fn test_obi_wan_prompt_loaded() {
        let manager = PersonaManager::new();
        let obi = manager.get_persona("obi").expect("obi persona should exist");

        // Verify the prompt contains Obi-Wan specific phrases
        assert!(obi.system_prompt.contains("Obi-Wan Kenobi"));
        assert!(obi.system_prompt.contains("certain point of view"));
        assert!(obi.system_prompt.contains("Philosophical"));
        assert!(obi.system_prompt.contains("Diplomatic Restraint"));
        assert!(obi.system_prompt.len() > 100, "Prompt should be substantial");
    }

    #[test]
    fn test_verbosity_suffix_concise() {
        let manager = PersonaManager::new();
        let prompt = manager.get_system_prompt_with_verbosity("muppet", None, "concise");
        assert!(prompt.contains("brief and to the point"));
        assert!(prompt.contains("2-3 sentences"));
        assert!(prompt.contains("Want me to elaborate?"));
    }

    #[test]
    fn test_verbosity_suffix_detailed() {
        let manager = PersonaManager::new();
        let prompt = manager.get_system_prompt_with_verbosity("muppet", None, "detailed");
        assert!(prompt.contains("comprehensive"));
        assert!(prompt.contains("Include examples"));
        assert!(prompt.contains("wants depth"));
    }

    #[test]
    fn test_verbosity_suffix_normal() {
        let manager = PersonaManager::new();
        let prompt_normal = manager.get_system_prompt_with_verbosity("muppet", None, "normal");
        let prompt_base = manager.get_system_prompt("muppet", None);
        // Normal should not add a suffix
        assert_eq!(prompt_normal, prompt_base);
    }

    #[test]
    fn test_verbosity_with_modifier() {
        let manager = PersonaManager::new();
        let prompt = manager.get_system_prompt_with_verbosity("muppet", Some("explain"), "concise");
        // Should have both modifier and verbosity
        assert!(prompt.contains("clear explanations"));
        assert!(prompt.contains("brief and to the point"));
    }
}