//! # Council Feature
//!
//! Multi-persona discussion threads with follow-up question support.
//!
//! - **Version**: 2.0.0
//! - **Since**: 3.31.0
//!
//! ## Changelog
//! - 2.0.0: Interoperability with debate, rules parameter, interactive buttons
//! - 1.0.0: Initial implementation with state tracking

use dashmap::DashMap;
use std::sync::OnceLock;

/// Active council state for follow-up questions
#[derive(Debug, Clone)]
pub struct CouncilState {
    /// The original prompt/topic
    pub topic: String,
    /// IDs of participating personas
    pub persona_ids: Vec<String>,
    /// Conversation history (role, persona_id, content)
    pub history: Vec<CouncilMessage>,
    /// User who initiated the council
    pub initiator_id: String,
    /// Guild ID (if in a guild)
    pub guild_id: Option<String>,
    /// Ground rules and definitions for the discussion
    pub rules: Option<String>,
    /// Whether the council session is active (awaiting interaction)
    pub is_active: bool,
    /// Whether opening statements have been completed
    pub opening_complete: bool,
}

/// A message in the council conversation
#[derive(Debug, Clone)]
pub struct CouncilMessage {
    /// "user" or "assistant"
    pub role: String,
    /// For assistant messages, which persona spoke
    pub persona_id: Option<String>,
    /// The message content
    pub content: String,
}

/// Global storage for active councils (keyed by thread ID)
static ACTIVE_COUNCILS: OnceLock<DashMap<u64, CouncilState>> = OnceLock::new();

/// Get or initialize the active councils map
pub fn get_active_councils() -> &'static DashMap<u64, CouncilState> {
    ACTIVE_COUNCILS.get_or_init(DashMap::new)
}

impl CouncilState {
    /// Create a new council state
    pub fn new(
        topic: String,
        persona_ids: Vec<String>,
        initiator_id: String,
        guild_id: Option<String>,
    ) -> Self {
        Self {
            topic,
            persona_ids,
            history: Vec::new(),
            initiator_id,
            guild_id,
            rules: None,
            is_active: true,
            opening_complete: false,
        }
    }

    /// Create a new council state with rules
    pub fn with_rules(
        topic: String,
        persona_ids: Vec<String>,
        initiator_id: String,
        guild_id: Option<String>,
        rules: Option<String>,
    ) -> Self {
        Self {
            topic,
            persona_ids,
            history: Vec::new(),
            initiator_id,
            guild_id,
            rules,
            is_active: true,
            opening_complete: false,
        }
    }

    /// Mark opening statements as complete
    pub fn mark_opening_complete(&mut self) {
        self.opening_complete = true;
    }

    /// Deactivate the council session
    pub fn deactivate(&mut self) {
        self.is_active = false;
    }

    /// Add a user message to the history
    pub fn add_user_message(&mut self, content: String) {
        self.history.push(CouncilMessage {
            role: "user".to_string(),
            persona_id: None,
            content,
        });
    }

    /// Add a persona response to the history
    pub fn add_persona_response(&mut self, persona_id: &str, content: String) {
        self.history.push(CouncilMessage {
            role: "assistant".to_string(),
            persona_id: Some(persona_id.to_string()),
            content,
        });
    }

    /// Get conversation history formatted for OpenAI context
    /// Returns a summary suitable for system prompt context
    pub fn get_context_summary(&self) -> String {
        if self.history.is_empty() {
            return format!("Original topic: {}", self.topic);
        }

        let mut summary = format!("Original topic: {}\n\nPrevious discussion:\n", self.topic);

        // Include recent history (last 10 messages to avoid token limits)
        let recent: Vec<_> = self.history.iter().rev().take(10).collect();
        for msg in recent.into_iter().rev() {
            match &msg.persona_id {
                Some(persona) => {
                    summary.push_str(&format!("[{}]: {}\n\n", persona, msg.content));
                }
                None => {
                    summary.push_str(&format!("[User]: {}\n\n", msg.content));
                }
            }
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_council_state_creation() {
        let state = CouncilState::new(
            "Test topic".to_string(),
            vec!["obi".to_string(), "muppet".to_string()],
            "user123".to_string(),
            Some("guild456".to_string()),
        );

        assert_eq!(state.topic, "Test topic");
        assert_eq!(state.persona_ids.len(), 2);
        assert!(state.history.is_empty());
        assert!(state.rules.is_none());
        assert!(state.is_active);
        assert!(!state.opening_complete);
    }

    #[test]
    fn test_council_state_with_rules() {
        let state = CouncilState::with_rules(
            "Test topic".to_string(),
            vec!["obi".to_string()],
            "user123".to_string(),
            None,
            Some("Be respectful".to_string()),
        );

        assert_eq!(state.rules, Some("Be respectful".to_string()));
        assert!(state.is_active);
    }

    #[test]
    fn test_council_state_lifecycle() {
        let mut state = CouncilState::new(
            "Test".to_string(),
            vec!["obi".to_string()],
            "user".to_string(),
            None,
        );

        assert!(!state.opening_complete);
        state.mark_opening_complete();
        assert!(state.opening_complete);

        assert!(state.is_active);
        state.deactivate();
        assert!(!state.is_active);
    }

    #[test]
    fn test_council_message_history() {
        let mut state = CouncilState::new(
            "Test topic".to_string(),
            vec!["obi".to_string()],
            "user123".to_string(),
            None,
        );

        state.add_user_message("What do you think?".to_string());
        state.add_persona_response("obi", "The Force guides us.".to_string());

        assert_eq!(state.history.len(), 2);
        assert_eq!(state.history[0].role, "user");
        assert_eq!(state.history[1].role, "assistant");
        assert_eq!(state.history[1].persona_id, Some("obi".to_string()));
    }

    #[test]
    fn test_get_active_councils() {
        let councils = get_active_councils();
        assert!(councils.is_empty() || !councils.is_empty()); // Just verify it initializes
    }
}
