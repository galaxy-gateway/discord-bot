//! Persona slash commands: /personas, /set_user

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Creates persona commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_personas_command(), create_set_user_command()]
}

/// Creates the personas command
fn create_personas_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("personas")
        .description("List all available personas and show your current one")
        .to_owned()
}

/// Creates the set_user command - unified user settings
fn create_set_user_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("set_user")
        .description("Set your personal bot preferences")
        .create_option(|option| {
            option
                .name("setting")
                .description("The setting to change")
                .kind(CommandOptionType::String)
                .required(true)
                .add_string_choice("persona", "persona")
        })
        .create_option(|option| {
            option
                .name("value")
                .description("The value to set")
                .kind(CommandOptionType::String)
                .required(true)
                .set_autocomplete(true)
        })
        .to_owned()
}
