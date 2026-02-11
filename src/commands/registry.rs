//! Command handler registry
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation for handler dispatch

use std::collections::HashMap;
use std::sync::Arc;

use super::handler::SlashCommandHandler;

/// Registry mapping command names to handlers
///
/// The registry allows handlers to be registered and looked up by command name.
/// Multiple command names can map to the same handler if they share logic.
///
/// # Example
///
/// ```ignore
/// let mut registry = CommandRegistry::new();
/// registry.register(Arc::new(PingHandler));
/// registry.register(Arc::new(HelpHandler));
///
/// if let Some(handler) = registry.get("ping") {
///     handler.handle(ctx, serenity_ctx, command).await?;
/// }
/// ```
#[derive(Clone)]
pub struct CommandRegistry {
    handlers: HashMap<&'static str, Arc<dyn SlashCommandHandler>>,
}

impl CommandRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for its declared command names
    ///
    /// The handler is registered for all names returned by `command_names()`.
    pub fn register(&mut self, handler: Arc<dyn SlashCommandHandler>) {
        for name in handler.command_names() {
            self.handlers.insert(name, Arc::clone(&handler));
        }
    }

    /// Get handler for a command name
    ///
    /// Returns None if no handler is registered for the given name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn SlashCommandHandler>> {
        self.handlers.get(name).cloned()
    }

    /// Check if a command is registered
    pub fn contains(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Number of registered command names
    ///
    /// Note: This counts command names, not unique handlers.
    /// A handler registered for multiple names will be counted multiple times.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Get all registered command names
    pub fn command_names(&self) -> impl Iterator<Item = &&'static str> {
        self.handlers.keys()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::context::CommandContext;
    use anyhow::Result;
    use async_trait::async_trait;
    use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
    use serenity::prelude::Context;

    // Mock handler for testing
    struct MockHandler {
        names: &'static [&'static str],
    }

    #[async_trait]
    impl SlashCommandHandler for MockHandler {
        fn command_names(&self) -> &'static [&'static str] {
            self.names
        }

        async fn handle(
            &self,
            _ctx: Arc<CommandContext>,
            _serenity_ctx: &Context,
            _command: &ApplicationCommandInteraction,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_registry_new_is_empty() {
        let registry = CommandRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_register_single() {
        let mut registry = CommandRegistry::new();
        registry.register(Arc::new(MockHandler { names: &["ping"] }));

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("ping"));
        assert!(!registry.contains("pong"));
    }

    #[test]
    fn test_registry_register_multiple_names() {
        let mut registry = CommandRegistry::new();
        registry.register(Arc::new(MockHandler {
            names: &["ask", "imagine", "debate"],
        }));

        assert_eq!(registry.len(), 3);
        assert!(registry.contains("ask"));
        assert!(registry.contains("imagine"));
        assert!(registry.contains("debate"));
    }

    #[test]
    fn test_registry_get_returns_handler() {
        let mut registry = CommandRegistry::new();
        registry.register(Arc::new(MockHandler { names: &["test"] }));

        let handler = registry.get("test");
        assert!(handler.is_some());

        let missing = registry.get("missing");
        assert!(missing.is_none());
    }

    #[test]
    fn test_registry_default() {
        let registry = CommandRegistry::default();
        assert!(registry.is_empty());
    }
}
