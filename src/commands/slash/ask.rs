//! # Ask Command
//!
//! Request a response from any persona with a custom prompt.
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.30.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// All available persona choices for the ask command
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
    vec![create_ask_command()]
}

fn create_ask_command() -> CreateApplicationCommand {
    let mut command = CreateApplicationCommand::default();
    command
        .name("ask")
        .description("Ask any persona a question with optional context")
        .create_option(|option| {
            option
                .name("persona")
                .description("The persona to respond")
                .kind(CommandOptionType::String)
                .required(true);
            for (name, value) in PERSONA_CHOICES {
                option.add_string_choice(name, value);
            }
            option
        })
        .create_option(|option| {
            option
                .name("prompt")
                .description("Your question or prompt for the persona")
                .kind(CommandOptionType::String)
                .required(true)
                .min_length(1)
                .max_length(2000)
        })
        .create_option(|option| {
            option
                .name("ignore_context")
                .description("Skip fetching channel/thread history (default: false)")
                .kind(CommandOptionType::Boolean)
                .required(false)
        });
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ask_command() {
        let commands = create_commands();
        assert_eq!(commands.len(), 1);

        let ask = &commands[0];
        let name = ask.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "ask");
    }

    #[test]
    fn test_persona_choices_complete() {
        // Ensure all 12 personas are available
        assert_eq!(PERSONA_CHOICES.len(), 12);
    }
}
