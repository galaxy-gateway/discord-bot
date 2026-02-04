//! # Debate Command
//!
//! Creates a threaded debate between two personas on a given topic.
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.27.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Default number of responses in a debate (3 exchanges per persona)
pub const DEFAULT_RESPONSES: i64 = 6;

/// Maximum allowed responses to prevent runaway costs
pub const MAX_RESPONSES: i64 = 20;

/// Minimum responses (at least one exchange each)
pub const MIN_RESPONSES: i64 = 2;

/// All available persona choices for debate
const PERSONA_CHOICES: &[(&str, &str)] = &[
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
];

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
                .description("Number of responses (default: 6, max: 20)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(MIN_RESPONSES as u64)
                .max_int_value(MAX_RESPONSES as u64)
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
        // Ensure all 12 personas are available
        assert_eq!(PERSONA_CHOICES.len(), 12);
    }

    #[test]
    fn test_response_limits() {
        assert!(DEFAULT_RESPONSES >= MIN_RESPONSES);
        assert!(DEFAULT_RESPONSES <= MAX_RESPONSES);
        assert!(MIN_RESPONSES >= 2); // At least one exchange
    }
}
