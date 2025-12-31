//! # Slash Commands (/)
//!
//! Discord native slash commands with autocomplete and validation.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.2.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Reorganized from monolithic slash_commands.rs

mod admin;
mod chat;
mod context_menu;
mod dm_stats;
mod imagine;
mod persona;
mod recipe;
mod remind;
mod utility;

use crate::features::plugins::{create_plugin_commands, Plugin};
use anyhow::Result;
use log::info;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::Command;
use serenity::model::application::interaction::application_command::CommandDataOption;
use serenity::model::id::GuildId;
use serenity::prelude::Context;

/// Creates all slash command definitions (without plugins)
pub fn create_slash_commands() -> Vec<CreateApplicationCommand> {
    create_slash_commands_with_plugins(&[])
}

/// Creates all slash command definitions with plugin commands
pub fn create_slash_commands_with_plugins(plugins: &[Plugin]) -> Vec<CreateApplicationCommand> {
    let mut commands = Vec::new();

    // Utility commands
    commands.extend(utility::create_commands());

    // Persona commands
    commands.extend(persona::create_commands());

    // Chat/AI commands
    commands.extend(chat::create_commands());

    // Recipe command
    commands.extend(recipe::create_commands());

    // Image generation
    commands.extend(imagine::create_commands());

    // Reminder commands
    commands.extend(remind::create_commands());

    // Admin commands
    commands.extend(admin::create_commands());

    // DM statistics commands
    commands.extend(dm_stats::create_commands());

    // Plugin-generated commands
    commands.extend(create_plugin_commands(plugins));

    commands
}

/// Creates all context menu commands
pub fn create_context_menu_commands() -> Vec<CreateApplicationCommand> {
    context_menu::create_commands()
}

/// Registers all slash commands globally (without plugins)
pub async fn register_global_commands(ctx: &Context) -> Result<()> {
    register_global_commands_with_plugins(ctx, &[]).await
}

/// Registers all slash commands globally with plugin commands
pub async fn register_global_commands_with_plugins(ctx: &Context, plugins: &[Plugin]) -> Result<()> {
    let slash_commands = create_slash_commands_with_plugins(plugins);
    let context_commands = create_context_menu_commands();

    Command::set_global_application_commands(&ctx.http, |commands| {
        for command in slash_commands {
            commands.add_application_command(command);
        }
        for command in context_commands {
            commands.add_application_command(command);
        }
        commands
    })
    .await?;

    info!(
        "Global slash commands registered successfully ({} commands, {} from plugins)",
        create_slash_commands().len() + create_plugin_commands(plugins).len(),
        create_plugin_commands(plugins).len()
    );
    Ok(())
}

/// Registers all slash commands for a specific guild (faster for testing)
pub async fn register_guild_commands(ctx: &Context, guild_id: GuildId) -> Result<()> {
    register_guild_commands_with_plugins(ctx, guild_id, &[]).await
}

/// Registers all slash commands for a specific guild with plugin commands
pub async fn register_guild_commands_with_plugins(
    ctx: &Context,
    guild_id: GuildId,
    plugins: &[Plugin],
) -> Result<()> {
    let slash_commands = create_slash_commands_with_plugins(plugins);
    let context_commands = create_context_menu_commands();

    guild_id
        .set_application_commands(&ctx.http, |commands| {
            for command in slash_commands {
                commands.add_application_command(command);
            }
            for command in context_commands {
                commands.add_application_command(command);
            }
            commands
        })
        .await?;

    info!(
        "Guild slash commands registered for guild {} ({} commands, {} from plugins)",
        guild_id,
        create_slash_commands().len() + create_plugin_commands(plugins).len(),
        create_plugin_commands(plugins).len()
    );
    Ok(())
}

/// Utility function to get string option from slash command
pub fn get_string_option(options: &[CommandDataOption], name: &str) -> Option<String> {
    options
        .iter()
        .find(|opt| opt.name == name)
        .and_then(|opt| opt.value.as_ref())
        .and_then(|val| val.as_str())
        .map(|s| s.to_string())
}

/// Utility function to get channel option from slash command
pub fn get_channel_option(options: &[CommandDataOption], name: &str) -> Option<u64> {
    options
        .iter()
        .find(|opt| opt.name == name)
        .and_then(|opt| opt.value.as_ref())
        .and_then(|val| val.as_str())
        .and_then(|s| s.parse().ok())
}

/// Utility function to get role option from slash command
pub fn get_role_option(options: &[CommandDataOption], name: &str) -> Option<u64> {
    options
        .iter()
        .find(|opt| opt.name == name)
        .and_then(|opt| opt.value.as_ref())
        .and_then(|val| val.as_str())
        .and_then(|s| s.parse().ok())
}

/// Utility function to get integer option from slash command
pub fn get_integer_option(options: &[CommandDataOption], name: &str) -> Option<i64> {
    options
        .iter()
        .find(|opt| opt.name == name)
        .and_then(|opt| opt.value.as_ref())
        .and_then(|val| val.as_i64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_slash_commands() {
        let commands = create_slash_commands();
        assert!(commands.len() >= 23, "Should have at least 23 commands");

        let command_names: Vec<String> = commands
            .iter()
            .map(|cmd| {
                cmd.0
                    .get("name")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();

        let expected_commands = vec![
            "ping",
            "help",
            "personas",
            "set_persona",
            "hey",
            "explain",
            "simple",
            "steps",
            "recipe",
            "imagine",
            "forget",
            "remind",
            "reminders",
            "introspect",
            "set_channel_verbosity",
            "set_guild_setting",
            "settings",
            "admin_role",
            // New utility commands
            "status",
            "version",
            "uptime",
            // New admin commands
            "features",
            "toggle",
            "sysinfo",
        ];

        for expected in expected_commands {
            assert!(
                command_names.contains(&expected.to_string()),
                "Missing command: {}",
                expected
            );
        }
    }

    #[test]
    fn test_create_context_menu_commands() {
        let commands = create_context_menu_commands();
        assert_eq!(commands.len(), 3, "Should have 3 context menu commands");
    }
}
