//! Persona slash commands: /personas, /set_persona

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Creates persona commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_personas_command(), create_set_persona_command()]
}

/// Creates the personas command
fn create_personas_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("personas")
        .description("List all available personas and show your current one")
        .to_owned()
}

/// Creates the set_persona command
fn create_set_persona_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("set_persona")
        .description("Set your default persona")
        .create_option(|option| {
            option
                .name("persona")
                .description("The persona to set as your default")
                .kind(CommandOptionType::String)
                .required(true)
                .add_string_choice("muppet", "muppet")
                .add_string_choice("chef", "chef")
                .add_string_choice("obi", "obi")
                .add_string_choice("teacher", "teacher")
                .add_string_choice("analyst", "analyst")
                .add_string_choice("visionary", "visionary")
        })
        .to_owned()
}
