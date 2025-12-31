//! # Command System
//!
//! Slash command (/) handling for Discord interactions.
//!
//! - **Version**: 2.0.0
//! - **Since**: 0.2.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 2.0.0: Remove bang commands, slash-only command system
//! - 1.0.0: Initial reorganization with modular command structure

pub mod slash;

// Re-export the CommandHandler from the handler module
pub use crate::command_handler::CommandHandler;

// Re-export commonly used items from submodules
pub use slash::{
    create_context_menu_commands, create_slash_commands, create_slash_commands_with_plugins,
    get_channel_option, get_integer_option, get_role_option, get_string_option,
    register_global_commands, register_global_commands_with_plugins,
    register_guild_commands, register_guild_commands_with_plugins,
};
