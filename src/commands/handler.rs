//! Slash command handler trait and infrastructure
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation for modular command handling

use anyhow::Result;
use async_trait::async_trait;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::prelude::Context;
use std::sync::Arc;

use super::context::CommandContext;

/// Trait for slash command handlers
///
/// Each command handler implements this trait to process one or more slash commands.
/// Handlers are registered with a CommandRegistry and dispatched based on command name.
///
/// # Example
///
/// ```ignore
/// pub struct PingHandler;
///
/// #[async_trait]
/// impl SlashCommandHandler for PingHandler {
///     fn command_names(&self) -> &'static [&'static str] {
///         &["ping"]
///     }
///
///     async fn handle(
///         &self,
///         ctx: Arc<CommandContext>,
///         serenity_ctx: &Context,
///         command: &ApplicationCommandInteraction,
///     ) -> Result<()> {
///         // Handle ping command
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait SlashCommandHandler: Send + Sync {
    /// Command name(s) this handler processes
    ///
    /// A handler can process multiple commands if they share logic.
    fn command_names(&self) -> &'static [&'static str];

    /// Handle the slash command
    ///
    /// # Arguments
    ///
    /// * `ctx` - Shared command context with database, personas, etc.
    /// * `serenity_ctx` - Serenity context for Discord API calls
    /// * `command` - The slash command interaction to handle
    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that the trait is object-safe (can be used with dyn)
    fn _assert_object_safe(_: &dyn SlashCommandHandler) {}
}
