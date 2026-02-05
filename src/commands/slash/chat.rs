//! Chat/AI slash commands: /hey, /explain, /simple, /steps

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Creates chat/AI commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![
        create_hey_command(),
        create_explain_command(),
        create_simple_command(),
        create_steps_command(),
    ]
}

/// Creates the hey command
fn create_hey_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("hey")
        .description("Chat with your current persona")
        .create_option(|option| {
            option
                .name("message")
                .description("Your message to the persona")
                .kind(CommandOptionType::String)
                .required(true)
        })
        .create_option(|option| {
            option
                .name("paragraphs")
                .description("Maximum paragraphs in response (0=unlimited, 1-10)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(0)
                .max_int_value(10)
        })
        .to_owned()
}

/// Creates the explain command
fn create_explain_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("explain")
        .description("Get a detailed explanation from your persona")
        .create_option(|option| {
            option
                .name("topic")
                .description("What you want explained")
                .kind(CommandOptionType::String)
                .required(true)
        })
        .create_option(|option| {
            option
                .name("paragraphs")
                .description("Maximum paragraphs in response (0=unlimited, 1-10)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(0)
                .max_int_value(10)
        })
        .to_owned()
}

/// Creates the simple command
fn create_simple_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("simple")
        .description("Get a simple explanation with analogies")
        .create_option(|option| {
            option
                .name("topic")
                .description("What you want explained simply")
                .kind(CommandOptionType::String)
                .required(true)
        })
        .create_option(|option| {
            option
                .name("paragraphs")
                .description("Maximum paragraphs in response (0=unlimited, 1-10)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(0)
                .max_int_value(10)
        })
        .to_owned()
}

/// Creates the steps command
fn create_steps_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("steps")
        .description("Break something down into clear, actionable steps")
        .create_option(|option| {
            option
                .name("task")
                .description("What you want broken down into steps")
                .kind(CommandOptionType::String)
                .required(true)
        })
        .create_option(|option| {
            option
                .name("paragraphs")
                .description("Maximum paragraphs in response (0=unlimited, 1-10)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(0)
                .max_int_value(10)
        })
        .to_owned()
}
