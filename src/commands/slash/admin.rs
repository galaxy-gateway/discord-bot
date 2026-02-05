//! Admin slash commands: /introspect, /settings, /set_channel, /set_guild, /admin_role, /features, /toggle, /sysinfo, /usage

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;
use serenity::model::permissions::Permissions;

/// Creates admin commands
pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![
        create_introspect_command(),
        create_set_channel_command(),
        create_set_guild_command(),
        create_settings_command(),
        create_admin_role_command(),
        create_features_command(),
        create_toggle_command(),
        create_sysinfo_command(),
        create_usage_command(),
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

/// Creates the set_channel command (admin) - unified channel settings
fn create_set_channel_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("set_channel")
        .description("Set a channel-specific bot setting (Admin)")
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .create_option(|option| {
            option
                .name("setting")
                .description("The setting to change")
                .kind(CommandOptionType::String)
                .required(true)
                .add_string_choice("verbosity", "verbosity")
                .add_string_choice("persona", "persona")
                .add_string_choice("conflict_mediation", "conflict_mediation")
        })
        .create_option(|option| {
            option
                .name("value")
                .description("The value to set")
                .kind(CommandOptionType::String)
                .required(true)
                .set_autocomplete(true)
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

/// Creates the set_guild command (admin)
fn create_set_guild_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("set_guild")
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
                .add_string_choice("response_embeds", "response_embeds")
                .add_string_choice("conflict_mediation", "conflict_mediation")
                .add_string_choice("conflict_sensitivity", "conflict_sensitivity")
                .add_string_choice("mediation_cooldown", "mediation_cooldown")
                // Medium priority settings
                .add_string_choice("max_context_messages", "max_context_messages")
                .add_string_choice("audio_transcription", "audio_transcription")
                .add_string_choice("audio_transcription_mode", "audio_transcription_mode")
                .add_string_choice("audio_transcription_output", "audio_transcription_output")
                .add_string_choice("mention_responses", "mention_responses")
                .add_string_choice("debate_auto_response", "debate_auto_response")
                // Global bot settings (stored in bot_settings table)
                .add_string_choice("startup_notification", "startup_notification")
                .add_string_choice("startup_notify_owner_id", "startup_notify_owner_id")
                .add_string_choice("startup_notify_channel_id", "startup_notify_channel_id")
                .add_string_choice("startup_dm_commit_count", "startup_dm_commit_count")
                .add_string_choice(
                    "startup_channel_commit_count",
                    "startup_channel_commit_count",
                )
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

/// Creates the usage command - displays OpenAI API usage and cost metrics
fn create_usage_command() -> CreateApplicationCommand {
    CreateApplicationCommand::default()
        .name("usage")
        .description("View OpenAI API usage and cost metrics")
        .create_option(|option| {
            option
                .name("scope")
                .description("What usage to display")
                .kind(CommandOptionType::String)
                .required(false)
                .add_string_choice("My Usage (Today)", "personal_today")
                .add_string_choice("My Usage (7 days)", "personal_7d")
                .add_string_choice("Server Usage (Today) - Admin", "server_today")
                .add_string_choice("Server Usage (7 days) - Admin", "server_7d")
                .add_string_choice("Top Users (7 days) - Admin", "top_users")
        })
        .to_owned()
}

// ==================== Validation Functions ====================

/// Valid user settings
pub const USER_SETTINGS: &[&str] = &["persona"];

/// Valid channel settings
pub const CHANNEL_SETTINGS: &[&str] = &["verbosity", "persona", "conflict_mediation", "max_paragraphs"];

/// Valid guild settings
pub const GUILD_SETTINGS: &[&str] = &[
    "default_verbosity",
    "default_persona",
    "response_embeds",
    "conflict_mediation",
    "conflict_sensitivity",
    "mediation_cooldown",
    "max_context_messages",
    "audio_transcription",
    "audio_transcription_mode",
    "audio_transcription_output",
    "mention_responses",
    "debate_auto_response",
    "startup_notification",
    "startup_notify_owner_id",
    "startup_notify_channel_id",
    "startup_dm_commit_count",
    "startup_channel_commit_count",
];

/// Valid verbosity levels
pub const VERBOSITY_VALUES: &[&str] = &["concise", "normal", "detailed"];

/// Valid persona values
pub const PERSONA_VALUES: &[&str] = &[
    "obi",
    "muppet",
    "chef",
    "teacher",
    "analyst",
    "visionary",
    "noir",
    "zen",
    "bard",
    "coach",
    "scientist",
    "gamer",
];

/// Valid enabled/disabled values
pub const ENABLED_DISABLED_VALUES: &[&str] = &["enabled", "disabled"];

/// Valid conflict sensitivity values
pub const SENSITIVITY_VALUES: &[&str] = &["low", "medium", "high", "ultra"];

/// Valid cooldown values (minutes)
pub const COOLDOWN_VALUES: &[&str] = &["1", "5", "10", "15", "30", "60"];

/// Valid context message counts
pub const CONTEXT_MESSAGE_VALUES: &[&str] = &["10", "20", "40", "60"];

/// Valid audio transcription modes
pub const AUDIO_MODE_VALUES: &[&str] = &["always", "mention_only"];

/// Valid audio output modes
pub const AUDIO_OUTPUT_VALUES: &[&str] = &["transcription_only", "with_commentary"];

/// Valid commit count values
pub const COMMIT_COUNT_VALUES: &[&str] = &["0", "1", "3", "5", "10"];

/// Validates a user setting and value, returns (is_valid, error_message)
pub fn validate_user_setting(setting: &str, value: &str) -> (bool, &'static str) {
    match setting {
        "persona" => {
            if PERSONA_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid persona. Use one of: `obi`, `muppet`, `chef`, `teacher`, `analyst`, `visionary`, `noir`, `zen`, `bard`, `coach`, `scientist`, `gamer`.")
            }
        }
        _ => (false, "Unknown user setting."),
    }
}

/// Validates a channel setting and value, returns (is_valid, error_message)
pub fn validate_channel_setting(setting: &str, value: &str) -> (bool, &'static str) {
    match setting {
        "verbosity" => {
            if VERBOSITY_VALUES.contains(&value) {
                (true, "")
            } else {
                (
                    false,
                    "Invalid verbosity. Use: `concise`, `normal`, or `detailed`.",
                )
            }
        }
        "persona" => {
            if value == "clear" || PERSONA_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid persona. Use one of: `obi`, `muppet`, `chef`, `teacher`, `analyst`, `visionary`, `noir`, `zen`, `bard`, `coach`, `scientist`, `gamer`, or `clear`.")
            }
        }
        "conflict_mediation" => {
            if ENABLED_DISABLED_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid value. Use: `enabled` or `disabled`.")
            }
        }
        "max_paragraphs" => {
            if let Ok(num) = value.parse::<i64>() {
                if num == 0 || (1..=10).contains(&num) {
                    (true, "")
                } else {
                    (false, "Invalid value. Use: `0` (unlimited) or `1`-`10`.")
                }
            } else {
                (false, "Invalid value. Use: `0` (unlimited) or `1`-`10`.")
            }
        }
        _ => (false, "Unknown channel setting."),
    }
}

