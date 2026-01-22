//! Utility slash commands: /ping, /help, /forget, /status, /version, /uptime, /commits

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Creates utility commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![
        create_ping_command(),
        create_help_command(),
        create_forget_command(),
        create_status_command(),
        create_version_command(),
        create_uptime_command(),
        create_commits_command(),
    ]
}

/// Creates the ping command
fn create_ping_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("ping")
        .description("Test bot responsiveness")
        .to_owned()
}

/// Creates the help command
fn create_help_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("help")
        .description("Show available commands and usage information")
        .to_owned()
}

/// Creates the forget command
fn create_forget_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("forget")
        .description("Clear your conversation history with the bot")
        .to_owned()
}

/// Creates the status command
fn create_status_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("status")
        .description("Show bot status, uptime, and system information")
        .to_owned()
}

/// Creates the version command
fn create_version_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("version")
        .description("Show bot version and feature versions")
        .to_owned()
}

/// Creates the uptime command
fn create_uptime_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("uptime")
        .description("Show how long the bot has been running")
        .to_owned()
}

/// Creates the commits command
fn create_commits_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("commits")
        .description("Show recent git commits for this bot")
        .create_option(|option| {
            option
                .name("count")
                .description("Number of commits to show (1-10, default: 1)")
                .kind(CommandOptionType::Integer)
                .min_int_value(1)
                .max_int_value(10)
                .required(false)
        })
        .to_owned()
}
