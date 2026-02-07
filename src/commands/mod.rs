//! # Command System
//!
//! Slash command (/) handling for Discord interactions.
//!
//! - **Version**: 2.1.0
//! - **Since**: 0.2.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 2.1.0: Add modular handler infrastructure (handler trait, context, registry)
//! - 2.0.0: Remove bang commands, slash-only command system
//! - 1.0.0: Initial reorganization with modular command structure

pub mod context;
pub mod handler;
pub mod handlers;
pub mod registry;
pub mod slash;

// Re-export the CommandHandler from the handler module
pub use crate::command_handler::CommandHandler;

// Re-export handler infrastructure
pub use context::CommandContext;
pub use handler::SlashCommandHandler;
pub use registry::CommandRegistry;

// Re-export commonly used items from submodules
pub use slash::{
    create_context_menu_commands, create_slash_commands, create_slash_commands_with_plugins,
    get_bool_option, get_channel_option, get_integer_option, get_role_option, get_string_option,
    register_global_commands, register_global_commands_with_plugins, register_guild_commands,
    register_guild_commands_with_plugins,
};