/// Validates a guild setting and value, returns (is_valid, error_message)
pub fn validate_guild_setting(setting: &str, value: &str) -> (bool, &'static str) {
    match setting {
        "default_verbosity" => {
            if VERBOSITY_VALUES.contains(&value) {
                (true, "")
            } else {
                (
                    false,
                    "Invalid verbosity level. Use: `concise`, `normal`, or `detailed`.",
                )
            }
        }
        "default_persona" => {
            if PERSONA_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid persona. Use one of: `obi`, `muppet`, `chef`, `teacher`, `analyst`, `visionary`, `noir`, `zen`, `bard`, `coach`, `scientist`, `gamer`.")
            }
        }
        "conflict_mediation"
        | "audio_transcription"
        | "mention_responses"
        | "response_embeds"
        | "debate_auto_response" => {
            if ENABLED_DISABLED_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid value. Use: `enabled` or `disabled`.")
            }
        }
        "conflict_sensitivity" => {
            if SENSITIVITY_VALUES.contains(&value) {
                (true, "")
            } else {
                (
                    false,
                    "Invalid sensitivity. Use: `low`, `medium`, `high`, or `ultra`.",
                )
            }
        }
        "mediation_cooldown" => {
            if COOLDOWN_VALUES.contains(&value) {
                (true, "")
            } else {
                (
                    false,
                    "Invalid cooldown. Use: `1`, `5`, `10`, `15`, `30`, or `60` (minutes).",
                )
            }
        }
        "max_context_messages" => {
            if CONTEXT_MESSAGE_VALUES.contains(&value) {
                (true, "")
            } else {
                (
                    false,
                    "Invalid context size. Use: `10`, `20`, `40`, or `60` (messages).",
                )
            }
        }
        "audio_transcription_mode" => {
            if AUDIO_MODE_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid mode. Use: `always` or `mention_only`.")
            }
        }
        "audio_transcription_output" => {
            if AUDIO_OUTPUT_VALUES.contains(&value) {
                (true, "")
            } else {
                (
                    false,
                    "Invalid output mode. Use: `transcription_only` or `with_commentary`.",
                )
            }
        }
        "startup_notification" => {
            if ENABLED_DISABLED_VALUES.contains(&value) {
                (true, "")
            } else {
                (false, "Invalid value. Use: `enabled` or `disabled`.")
            }
        }
        "startup_notify_owner_id" => {
            // Must be a valid Discord user ID (numeric)
            if !value.is_empty() && value.parse::<u64>().is_ok() {
                (true, "")
            } else {
                (
                    false,
                    "Invalid user ID. Enter a valid Discord user ID (numeric).",
                )
            }
        }
        "startup_notify_channel_id" => {
            // Must be a valid Discord channel ID (numeric)
            if !value.is_empty() && value.parse::<u64>().is_ok() {
                (true, "")
            } else {
                (
                    false,
                    "Invalid channel ID. Enter a valid Discord channel ID (numeric).",
                )
            }
        }
        "startup_dm_commit_count" | "startup_channel_commit_count" => {
            // Accept any number 0-20
            if let Ok(count) = value.parse::<usize>() {
                if count <= 20 {
                    (true, "")
                } else {
                    (false, "Commit count must be between 0 and 20.")
                }
            } else {
                (false, "Invalid value. Enter a number between 0 and 20.")
            }
        }
        _ => (false, "Unknown guild setting."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_admin_commands() {
        let commands = create_commands();
        assert_eq!(commands.len(), 9, "Should have 9 admin commands");
    }

    // ==================== User Setting Validation Tests ====================

    #[test]
    fn test_validate_user_persona_valid() {
        assert!(validate_user_setting("persona", "obi").0);
        assert!(validate_user_setting("persona", "muppet").0);
        assert!(validate_user_setting("persona", "chef").0);
        assert!(validate_user_setting("persona", "teacher").0);
        assert!(validate_user_setting("persona", "analyst").0);
        assert!(validate_user_setting("persona", "visionary").0);
        assert!(validate_user_setting("persona", "noir").0);
        assert!(validate_user_setting("persona", "zen").0);
        assert!(validate_user_setting("persona", "bard").0);
        assert!(validate_user_setting("persona", "coach").0);
        assert!(validate_user_setting("persona", "scientist").0);
        assert!(validate_user_setting("persona", "gamer").0);
    }

    #[test]
    fn test_validate_user_persona_invalid() {
        let (valid, msg) = validate_user_setting("persona", "unknown");
        assert!(!valid);
        assert!(msg.contains("obi"));
    }

    #[test]
    fn test_validate_user_unknown_setting() {
        let (valid, msg) = validate_user_setting("unknown_setting", "value");
        assert!(!valid);
        assert!(msg.contains("Unknown"));
    }

    #[test]
    fn test_user_settings_list() {
        assert_eq!(USER_SETTINGS.len(), 1);
        assert!(USER_SETTINGS.contains(&"persona"));
    }

    // ==================== Channel Setting Validation Tests ====================

    #[test]
    fn test_validate_channel_verbosity_valid() {
        assert!(validate_channel_setting("verbosity", "concise").0);
        assert!(validate_channel_setting("verbosity", "normal").0);
        assert!(validate_channel_setting("verbosity", "detailed").0);
    }

    #[test]
    fn test_validate_channel_verbosity_invalid() {
        let (valid, msg) = validate_channel_setting("verbosity", "verbose");
        assert!(!valid);
        assert!(msg.contains("concise"));
    }

    #[test]
    fn test_validate_channel_persona_valid() {
        assert!(validate_channel_setting("persona", "obi").0);
        assert!(validate_channel_setting("persona", "muppet").0);
        assert!(validate_channel_setting("persona", "chef").0);
        assert!(validate_channel_setting("persona", "teacher").0);
        assert!(validate_channel_setting("persona", "analyst").0);
        assert!(validate_channel_setting("persona", "visionary").0);
        assert!(validate_channel_setting("persona", "noir").0);
        assert!(validate_channel_setting("persona", "zen").0);
        assert!(validate_channel_setting("persona", "bard").0);
        assert!(validate_channel_setting("persona", "coach").0);
        assert!(validate_channel_setting("persona", "scientist").0);
        assert!(validate_channel_setting("persona", "gamer").0);
        assert!(validate_channel_setting("persona", "clear").0);
    }

    #[test]
    fn test_validate_channel_persona_invalid() {
        let (valid, msg) = validate_channel_setting("persona", "unknown");
        assert!(!valid);
        assert!(msg.contains("obi"));
    }

    #[test]
    fn test_validate_channel_conflict_mediation_valid() {
        assert!(validate_channel_setting("conflict_mediation", "enabled").0);
        assert!(validate_channel_setting("conflict_mediation", "disabled").0);
    }

    #[test]
    fn test_validate_channel_conflict_mediation_invalid() {
        let (valid, msg) = validate_channel_setting("conflict_mediation", "on");
        assert!(!valid);
        assert!(msg.contains("enabled"));
    }

    #[test]
    fn test_validate_channel_unknown_setting() {
        let (valid, msg) = validate_channel_setting("unknown_setting", "value");
        assert!(!valid);
        assert!(msg.contains("Unknown"));
    }

    #[test]
    fn test_validate_channel_max_paragraphs_valid() {
        assert!(validate_channel_setting("max_paragraphs", "0").0);
        assert!(validate_channel_setting("max_paragraphs", "1").0);
        assert!(validate_channel_setting("max_paragraphs", "5").0);
        assert!(validate_channel_setting("max_paragraphs", "10").0);
    }

    #[test]
    fn test_validate_channel_max_paragraphs_invalid() {
        let (valid, msg) = validate_channel_setting("max_paragraphs", "11");
        assert!(!valid);
        assert!(msg.contains("0") && msg.contains("10"));

        let (valid2, _) = validate_channel_setting("max_paragraphs", "-1");
        assert!(!valid2);

        let (valid3, _) = validate_channel_setting("max_paragraphs", "abc");
        assert!(!valid3);
    }

    // ==================== Guild Setting Validation Tests ====================

    #[test]
    fn test_validate_guild_default_verbosity_valid() {
        assert!(validate_guild_setting("default_verbosity", "concise").0);
        assert!(validate_guild_setting("default_verbosity", "normal").0);
        assert!(validate_guild_setting("default_verbosity", "detailed").0);
    }

    #[test]
    fn test_validate_guild_default_verbosity_invalid() {
        let (valid, msg) = validate_guild_setting("default_verbosity", "brief");
        assert!(!valid);
        assert!(msg.contains("concise"));
    }

    #[test]
    fn test_validate_guild_default_persona_valid() {
        assert!(validate_guild_setting("default_persona", "obi").0);
        assert!(validate_guild_setting("default_persona", "muppet").0);
        assert!(validate_guild_setting("default_persona", "chef").0);
        assert!(validate_guild_setting("default_persona", "teacher").0);
        assert!(validate_guild_setting("default_persona", "analyst").0);
        assert!(validate_guild_setting("default_persona", "visionary").0);
        assert!(validate_guild_setting("default_persona", "noir").0);
        assert!(validate_guild_setting("default_persona", "zen").0);
        assert!(validate_guild_setting("default_persona", "bard").0);
        assert!(validate_guild_setting("default_persona", "coach").0);
        assert!(validate_guild_setting("default_persona", "scientist").0);
        assert!(validate_guild_setting("default_persona", "gamer").0);
    }

    #[test]
    fn test_validate_guild_default_persona_invalid() {
        let (valid, msg) = validate_guild_setting("default_persona", "unknown");
        assert!(!valid);
        assert!(msg.contains("obi"));
    }

    #[test]
    fn test_validate_guild_conflict_sensitivity_valid() {
        assert!(validate_guild_setting("conflict_sensitivity", "low").0);
        assert!(validate_guild_setting("conflict_sensitivity", "medium").0);
        assert!(validate_guild_setting("conflict_sensitivity", "high").0);
        assert!(validate_guild_setting("conflict_sensitivity", "ultra").0);
    }

    #[test]
    fn test_validate_guild_conflict_sensitivity_invalid() {
        let (valid, msg) = validate_guild_setting("conflict_sensitivity", "max");
        assert!(!valid);
        assert!(msg.contains("low"));
    }

    #[test]
    fn test_validate_guild_mediation_cooldown_valid() {
        assert!(validate_guild_setting("mediation_cooldown", "1").0);
        assert!(validate_guild_setting("mediation_cooldown", "5").0);
        assert!(validate_guild_setting("mediation_cooldown", "60").0);
    }

    #[test]
    fn test_validate_guild_mediation_cooldown_invalid() {
        let (valid, msg) = validate_guild_setting("mediation_cooldown", "2");
        assert!(!valid);
        assert!(msg.contains("minutes"));
    }

    #[test]
    fn test_validate_guild_max_context_valid() {
        assert!(validate_guild_setting("max_context_messages", "10").0);
        assert!(validate_guild_setting("max_context_messages", "40").0);
    }

    #[test]
    fn test_validate_guild_max_context_invalid() {
        let (valid, msg) = validate_guild_setting("max_context_messages", "50");
        assert!(!valid);
        assert!(msg.contains("messages"));
    }

    #[test]
    fn test_validate_guild_audio_transcription_valid() {
        assert!(validate_guild_setting("audio_transcription", "enabled").0);
        assert!(validate_guild_setting("audio_transcription", "disabled").0);
    }

    #[test]
    fn test_validate_guild_audio_mode_valid() {
        assert!(validate_guild_setting("audio_transcription_mode", "always").0);
        assert!(validate_guild_setting("audio_transcription_mode", "mention_only").0);
    }

    #[test]
    fn test_validate_guild_audio_output_valid() {
        assert!(validate_guild_setting("audio_transcription_output", "transcription_only").0);
        assert!(validate_guild_setting("audio_transcription_output", "with_commentary").0);
    }

    #[test]
    fn test_validate_guild_startup_settings_valid() {
        assert!(validate_guild_setting("startup_notification", "enabled").0);
        assert!(validate_guild_setting("startup_dm_commit_count", "5").0);
        assert!(validate_guild_setting("startup_channel_commit_count", "0").0);
        assert!(validate_guild_setting("startup_notify_owner_id", "123456789").0);
    }

    #[test]
    fn test_validate_guild_unknown_setting() {
        let (valid, msg) = validate_guild_setting("unknown_setting", "value");
        assert!(!valid);
        assert!(msg.contains("Unknown"));
    }

    #[test]
    fn test_channel_settings_list() {
        assert_eq!(CHANNEL_SETTINGS.len(), 4);
        assert!(CHANNEL_SETTINGS.contains(&"verbosity"));
        assert!(CHANNEL_SETTINGS.contains(&"persona"));
        assert!(CHANNEL_SETTINGS.contains(&"conflict_mediation"));
        assert!(CHANNEL_SETTINGS.contains(&"max_paragraphs"));
    }

    #[test]
    fn test_guild_settings_list() {
        assert!(GUILD_SETTINGS.len() >= 10);
        assert!(GUILD_SETTINGS.contains(&"default_verbosity"));
        assert!(GUILD_SETTINGS.contains(&"default_persona"));
        assert!(GUILD_SETTINGS.contains(&"conflict_mediation"));
    }
}
