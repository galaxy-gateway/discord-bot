//! DM statistics slash commands

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Creates DM stats commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![
        create_dm_stats_command(),
        create_session_history_command(),
    ]
}

/// Creates the dm_stats command
fn create_dm_stats_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("dm_stats")
        .description("View your DM interaction statistics")
        .create_option(|option| {
            option
                .name("period")
                .description("Time period for statistics")
                .kind(CommandOptionType::String)
                .required(false)
                .add_string_choice("Today", "today")
                .add_string_choice("This Week", "week")
                .add_string_choice("This Month", "month")
                .add_string_choice("All Time", "all")
        })
        .to_owned()
}

/// Creates the session_history command
fn create_session_history_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("session_history")
        .description("View your recent DM sessions")
        .create_option(|option| {
            option
                .name("limit")
                .description("Number of sessions to show (1-20)")
                .kind(CommandOptionType::Integer)
                .required(false)
                .min_int_value(1)
                .max_int_value(20)
        })
        .to_owned()
}
