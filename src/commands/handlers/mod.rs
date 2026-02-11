//! Per-command handler implementations
//!
//! - **Version**: 2.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 2.0.0: Remove AiChatHandler (hey, explain, simple, steps, recipe) - consolidated into /ask
//! - 1.6.0: Add ContextMenuHandler (Analyze Message, Explain Message, Analyze User)
//! - 1.5.0: Add InfoHandler (introspect, commits, features, toggle, sysinfo, usage, dm_stats, session_history)
//! - 1.4.0: Add CouncilHandler (council, conclude)
//! - 1.3.0: Add AdminHandler, AskHandler, DebateHandler
//! - 1.2.0: Add AiChatHandler (hey, explain, simple, steps, recipe)
//! - 1.1.0: Add ImagineHandler
//! - 1.0.0: Initial extraction from monolithic command_handler.rs

pub mod admin;
pub mod ask;
pub mod context_menu;
pub mod council;
pub mod debate;
pub mod imagine;
pub mod info;
pub mod persona;
pub mod remind;
pub mod utility;

use std::sync::Arc;

use super::handler::SlashCommandHandler;

/// Create all registered command handlers
///
/// Returns a vector of handlers ready to be registered with CommandRegistry.
pub fn create_all_handlers() -> Vec<Arc<dyn SlashCommandHandler>> {
    vec![
        Arc::new(utility::UtilityHandler),
        Arc::new(persona::PersonaHandler),
        Arc::new(remind::RemindHandler),
        Arc::new(imagine::ImagineHandler),
        Arc::new(admin::AdminHandler),
        Arc::new(ask::AskHandler),
        Arc::new(debate::DebateHandler),
        Arc::new(council::CouncilHandler),
        Arc::new(info::InfoHandler),
        Arc::new(context_menu::ContextMenuHandler),
    ]
}
