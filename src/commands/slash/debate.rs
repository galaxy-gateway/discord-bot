//! # Debate Command
//!
//! Creates a threaded debate between two personas on a given topic.
//!
//! - **Version**: 2.1.0
//! - **Since**: 3.27.0
//!
//! ## Changelog
//! - 2.1.0: Use shared PERSONA_CHOICES from personas::choices
//! - 2.0.0: Added rules parameter, opening-only default, interactive controls
//! - 1.0.0: Initial implementation

use crate::features::personas::PERSONA_CHOICES;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Default number of responses in a debate (opening statements only)
pub const DEFAULT_RESPONSES: i64 = 2;

/// Maximum allowed responses to prevent runaway costs
pub const MAX_RESPONSES: i64 = 20;

/// Minimum responses (0 means opening statements only, controlled interactively)
pub const MIN_RESPONSES: i64 = 0;

pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_debate_command()]
}

fn create_debate_command() -> CreateApplicationCommand {
    let mut command = CreateApplicationCommand::default();
    command
        .name("debate")
        .description("Start a threaded debate between two personas on a topic")
        .create_option(|option| {
            option
                .name("persona1")
                .description("First debater")
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
                .description("Second debater")
                .kind(CommandOptionType::String)
                .required(true);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("topic")
                .description("The topic or question to debate")
                .kind(CommandOptionType::String)
                .required(true)
                .min_length(5)
                .max_length(500)
        })
        .create_option(|option| {
            option
                .name("rounds")
                .description("Number of responses (default: 2 opening, 0 for interactive only)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(MIN_RESPONSES as u64)
                .max_int_value(MAX_RESPONSES as u64)
        })
        .create_option(|option| {
            option
                .name("rules")
                .description("Ground rules and term definitions for the debate")
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
    fn test_create_debate_command() {
        let commands = create_commands();
        assert_eq!(commands.len(), 1);

        let debate = &commands[0];
        let name = debate.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "debate");
    }

    #[test]
    fn test_persona_choices_complete() {
        // Ensure all 17 personas are available
        assert_eq!(PERSONA_CHOICES.len(), 17);
    }

    #[test]
    fn test_response_limits() {
        assert!(DEFAULT_RESPONSES >= MIN_RESPONSES);
        assert!(DEFAULT_RESPONSES <= MAX_RESPONSES);
        assert!(MIN_RESPONSES == 0); // Opening only can be 0 for interactive mode
        assert!(DEFAULT_RESPONSES == 2); // Default is opening statements only
    }
}
