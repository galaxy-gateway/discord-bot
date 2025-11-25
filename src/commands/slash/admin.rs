//! Admin slash commands: /introspect, /settings, /set_channel_verbosity, /set_guild_setting, /admin_role, /features, /toggle, /sysinfo

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;
use serenity::model::permissions::Permissions;

/// Creates admin commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![
        create_introspect_command(),
        create_set_channel_verbosity_command(),
        create_set_guild_setting_command(),
        create_settings_command(),
        create_admin_role_command(),
        create_features_command(),
        create_toggle_command(),
        create_sysinfo_command(),
    ]
}

/// Creates the introspect command (admin) - lets personas explain their own code
fn create_introspect_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("introspect")
        .description("Let your persona explain their own implementation (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .create_option(|option| {
            option
                .name("component")
                .description("Which part of the bot to explain")
                .kind(CommandOptionType::String)
                .required(true)
                .add_string_choice("Overview - Bot architecture", "overview")
                .add_string_choice("Personas - Personality system", "personas")
                .add_string_choice("Reminders - Scheduling system", "reminders")
                .add_string_choice("Conflict - Mediation system", "conflict")
                .add_string_choice("Commands - How I process commands", "commands")
                .add_string_choice("Database - How I remember things", "database")
        })
        .to_owned()
}

/// Creates the set_channel_verbosity command (admin)
fn create_set_channel_verbosity_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("set_channel_verbosity")
        .description("Set the verbosity level for a channel (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .create_option(|option| {
            option
                .name("level")
                .description("The verbosity level")
                .kind(CommandOptionType::String)
                .required(true)
                .add_string_choice("concise", "concise")
                .add_string_choice("normal", "normal")
                .add_string_choice("detailed", "detailed")
        })
        .create_option(|option| {
            option
                .name("channel")
                .description("Target channel (defaults to current channel)")
                .kind(CommandOptionType::Channel)
                .required(false)
        })
        .to_owned()
}

/// Creates the set_guild_setting command (admin)
fn create_set_guild_setting_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("set_guild_setting")
        .description("Set a guild-wide bot setting (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .create_option(|option| {
            option
                .name("setting")
                .description("The setting to change")
                .kind(CommandOptionType::String)
                .required(true)
                // High priority settings
                .add_string_choice("default_verbosity", "default_verbosity")
                .add_string_choice("default_persona", "default_persona")
                .add_string_choice("conflict_mediation", "conflict_mediation")
                .add_string_choice("conflict_sensitivity", "conflict_sensitivity")
                .add_string_choice("mediation_cooldown", "mediation_cooldown")
                // Medium priority settings
                .add_string_choice("max_context_messages", "max_context_messages")
                .add_string_choice("audio_transcription", "audio_transcription")
                .add_string_choice("audio_transcription_mode", "audio_transcription_mode")
                .add_string_choice("audio_transcription_output", "audio_transcription_output")
                .add_string_choice("mention_responses", "mention_responses")
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

/// Creates the settings command (admin)
fn create_settings_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("settings")
        .description("View current bot settings for this guild and channel (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .to_owned()
}

/// Creates the admin_role command (Discord admin only)
fn create_admin_role_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("admin_role")
        .description("Set which role can manage bot settings (Server Admin only)")
        .default_member_permissions(Permissions::ADMINISTRATOR)
        .create_option(|option| {
            option
                .name("role")
                .description("The role to grant bot management permissions")
                .kind(CommandOptionType::Role)
                .required(true)
        })
        .to_owned()
}

/// Creates the features command (admin) - shows all features with status
fn create_features_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("features")
        .description("List all bot features with their versions and toggle status (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .to_owned()
}

/// Creates the toggle command (admin) - enables/disables toggleable features
fn create_toggle_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("toggle")
        .description("Enable or disable a toggleable feature for this server (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .create_option(|option| {
            option
                .name("feature")
                .description("The feature to toggle")
                .kind(CommandOptionType::String)
                .required(true)
                // Add choices for toggleable features
                .add_string_choice("Reminders", "reminders")
                .add_string_choice("Conflict Detection", "conflict_detection")
                .add_string_choice("Conflict Mediation", "conflict_mediation")
                .add_string_choice("Image Generation", "image_generation")
                .add_string_choice("Audio Transcription", "audio_transcription")
        })
        .to_owned()
}

/// Creates the sysinfo command (admin) - displays system diagnostics and metrics
fn create_sysinfo_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("sysinfo")
        .description("Display system information, bot diagnostics, and resource history (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .create_option(|option| {
            option
                .name("view")
                .description("What information to display")
                .kind(CommandOptionType::String)
                .required(false)
                .add_string_choice("Current Status", "current")
                .add_string_choice("History (24h)", "history_24h")
                .add_string_choice("History (7d)", "history_7d")
        })
        .to_owned()
}
