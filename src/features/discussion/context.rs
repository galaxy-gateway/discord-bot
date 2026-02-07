//! # Discussion Context Detection
//!
//! Functions for detecting and formatting prior discussion context
//! to enable interoperability between council and debate.

use crate::features::council::get_active_councils;
use crate::features::debate::get_active_debates;
use crate::features::personas::PersonaManager;

use super::{DiscussionMessage, DiscussionType, ThreadContext};

/// Detect if there's an active discussion session in a thread
///
/// Checks both ACTIVE_COUNCILS and ACTIVE_DEBATES maps to find
/// any existing session that could provide context.
pub fn detect_thread_context(thread_id: u64) -> Option<ThreadContext> {
    // First check for active council
    if let Some(council_state) = get_active_councils().get(&thread_id) {
        let mut messages: Vec<DiscussionMessage> = Vec::new();

        for msg in &council_state.history {
            let discussion_msg = if msg.role == "user" {
                DiscussionMessage::user(&msg.content)
            } else {
                DiscussionMessage::persona(msg.persona_id.clone().unwrap_or_default(), &msg.content)
            };
            messages.push(discussion_msg);
        }

        let mut ctx = ThreadContext::new(
            DiscussionType::Council,
            &council_state.topic,
            council_state.persona_ids.clone(),
        );
        ctx.messages = messages;
        ctx.rules = council_state.rules.clone();

        return Some(ctx);
    }

    // Then check for active debate
    if let Some(debate_state) = get_active_debates().get(&thread_id) {
        let mut messages: Vec<DiscussionMessage> = Vec::new();

        // Debate history is Vec<(String, String)> where first is role, second is content
        // We need to track which persona spoke based on alternation
        let mut is_persona1 = true;
        for (role, content) in &debate_state.history {
            let discussion_msg = if role == "user" {
                DiscussionMessage::user(content)
            } else {
                // Determine speaker based on alternation pattern
                let speaker = if is_persona1 {
                    &debate_state.config.persona1_id
                } else {
                    &debate_state.config.persona2_id
                };
                is_persona1 = !is_persona1;
                DiscussionMessage::persona(speaker, content)
            };
            messages.push(discussion_msg);
        }

        let mut ctx = ThreadContext::new(
            DiscussionType::Debate,
            &debate_state.config.topic,
            vec![
                debate_state.config.persona1_id.clone(),
                debate_state.config.persona2_id.clone(),
            ],
        );
        ctx.messages = messages;
        ctx.rules = debate_state.config.rules.clone();

        return Some(ctx);
    }

    None
}

/// Format prior discussion context for inclusion in AI system prompts
///
/// Creates a concise summary of the prior discussion that can be injected
/// into system prompts for new participants.
pub fn format_prior_context(context: &ThreadContext, persona_manager: &PersonaManager) -> String {
    let mut output = String::new();

    // Header with discussion type and topic
    output.push_str(&format!("## Prior {} Context\n\n", context.discussion_type));
    output.push_str(&format!("**Original Topic:** {}\n\n", context.topic));

    // List participants with names
    let participant_names: Vec<String> = context
        .participants
        .iter()
        .map(|id| {
            persona_manager
                .get_persona(id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    output.push_str(&format!(
        "**Previous Participants:** {}\n\n",
        participant_names.join(", ")
    ));

    // Include rules if present
    if let Some(rules) = &context.rules {
        output.push_str(&format!("**Established Rules:** {rules}\n\n"));
    }

    // Include recent messages (last 10 to avoid overwhelming context)
    if !context.messages.is_empty() {
        output.push_str("**Discussion Summary:**\n");

        let recent_messages: Vec<_> = context
            .messages
            .iter()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        for msg in recent_messages {
            let speaker_name = match &msg.speaker {
                Some(id) => persona_manager
                    .get_persona(id)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| id.clone()),
                None => "User".to_string(),
            };

            // Truncate long messages for the summary
            let content_preview = if msg.content.len() > 200 {
                format!("{}...", &msg.content[..197])
            } else {
                msg.content.clone()
            };

            output.push_str(&format!("- **{speaker_name}**: {content_preview}\n"));
        }
    }

    output.push_str("\nYou are joining this ongoing discussion. Build upon what has been said while bringing your unique perspective.\n");

    output
}

/// Format context specifically for new council members joining a debate
pub fn format_debate_context_for_council(
    context: &ThreadContext,
    persona_manager: &PersonaManager,
) -> String {
    let mut output = format_prior_context(context, persona_manager);
    output.push_str("\nAs a council member, provide a fresh perspective that may synthesize or challenge the debate positions.\n");
    output
}

/// Format context specifically for new debaters joining a council
pub fn format_council_context_for_debate(
    context: &ThreadContext,
    persona_manager: &PersonaManager,
) -> String {
    let mut output = format_prior_context(context, persona_manager);
    output.push_str(
        "\nAs a debater, take a clear position that addresses the council's discussion points.\n",
    );
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_context_creation() {
        let ctx = ThreadContext::new(
            DiscussionType::Council,
            "Test topic",
            vec!["obi".to_string(), "muppet".to_string()],
        );

        assert_eq!(ctx.discussion_type, DiscussionType::Council);
        assert_eq!(ctx.participants.len(), 2);
    }

    #[test]
    fn test_format_prior_context() {
        let mut ctx = ThreadContext::new(
            DiscussionType::Council,
            "Should we use tabs or spaces?",
            vec!["obi".to_string()],
        );
        ctx.add_message(DiscussionMessage::user("What are your thoughts?"));
        ctx.add_message(DiscussionMessage::persona(
            "obi",
            "The Force suggests spaces for readability.",
        ));
        ctx = ctx.with_rules(Some("Be respectful of coding preferences".to_string()));

        let persona_manager = PersonaManager::new();
        let formatted = format_prior_context(&ctx, &persona_manager);

        assert!(formatted.contains("Prior Council Context"));
        assert!(formatted.contains("tabs or spaces"));
        assert!(formatted.contains("Established Rules"));
        assert!(formatted.contains("Discussion Summary"));
    }
}
