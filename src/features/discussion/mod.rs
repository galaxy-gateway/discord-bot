//! # Discussion Feature
//!
//! Shared types and utilities for council and debate interoperability.
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.33.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation with shared types and context detection

pub mod buttons;
pub mod context;

pub use buttons::*;
pub use context::*;

use serde::{Deserialize, Serialize};

/// Type of discussion session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscussionType {
    /// Multi-persona council discussion
    Council,
    /// Two-persona debate
    Debate,
}

impl std::fmt::Display for DiscussionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscussionType::Council => write!(f, "Council"),
            DiscussionType::Debate => write!(f, "Debate"),
        }
    }
}

/// A message within a discussion session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionMessage {
    /// Role: "user", "assistant", or "system"
    pub role: String,
    /// For assistant messages, which persona spoke (persona ID)
    pub speaker: Option<String>,
    /// The message content
    pub content: String,
}

impl DiscussionMessage {
    /// Create a new user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            speaker: None,
            content: content.into(),
        }
    }

    /// Create a new assistant message from a persona
    pub fn persona(persona_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            speaker: Some(persona_id.into()),
            content: content.into(),
        }
    }

    /// Create a new system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            speaker: None,
            content: content.into(),
        }
    }
}

/// Context from a prior discussion thread
#[derive(Debug, Clone)]
pub struct ThreadContext {
    /// Type of the original discussion
    pub discussion_type: DiscussionType,
    /// Original topic/prompt
    pub topic: String,
    /// Participants (persona IDs)
    pub participants: Vec<String>,
    /// Message history
    pub messages: Vec<DiscussionMessage>,
    /// Ground rules/definitions (if any)
    pub rules: Option<String>,
}

impl ThreadContext {
    /// Create a new thread context
    pub fn new(
        discussion_type: DiscussionType,
        topic: impl Into<String>,
        participants: Vec<String>,
    ) -> Self {
        Self {
            discussion_type,
            topic: topic.into(),
            participants,
            messages: Vec::new(),
            rules: None,
        }
    }

    /// Add a message to the context
    pub fn add_message(&mut self, message: DiscussionMessage) {
        self.messages.push(message);
    }

    /// Set rules for the discussion
    pub fn with_rules(mut self, rules: Option<String>) -> Self {
        self.rules = rules;
        self
    }

    /// Get the number of messages
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discussion_type_display() {
        assert_eq!(format!("{}", DiscussionType::Council), "Council");
        assert_eq!(format!("{}", DiscussionType::Debate), "Debate");
    }

    #[test]
    fn test_discussion_message_constructors() {
        let user_msg = DiscussionMessage::user("Hello");
        assert_eq!(user_msg.role, "user");
        assert!(user_msg.speaker.is_none());
        assert_eq!(user_msg.content, "Hello");

        let persona_msg = DiscussionMessage::persona("obi", "The Force is with you");
        assert_eq!(persona_msg.role, "assistant");
        assert_eq!(persona_msg.speaker, Some("obi".to_string()));

        let system_msg = DiscussionMessage::system("System info");
        assert_eq!(system_msg.role, "system");
        assert!(system_msg.speaker.is_none());
    }

    #[test]
    fn test_thread_context() {
        let mut ctx = ThreadContext::new(
            DiscussionType::Council,
            "Test topic",
            vec!["obi".to_string(), "muppet".to_string()],
        );

        assert_eq!(ctx.discussion_type, DiscussionType::Council);
        assert_eq!(ctx.topic, "Test topic");
        assert_eq!(ctx.participants.len(), 2);
        assert_eq!(ctx.message_count(), 0);

        ctx.add_message(DiscussionMessage::user("Question"));
        assert_eq!(ctx.message_count(), 1);

        let ctx_with_rules = ctx.with_rules(Some("Be respectful".to_string()));
        assert_eq!(ctx_with_rules.rules, Some("Be respectful".to_string()));
    }
}
