//! # Council Command
//!
//! Gather responses from multiple personas on a single prompt.
//!
//! - **Version**: 2.1.0
//! - **Since**: 3.31.0
//!
//! ## Changelog
//! - 2.1.0: Use shared PERSONA_CHOICES from personas::choices
//! - 2.0.0: Added rules parameter and interactive controls
//! - 1.0.0: Initial implementation

use crate::features::personas::PERSONA_CHOICES;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Minimum number of personas for a council (at least 2 for meaningful discussion)
pub const MIN_PERSONAS: usize = 2;

/// Maximum number of personas for a council (to manage API costs and thread length)
pub const MAX_PERSONAS: usize = 6;

pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_council_command()]
}

fn create_council_command() -> CreateApplicationCommand {
    let mut command = CreateApplicationCommand::default();
    command
        .name("council")
        .description("Gather responses from multiple personas on a single prompt")
        .create_option(|option| {
            option
                .name("prompt")
                .description("The question or topic for the council")
                .kind(CommandOptionType::String)
                .required(true)
                .min_length(5)
                .max_length(1000)
        })
        .create_option(|option| {
            option
                .name("persona1")
                .description("First council member (required)")
                .kind(CommandOptionType::String)
                .required(true);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("persona2")
                .description("Second council member (required)")
                .kind(CommandOptionType::String)
                .required(true);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("persona3")
                .description("Third council member (optional)")
                .kind(CommandOptionType::String)
                .required(false);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("persona4")
                .description("Fourth council member (optional)")
                .kind(CommandOptionType::String)
                .required(false);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("persona5")
                .description("Fifth council member (optional)")
                .kind(CommandOptionType::String)
                .required(false);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("persona6")
                .description("Sixth council member (optional)")
                .kind(CommandOptionType::String)
                .required(false);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("rules")
                .description("Ground rules and term definitions for the discussion")
                .kind(CommandOptionType::String)
                .required(false)
                .max_length(1000)
        });
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_council_command() {
        let commands = create_commands();
        assert_eq!(commands.len(), 1);

        let council = &commands[0];
        let name = council.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "council");
    }

    #[test]
    fn test_persona_choices_complete() {
        // Ensure all 17 personas are available
        assert_eq!(PERSONA_CHOICES.len(), 17);
    }

    #[test]
    fn test_council_limits() {
        assert!(MIN_PERSONAS >= 2, "Council needs at least 2 personas");
        assert!(MAX_PERSONAS <= 17, "Cannot exceed available personas");
        assert!(MAX_PERSONAS >= MIN_PERSONAS);
    }
}
