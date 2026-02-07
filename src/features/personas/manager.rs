//! # Feature: Persona System
//!
//! Multi-personality AI responses with 17 distinct personas (obi, muppet, chef, teacher, analyst, visionary,
//! noir, zen, bard, coach, scientist, gamer, architect, debugger, reviewer, devops, designer).
//! Each persona has a unique system prompt loaded from prompt/*.md files at compile time.
//!
//! - **Version**: 1.6.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.6.0: Added 5 software development personas - architect, debugger, reviewer, devops, designer
//! - 1.5.0: Added SVG portrait assets and portrait URL generation
//! - 1.4.0: Added embed responses with persona colors and optional portrait support
//! - 1.3.0: Added 6 new personas - noir, zen, bard, coach, scientist, gamer
//! - 1.2.0: Added channel-level persona override via /set_channel_setting persona
//! - 1.1.0: Added visionary persona - a future-focused big-picture thinker
//! - 1.0.0: Initial release with 5 personas and verbosity modifiers

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub name: String,
    pub system_prompt: String,
    pub description: String,
    /// Optional portrait URL for embed author icon
    pub portrait_url: Option<String>,
    /// Embed accent color (Discord color format)
    pub color: u32,
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
        // Colors chosen to reflect each persona's personality/theme
        personas.insert("obi".to_string(), Persona {
            name: "Obi-Wan".to_string(),
            system_prompt: include_str!("../../../prompt/obi.md").to_string(),
            description: "A wise Jedi Master who speaks with patience, diplomacy, and philosophical insight".to_string(),
            portrait_url: None,
            color: 0x4A90D9, // Calm blue - Jedi wisdom
        });

        personas.insert("muppet".to_string(), Persona {
            name: "Muppet Friend".to_string(),
            system_prompt: include_str!("../../../prompt/muppet.md").to_string(),
            description: "A warm, enthusiastic friend who brings Muppet-style joy, humor, and heart to every conversation!".to_string(),
            portrait_url: None,
            color: 0xFF6B35, // Warm orange - Muppet energy
        });

        personas.insert(
            "chef".to_string(),
            Persona {
                name: "Chef".to_string(),
                system_prompt: include_str!("../../../prompt/chef.md").to_string(),
                description: "A passionate chef who shares recipes and cooking wisdom".to_string(),
                portrait_url: None,
                color: 0xE74C3C, // Culinary red
            },
        );

        personas.insert(
            "teacher".to_string(),
            Persona {
                name: "Teacher".to_string(),
                system_prompt: include_str!("../../../prompt/teacher.md").to_string(),
                description: "A patient teacher who explains things clearly".to_string(),
                portrait_url: None,
                color: 0x27AE60, // Educational green
            },
        );

        personas.insert(
            "analyst".to_string(),
            Persona {
                name: "Step-by-Step Analyst".to_string(),
                system_prompt: include_str!("../../../prompt/analyst.md").to_string(),
                description: "An analyst who breaks things down into clear steps".to_string(),
                portrait_url: None,
                color: 0x3498DB, // Professional blue
            },
        );

        personas.insert("visionary".to_string(), Persona {
            name: "The Visionary".to_string(),
            system_prompt: include_str!("../../../prompt/visionary.md").to_string(),
            description: "A future-focused big-picture thinker who transforms chaos into actionable plans".to_string(),
            portrait_url: None,
            color: 0x9B59B6, // Futuristic purple
        });

        personas.insert(
            "noir".to_string(),
            Persona {
                name: "Noir Detective".to_string(),
                system_prompt: include_str!("../../../prompt/noir.md").to_string(),
                description:
                    "A hard-boiled 1940s detective who treats every question like a case to crack"
                        .to_string(),
                portrait_url: None,
                color: 0x2C3E50, // Dark noir gray
            },
        );

        personas.insert(
            "zen".to_string(),
            Persona {
                name: "Zen Master".to_string(),
                system_prompt: include_str!("../../../prompt/zen.md").to_string(),
                description: "A contemplative sage who brings calm wisdom and mindful perspective"
                    .to_string(),
                portrait_url: None,
                color: 0x1ABC9C, // Peaceful teal
            },
        );

        personas.insert(
            "bard".to_string(),
            Persona {
                name: "The Bard".to_string(),
                system_prompt: include_str!("../../../prompt/bard.md").to_string(),
                description:
                    "A charismatic storyteller who weaves narrative magic into every conversation"
                        .to_string(),
                portrait_url: None,
                color: 0xF39C12, // Fantasy gold
            },
        );

        personas.insert(
            "coach".to_string(),
            Persona {
                name: "The Coach".to_string(),
                system_prompt: include_str!("../../../prompt/coach.md").to_string(),
                description:
                    "A motivational coach who helps you get in the game and reach your potential"
                        .to_string(),
                portrait_url: None,
                color: 0xE67E22, // Energetic orange
            },
        );

        personas.insert(
            "scientist".to_string(),
            Persona {
                name: "The Scientist".to_string(),
                system_prompt: include_str!("../../../prompt/scientist.md").to_string(),
                description: "A curious researcher who loves explaining how things work"
                    .to_string(),
                portrait_url: None,
                color: 0x00CED1, // Scientific cyan
            },
        );

        personas.insert(
            "gamer".to_string(),
            Persona {
                name: "The Gamer".to_string(),
                system_prompt: include_str!("../../../prompt/gamer.md").to_string(),
                description: "A friendly gamer who speaks the language of gaming culture"
                    .to_string(),
                portrait_url: None,
                color: 0x9146FF, // Twitch purple
            },
        );

        // Software Development & Design Personas
        personas.insert(
            "architect".to_string(),
            Persona {
                name: "The Architect".to_string(),
                system_prompt: include_str!("../../../prompt/architect.md").to_string(),
                description:
                    "A systems thinker who designs for scale, trade-offs, and the long game"
                        .to_string(),
                portrait_url: None,
                color: 0x34495E, // Blueprint slate
            },
        );

        personas.insert(
            "debugger".to_string(),
            Persona {
                name: "The Debugger".to_string(),
                system_prompt: include_str!("../../../prompt/debugger.md").to_string(),
                description:
                    "A tenacious bug hunter who tracks down root causes with methodical precision"
                        .to_string(),
                portrait_url: None,
                color: 0xC0392B, // Error red
            },
        );

        personas.insert(
            "reviewer".to_string(),
            Persona {
                name: "The Reviewer".to_string(),
                system_prompt: include_str!("../../../prompt/reviewer.md").to_string(),
                description:
                    "A thoughtful code reviewer who makes code better and helps developers grow"
                        .to_string(),
                portrait_url: None,
                color: 0x27AE60, // Approval green
            },
        );

        personas.insert(
            "devops".to_string(),
            Persona {
                name: "The DevOps".to_string(),
                system_prompt: include_str!("../../../prompt/devops.md").to_string(),
                description:
                    "An automation expert who makes deployments boring and systems observable"
                        .to_string(),
                portrait_url: None,
                color: 0x2980B9, // Pipeline blue
            },
        );

        personas.insert(
            "designer".to_string(),
            Persona {
                name: "The Designer".to_string(),
                system_prompt: include_str!("../../../prompt/designer.md").to_string(),
                description: "A UX advocate who designs with empathy, clarity, and accessibility"
                    .to_string(),
                portrait_url: None,
                color: 0xE91E63, // Creative pink
            },
        );

        PersonaManager { personas }
    }

    pub fn get_persona(&self, name: &str) -> Option<&Persona> {
        self.personas.get(name)
    }

    pub fn list_personas(&self) -> Vec<(&String, &Persona)> {
        self.personas.iter().collect()
    }

    /// Get the portrait URL for a persona.
    ///
    /// Uses the persona's custom portrait_url if set, otherwise generates one
    /// from PERSONA_PORTRAIT_BASE_URL environment variable.
    ///
    /// Example base URLs:
    /// - GitHub raw: https://raw.githubusercontent.com/user/repo/main/assets/personas
    /// - GitHub pages: https://user.github.io/repo/assets/personas
    /// - Custom CDN: https://cdn.example.com/personas
    pub fn get_portrait_url(&self, persona_id: &str) -> Option<String> {
        // First check if persona has a custom portrait URL
        if let Some(persona) = self.personas.get(persona_id) {
            if persona.portrait_url.is_some() {
                return persona.portrait_url.clone();
            }
        }

        // Generate URL from base URL if configured
        if let Ok(base_url) = env::var("PERSONA_PORTRAIT_BASE_URL") {
            let base = base_url.trim_end_matches('/');
            Some(format!("{base}/{persona_id}.png"))
        } else {
            None
        }
    }

    /// Get a persona with its portrait URL resolved
    pub fn get_persona_with_portrait(&self, name: &str) -> Option<Persona> {
        self.personas.get(name).map(|p| {
            let mut persona = p.clone();
            if persona.portrait_url.is_none() {
                persona.portrait_url = self.get_portrait_url(name);
            }
            persona
        })
    }

    pub fn get_system_prompt(&self, persona_name: &str, modifier: Option<&str>) -> String {
        self.get_system_prompt_with_verbosity(persona_name, modifier, "normal")
    }

    /// Get system prompt with verbosity level applied
    pub fn get_system_prompt_with_verbosity(
        &self,
        persona_name: &str,
        modifier: Option<&str>,
        verbosity: &str,
    ) -> String {
        let base_prompt = self
            .personas
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

/// Apply paragraph limit to system prompt.
/// 0 = no limit (returns prompt unchanged), 1-10 = enforced limit
pub fn apply_paragraph_limit(prompt: &str, max_paragraphs: i64) -> String {
    if max_paragraphs > 0 {
        format!(
            "{prompt}\n\nIMPORTANT: Limit your response to {max_paragraphs} paragraph(s) maximum."
        )
    } else {
        prompt.to_string()
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
        assert!(manager.get_persona("visionary").is_some());
        assert!(manager.get_persona("noir").is_some());
        assert!(manager.get_persona("zen").is_some());
        assert!(manager.get_persona("bard").is_some());
        assert!(manager.get_persona("coach").is_some());
        assert!(manager.get_persona("scientist").is_some());
        assert!(manager.get_persona("gamer").is_some());
        // Software development personas
        assert!(manager.get_persona("architect").is_some());
        assert!(manager.get_persona("debugger").is_some());
        assert!(manager.get_persona("reviewer").is_some());
        assert!(manager.get_persona("devops").is_some());
        assert!(manager.get_persona("designer").is_some());
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
            assert!(persona.color != 0, "Persona should have a color set");
        }
    }

    #[test]
    fn test_persona_colors() {
        let manager = PersonaManager::new();

        // Verify each persona has a unique, non-zero color
        let obi = manager.get_persona("obi").unwrap();
        assert_eq!(obi.color, 0x4A90D9);

        let visionary = manager.get_persona("visionary").unwrap();
        assert_eq!(visionary.color, 0x9B59B6);

        let gamer = manager.get_persona("gamer").unwrap();
        assert_eq!(gamer.color, 0x9146FF);
    }

    #[test]
    fn test_obi_wan_prompt_loaded() {
        let manager = PersonaManager::new();
        let obi = manager
            .get_persona("obi")
            .expect("obi persona should exist");

        // Verify the prompt contains Obi-Wan specific phrases
        assert!(obi.system_prompt.contains("Obi-Wan Kenobi"));
        assert!(obi.system_prompt.contains("certain point of view"));
        assert!(obi.system_prompt.contains("Philosophical"));
        assert!(obi.system_prompt.contains("Diplomatic Restraint"));
        assert!(
            obi.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
    }

    #[test]
    fn test_visionary_prompt_loaded() {
        let manager = PersonaManager::new();
        let visionary = manager
            .get_persona("visionary")
            .expect("visionary persona should exist");

        // Verify the prompt contains Visionary specific phrases
        assert!(visionary.system_prompt.contains("The Visionary"));
        assert!(visionary.system_prompt.contains("big-picture"));
        assert!(visionary.system_prompt.contains("Future-Focused"));
        assert!(visionary.system_prompt.contains("Transformation Energy"));
        assert!(visionary.system_prompt.contains("Hardcore Intensity"));
        assert!(
            visionary.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
    }

    #[test]
    fn test_architect_prompt_loaded() {
        let manager = PersonaManager::new();
        let architect = manager
            .get_persona("architect")
            .expect("architect persona should exist");

        assert!(architect.system_prompt.contains("The Architect"));
        assert!(architect.system_prompt.contains("Systems Thinker"));
        assert!(architect.system_prompt.contains("trade-offs"));
        assert!(
            architect.system_prompt.contains("scalability")
                || architect.system_prompt.contains("scale")
        );
        assert!(
            architect.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
    }

    #[test]
    fn test_debugger_prompt_loaded() {
        let manager = PersonaManager::new();
        let debugger = manager
            .get_persona("debugger")
            .expect("debugger persona should exist");

        assert!(debugger.system_prompt.contains("The Debugger"));
        assert!(debugger.system_prompt.contains("root cause"));
        assert!(debugger.system_prompt.contains("Evidence-Driven"));
        assert!(
            debugger.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
    }

    #[test]
    fn test_reviewer_prompt_loaded() {
        let manager = PersonaManager::new();
        let reviewer = manager
            .get_persona("reviewer")
            .expect("reviewer persona should exist");

        assert!(reviewer.system_prompt.contains("The Reviewer"));
        assert!(reviewer.system_prompt.contains("code review"));
        assert!(reviewer.system_prompt.contains("Constructively Critical"));
        assert!(
            reviewer.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
    }

    #[test]
    fn test_devops_prompt_loaded() {
        let manager = PersonaManager::new();
        let devops = manager
            .get_persona("devops")
            .expect("devops persona should exist");

        assert!(devops.system_prompt.contains("The DevOps"));
        assert!(devops.system_prompt.contains("Automation"));
        assert!(
            devops.system_prompt.contains("CI/CD") || devops.system_prompt.contains("pipeline")
        );
        assert!(
            devops.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
    }

    #[test]
    fn test_designer_prompt_loaded() {
        let manager = PersonaManager::new();
        let designer = manager
            .get_persona("designer")
            .expect("designer persona should exist");

        assert!(designer.system_prompt.contains("The Designer"));
        assert!(
            designer.system_prompt.contains("UX")
                || designer.system_prompt.contains("user experience")
        );
        assert!(designer.system_prompt.contains("Accessibility"));
        assert!(
            designer.system_prompt.len() > 100,
            "Prompt should be substantial"
        );
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

    #[test]
    fn test_get_portrait_url_without_base() {
        // Clear any existing base URL
        env::remove_var("PERSONA_PORTRAIT_BASE_URL");

        let manager = PersonaManager::new();
        // Without base URL configured, should return None
        assert!(manager.get_portrait_url("obi").is_none());
    }

    #[test]
    fn test_get_portrait_url_with_base() {
        env::set_var("PERSONA_PORTRAIT_BASE_URL", "https://example.com/portraits");

        let manager = PersonaManager::new();
        let url = manager.get_portrait_url("obi");

        assert!(url.is_some());
        assert_eq!(url.unwrap(), "https://example.com/portraits/obi.png");

        env::remove_var("PERSONA_PORTRAIT_BASE_URL");
    }

    #[test]
    fn test_get_portrait_url_trailing_slash() {
        env::set_var(
            "PERSONA_PORTRAIT_BASE_URL",
            "https://example.com/portraits/",
        );

        let manager = PersonaManager::new();
        let url = manager.get_portrait_url("muppet");

        assert!(url.is_some());
        // Should not have double slashes
        assert_eq!(url.unwrap(), "https://example.com/portraits/muppet.png");

        env::remove_var("PERSONA_PORTRAIT_BASE_URL");
    }

    #[test]
    fn test_get_persona_with_portrait() {
        env::set_var("PERSONA_PORTRAIT_BASE_URL", "https://cdn.example.com/img");

        let manager = PersonaManager::new();
        let persona = manager.get_persona_with_portrait("chef");

        assert!(persona.is_some());
        let p = persona.unwrap();
        assert_eq!(p.name, "Chef");
        assert!(p.portrait_url.is_some());
        assert_eq!(
            p.portrait_url.unwrap(),
            "https://cdn.example.com/img/chef.png"
        );

        env::remove_var("PERSONA_PORTRAIT_BASE_URL");
    }

    #[test]
    fn test_apply_paragraph_limit_no_limit() {
        let prompt = "Test prompt";
        let result = apply_paragraph_limit(prompt, 0);
        assert_eq!(result, "Test prompt");
    }

    #[test]
    fn test_apply_paragraph_limit_with_limit() {
        let prompt = "Test prompt";
        let result = apply_paragraph_limit(prompt, 3);
        assert!(result.contains("Test prompt"));
        assert!(result.contains("IMPORTANT: Limit your response to 3 paragraph(s) maximum."));
    }

    #[test]
    fn test_apply_paragraph_limit_single_paragraph() {
        let prompt = "Test prompt";
        let result = apply_paragraph_limit(prompt, 1);
        assert!(result.contains("IMPORTANT: Limit your response to 1 paragraph(s) maximum."));
    }
}
