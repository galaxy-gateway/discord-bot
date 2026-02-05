//! # Discussion Button Components
//!
//! Button builders for council and debate interactive controls.

use serenity::builder::CreateComponents;
use serenity::model::application::component::ButtonStyle;

use crate::features::personas::PersonaManager;

/// Button ID prefixes for routing
pub const SPEAKER_COUNCIL_PREFIX: &str = "speaker_council_";
pub const CONTINUE_COUNCIL_PREFIX: &str = "continue_council_";
pub const DISMISS_COUNCIL_PREFIX: &str = "dismiss_council_";
pub const HEAR_DEBATE_PREFIX: &str = "hear_debate_";
pub const CONTINUE_DEBATE_PREFIX: &str = "debate_continue_";
pub const END_DEBATE_PREFIX: &str = "debate_end_";

/// Create council control buttons after opening statements
///
/// Includes speaker selection buttons and continue/dismiss controls.
pub fn create_council_buttons(
    thread_id: u64,
    persona_ids: &[String],
    persona_manager: &PersonaManager,
) -> CreateComponents {
    let mut components = CreateComponents::default();

    // Create speaker selection buttons (up to 5 per row, max 2 rows = 10 personas)
    // Discord limits: 5 buttons per row, 5 rows total

    // Collect all the speaker buttons we need
    let speaker_buttons: Vec<(String, String, String)> = persona_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| {
            let name = persona_manager
                .get_persona(id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| id.clone());
            let custom_id = format!("{}{}_{}_{}", SPEAKER_COUNCIL_PREFIX, thread_id, id, idx);
            let label = if name.len() > 20 {
                format!("{}...", &name[..17])
            } else {
                name
            };
            (custom_id, label, id.clone())
        })
        .collect();

    // Split into rows of 5
    let mut button_chunks: Vec<Vec<(String, String, String)>> = Vec::new();
    let mut current_chunk: Vec<(String, String, String)> = Vec::new();

    for button in speaker_buttons {
        current_chunk.push(button);
        if current_chunk.len() >= 5 {
            button_chunks.push(current_chunk);
            current_chunk = Vec::new();
        }
    }
    if !current_chunk.is_empty() {
        button_chunks.push(current_chunk);
    }

    // Add speaker rows (max 2 rows for speakers to leave room for controls)
    for chunk in button_chunks.iter().take(2) {
        components.create_action_row(|row| {
            for (custom_id, label, _) in chunk {
                row.create_button(|btn| {
                    btn.custom_id(custom_id)
                        .label(format!("Hear {}", label))
                        .style(ButtonStyle::Primary)
                });
            }
            row
        });
    }

    // Add control row
    components.create_action_row(|row| {
        row.create_button(|btn| {
            btn.custom_id(format!("{}{}", CONTINUE_COUNCIL_PREFIX, thread_id))
                .label("Continue Discussion")
                .style(ButtonStyle::Success)
        })
        .create_button(|btn| {
            btn.custom_id(format!("{}{}", DISMISS_COUNCIL_PREFIX, thread_id))
                .label("Dismiss Council")
                .style(ButtonStyle::Secondary)
        })
    });

    components
}

/// Create debate control buttons after opening statements
///
/// Includes buttons to hear from specific debaters, continue, or end.
pub fn create_debate_buttons(
    thread_id: u64,
    persona1_id: &str,
    persona2_id: &str,
    persona_manager: &PersonaManager,
) -> CreateComponents {
    let mut components = CreateComponents::default();

    let p1_name = persona_manager
        .get_persona(persona1_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| persona1_id.to_string());
    let p2_name = persona_manager
        .get_persona(persona2_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| persona2_id.to_string());

    // Debater selection row
    components.create_action_row(|row| {
        row.create_button(|btn| {
            btn.custom_id(format!(
                "{}{}_{}",
                HEAR_DEBATE_PREFIX, thread_id, persona1_id
            ))
            .label(format!("Hear {}", p1_name))
            .style(ButtonStyle::Primary)
        })
        .create_button(|btn| {
            btn.custom_id(format!(
                "{}{}_{}",
                HEAR_DEBATE_PREFIX, thread_id, persona2_id
            ))
            .label(format!("Hear {}", p2_name))
            .style(ButtonStyle::Primary)
        })
    });

    // Control row
    components.create_action_row(|row| {
        row.create_button(|btn| {
            btn.custom_id(format!("{}{}", CONTINUE_DEBATE_PREFIX, thread_id))
                .label("Continue (+4 rounds)")
                .style(ButtonStyle::Success)
        })
        .create_button(|btn| {
            btn.custom_id(format!("{}{}", END_DEBATE_PREFIX, thread_id))
                .label("End Debate")
                .style(ButtonStyle::Secondary)
        })
    });

    components
}

