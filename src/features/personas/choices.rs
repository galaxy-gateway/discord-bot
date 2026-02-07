//! Shared persona choices for slash commands
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from duplicated constants in ask.rs, debate.rs, council.rs

use serenity::builder::CreateApplicationCommandOption;

/// All available persona choices (display_name, id)
pub const PERSONA_CHOICES: &[(&str, &str)] = &[
    ("Obi-Wan", "obi"),
    ("Muppet Friend", "muppet"),
    ("Chef", "chef"),
    ("Teacher", "teacher"),
    ("Analyst", "analyst"),
    ("Visionary", "visionary"),
    ("Noir Detective", "noir"),
    ("Zen Master", "zen"),
    ("Bard", "bard"),
    ("Coach", "coach"),
    ("Scientist", "scientist"),
    ("Gamer", "gamer"),
    ("Architect", "architect"),
    ("Debugger", "debugger"),
    ("Reviewer", "reviewer"),
    ("DevOps", "devops"),
    ("Designer", "designer"),
];

/// Add all persona choices to a command option builder
pub fn add_persona_choices(option: &mut CreateApplicationCommandOption) {
    for (name, value) in PERSONA_CHOICES {
        option.add_string_choice(name, value);
    }
}

/// Validate a persona ID exists
pub fn is_valid_persona(id: &str) -> bool {
    PERSONA_CHOICES.iter().any(|(_, pid)| *pid == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persona_choices_complete() {
        assert_eq!(PERSONA_CHOICES.len(), 17);
    }

    #[test]
    fn test_is_valid_persona() {
        assert!(is_valid_persona("obi"));
        assert!(is_valid_persona("designer"));
        assert!(!is_valid_persona("invalid"));
    }

    #[test]
    fn test_all_personas_have_unique_ids() {
        let mut ids: Vec<&str> = PERSONA_CHOICES.iter().map(|(_, id)| *id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PERSONA_CHOICES.len(), "Duplicate persona IDs found");
    }
}