/// Create a simple "awaiting input" button set for paused discussions
pub fn create_awaiting_buttons(
    thread_id: u64,
    discussion_type: super::DiscussionType,
) -> CreateComponents {
    let mut components = CreateComponents::default();

    match discussion_type {
        super::DiscussionType::Council => {
            components.create_action_row(|row| {
                row.create_button(|btn| {
                    btn.custom_id(format!("{}{}", CONTINUE_COUNCIL_PREFIX, thread_id))
                        .label("Continue Discussion")
                        .style(ButtonStyle::Success)
                })
                .create_button(|btn| {
                    btn.custom_id(format!("{}{}", DISMISS_COUNCIL_PREFIX, thread_id))
                        .label("Dismiss Council")
                        .style(ButtonStyle::Secondary)
                })
            });
        }
        super::DiscussionType::Debate => {
            components.create_action_row(|row| {
                row.create_button(|btn| {
                    btn.custom_id(format!("{}{}", CONTINUE_DEBATE_PREFIX, thread_id))
                        .label("Continue Debate")
                        .style(ButtonStyle::Success)
                })
                .create_button(|btn| {
                    btn.custom_id(format!("{}{}", END_DEBATE_PREFIX, thread_id))
                        .label("End Debate")
                        .style(ButtonStyle::Secondary)
                })
            });
        }
    }

    components
}

/// Parse a council speaker button custom_id
///
/// Returns (thread_id, persona_id) if valid
pub fn parse_council_speaker_id(custom_id: &str) -> Option<(u64, String)> {
    let stripped = custom_id.strip_prefix(SPEAKER_COUNCIL_PREFIX)?;
    // Format: {thread_id}_{persona_id}_{row_index}
    let parts: Vec<&str> = stripped.splitn(3, '_').collect();
    if parts.len() >= 2 {
        let thread_id = parts[0].parse().ok()?;
        let persona_id = parts[1].to_string();
        Some((thread_id, persona_id))
    } else {
        None
    }
}

/// Parse a debate hear button custom_id
///
/// Returns (thread_id, persona_id) if valid
pub fn parse_debate_hear_id(custom_id: &str) -> Option<(u64, String)> {
    let stripped = custom_id.strip_prefix(HEAR_DEBATE_PREFIX)?;
    // Format: {thread_id}_{persona_id}
    let parts: Vec<&str> = stripped.splitn(2, '_').collect();
    if parts.len() == 2 {
        let thread_id = parts[0].parse().ok()?;
        let persona_id = parts[1].to_string();
        Some((thread_id, persona_id))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_council_speaker_id() {
        let custom_id = "speaker_council_123456789_obi_0";
        let result = parse_council_speaker_id(custom_id);
        assert!(result.is_some());
        let (thread_id, persona_id) = result.unwrap();
        assert_eq!(thread_id, 123456789);
        assert_eq!(persona_id, "obi");
    }

    #[test]
    fn test_parse_debate_hear_id() {
        let custom_id = "hear_debate_987654321_muppet";
        let result = parse_debate_hear_id(custom_id);
        assert!(result.is_some());
        let (thread_id, persona_id) = result.unwrap();
        assert_eq!(thread_id, 987654321);
        assert_eq!(persona_id, "muppet");
    }

    #[test]
    fn test_invalid_parse() {
        assert!(parse_council_speaker_id("invalid_id").is_none());
        assert!(parse_debate_hear_id("invalid_id").is_none());
    }
}
