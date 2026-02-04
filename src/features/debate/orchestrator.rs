//! # Debate Orchestrator
//!
//! Manages the flow of a debate between two personas in a Discord thread.

use anyhow::Result;
use dashmap::DashMap;
use log::{debug, error, info};
use serenity::builder::CreateEmbed;
use serenity::model::application::component::ButtonStyle;
use serenity::model::id::ChannelId;
use serenity::prelude::Context;
use std::sync::OnceLock;
use tokio::time::{sleep, Duration};

use crate::features::personas::{Persona, PersonaManager};

/// Active debate state for continuation
#[derive(Debug, Clone)]
pub struct DebateState {
    pub config: DebateConfig,
    pub history: Vec<(String, String)>,
    pub total_rounds_completed: i64,
    pub last_speaker_was_persona1: bool,
}

/// Global storage for active debates (keyed by thread ID)
static ACTIVE_DEBATES: OnceLock<DashMap<u64, DebateState>> = OnceLock::new();

/// Get or initialize the active debates map
pub fn get_active_debates() -> &'static DashMap<u64, DebateState> {
    ACTIVE_DEBATES.get_or_init(DashMap::new)
}

/// Number of additional rounds when continuing
pub const CONTINUE_ROUNDS: i64 = 4;

/// Configuration for a debate session
#[derive(Debug, Clone)]
pub struct DebateConfig {
    /// First debater persona ID
    pub persona1_id: String,
    /// Second debater persona ID
    pub persona2_id: String,
    /// Topic or question to debate
    pub topic: String,
    /// Number of total responses
    pub rounds: i64,
    /// User who initiated the debate
    pub initiator_id: String,
    /// Guild ID (if in a guild)
    pub guild_id: Option<String>,
    /// Initial history from previous debate (for tag-team debates)
    pub initial_history: Option<Vec<(String, String)>>,
    /// Names of previous debaters (for context in tag-team)
    pub previous_debaters: Option<(String, String)>,
}

/// Orchestrates a debate between two personas
pub struct DebateOrchestrator {
    persona_manager: PersonaManager,
}

impl DebateOrchestrator {
    pub fn new() -> Self {
        Self {
            persona_manager: PersonaManager::new(),
        }
    }

    /// Build the system prompt for a debate participant
    fn build_debate_prompt(&self, persona: &Persona, opponent_name: &str, topic: &str, is_opening: bool) -> String {
        self.build_debate_prompt_with_context(persona, opponent_name, topic, is_opening, None)
    }

    /// Build the system prompt with optional context about previous debaters
    fn build_debate_prompt_with_context(
        &self,
        persona: &Persona,
        opponent_name: &str,
        topic: &str,
        is_opening: bool,
        previous_debaters: Option<&(String, String)>,
    ) -> String {
        let base_prompt = &persona.system_prompt;

        let debate_instructions = match (is_opening, previous_debaters) {
            // Joining an existing debate - first entry
            (true, Some((prev1, prev2))) => {
                format!(
                    r#"

## Debate Context
You are joining a debate that was previously between {prev1} and {prev2} on the topic: "{topic}"

You are now debating against {opponent_name}. Review what was said before and bring your fresh perspective.

This is your OPENING STATEMENT as a new participant. You should:
- Acknowledge key points made by the previous debaters
- Present your own unique take on the topic
- Challenge or build upon what was discussed before

Guidelines:
- Stay completely in character as {persona_name}
- Show you've understood the previous discussion
- Bring something new to the conversation
- Keep your response focused and engaging (2-4 paragraphs)"#,
                    prev1 = prev1,
                    prev2 = prev2,
                    opponent_name = opponent_name,
                    topic = topic,
                    persona_name = persona.name
                )
            }
            // Normal opening statement
            (true, None) => {
                format!(
                    r#"

## Debate Context
You are participating in a friendly debate against {opponent_name} on the topic: "{topic}"

This is your OPENING STATEMENT. Present your initial position on this topic.

Guidelines:
- Stay completely in character as {persona_name}
- Present your viewpoint clearly and persuasively
- Keep your response focused and engaging (2-4 paragraphs)
- Be respectful but feel free to be passionate about your position
- Use your unique personality and speaking style"#,
                    opponent_name = opponent_name,
                    topic = topic,
                    persona_name = persona.name
                )
            }
            // Normal response (no previous debaters context needed for responses)
            (false, _) => {
                format!(
                    r#"

## Debate Context
You are in a debate against {opponent_name} on the topic: "{topic}"

Respond to your opponent's previous argument. You may:
- Counter their points with your own reasoning
- Acknowledge valid points while presenting alternatives
- Introduce new perspectives they haven't considered
- Use examples, analogies, or evidence that fits your character

Guidelines:
- Stay completely in character as {persona_name}
- Keep your response focused (2-3 paragraphs)
- Be respectful but don't shy away from disagreement
- Build on the conversation rather than repeating yourself"#,
                    opponent_name = opponent_name,
                    topic = topic,
                    persona_name = persona.name
                )
            }
        };

        format!("{}{}", base_prompt, debate_instructions)
    }

    /// Build an embed for a debate response
    fn build_debate_embed(&self, persona: &Persona, persona_id: &str, response: &str, round: i64, total_rounds: i64) -> CreateEmbed {
        let mut embed = CreateEmbed::default();

        embed.author(|a| {
            a.name(&persona.name);
            if let Some(portrait_url) = self.persona_manager.get_portrait_url(persona_id) {
                a.icon_url(portrait_url);
            }
            a
        });

        embed.color(persona.color);

        // Truncate if needed (Discord embed description limit is 4096)
        let text = if response.len() > 4000 {
            format!("{}...", &response[..3997])
        } else {
            response.to_string()
        };
        embed.description(text);

        embed.footer(|f| f.text(format!("Response {}/{}", round, total_rounds)));

        embed
    }

    /// Run a complete debate in a thread
    pub async fn run_debate<F, Fut>(
        &self,
        ctx: &Context,
        thread_id: ChannelId,
        config: DebateConfig,
        get_ai_response: F,
    ) -> Result<()>
    where
        F: Fn(String, String, Vec<(String, String)>) -> Fut,
        Fut: std::future::Future<Output = Result<String>>,
    {
        let persona1 = self.persona_manager.get_persona(&config.persona1_id)
            .ok_or_else(|| anyhow::anyhow!("Persona '{}' not found", config.persona1_id))?
            .clone();
        let persona2 = self.persona_manager.get_persona(&config.persona2_id)
            .ok_or_else(|| anyhow::anyhow!("Persona '{}' not found", config.persona2_id))?
            .clone();

        let is_tag_team = config.initial_history.is_some();
        info!(
            "Starting debate: {} vs {} on '{}' ({} rounds){}",
            persona1.name, persona2.name, config.topic, config.rounds,
            if is_tag_team { " [TAG-TEAM: continuing from previous debate]" } else { "" }
        );

        // Conversation history for context - start with initial history if provided
        let mut history: Vec<(String, String)> = config.initial_history.clone().unwrap_or_default();

        for round in 1..=config.rounds {
            let is_opening = round == 1;
            let (current_persona, current_persona_id, opponent_persona) = if round % 2 == 1 {
                (&persona1, &config.persona1_id, &persona2)
            } else {
                (&persona2, &config.persona2_id, &persona1)
            };

            debug!(
                "Round {}/{}: {} responding",
                round, config.rounds, current_persona.name
            );

            // Build the prompt for this turn (with context about previous debaters if tag-team)
            let system_prompt = self.build_debate_prompt_with_context(
                current_persona,
                &opponent_persona.name,
                &config.topic,
                is_opening,
                if is_opening && is_tag_team { config.previous_debaters.as_ref() } else { None },
            );

            // Build the user message (context for the AI)
            let user_message = if is_opening && is_tag_team {
                format!(
                    "You are joining an ongoing debate. Review what the previous debaters said, then present your opening position on: {}",
                    config.topic
                )
            } else if is_opening {
                format!("Begin the debate on: {}", config.topic)
            } else {
                format!(
                    "Your opponent {} just said their piece. Respond to continue the debate on: {}",
                    opponent_persona.name, config.topic
                )
            };

            // Show typing indicator
            if let Err(e) = thread_id.broadcast_typing(&ctx.http).await {
                debug!("Failed to send typing indicator: {}", e);
            }

            // Get AI response
            let response = match get_ai_response(
                system_prompt,
                user_message,
                history.clone(),
            ).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to get AI response for round {}: {}", round, e);
                    // Post error message and continue
                    let _ = thread_id.send_message(&ctx.http, |m| {
                        m.content(format!(
                            "*{} seems lost in thought... (API error, skipping turn)*",
                            current_persona.name
                        ))
                    }).await;
                    continue;
                }
            };

            // Add to history for context
            history.push(("assistant".to_string(), response.clone()));

            // Build and send the embed
            let embed = self.build_debate_embed(current_persona, current_persona_id, &response, round, config.rounds);

            if let Err(e) = thread_id.send_message(&ctx.http, |m| {
                m.set_embed(embed)
            }).await {
                error!("Failed to send debate message: {}", e);
                break;
            }

            // Small delay between responses to feel more natural
            if round < config.rounds {
                sleep(Duration::from_millis(1500)).await;
            }
        }

        // Save debate state for potential continuation
        let final_state = DebateState {
            config: config.clone(),
            history: history.clone(),
            total_rounds_completed: config.rounds,
            last_speaker_was_persona1: config.rounds % 2 == 1,
        };
        get_active_debates().insert(thread_id.0, final_state);

        // Send closing message with continue button
        let closing_embed = self.build_closing_embed(&persona1, &persona2, &config.topic, config.rounds);
        let _ = thread_id.send_message(&ctx.http, |m| {
            m.set_embed(closing_embed)
                .components(|c| {
                    c.create_action_row(|row| {
                        row.create_button(|btn| {
                            btn.custom_id(format!("debate_continue_{}", thread_id.0))
                                .label(format!("Continue (+{} rounds)", CONTINUE_ROUNDS))
                                .style(ButtonStyle::Primary)
                                .emoji('🎭')
                        })
                        .create_button(|btn| {
                            btn.custom_id(format!("debate_end_{}", thread_id.0))
                                .label("End Debate")
                                .style(ButtonStyle::Secondary)
                        })
                    })
                })
        }).await;

        info!("Debate completed: {} vs {} on '{}'", persona1.name, persona2.name, config.topic);
        Ok(())
    }

    /// Continue an existing debate with additional rounds
    pub async fn continue_debate<F, Fut>(
        &self,
        ctx: &Context,
        thread_id: ChannelId,
        additional_rounds: i64,
        get_ai_response: F,
    ) -> Result<()>
    where
        F: Fn(String, String, Vec<(String, String)>) -> Fut,
        Fut: std::future::Future<Output = Result<String>>,
    {
        // Get the saved state
        let state = get_active_debates()
            .get(&thread_id.0)
            .map(|s| s.clone())
            .ok_or_else(|| anyhow::anyhow!("No active debate found for this thread"))?;

        let persona1 = self.persona_manager.get_persona(&state.config.persona1_id)
            .ok_or_else(|| anyhow::anyhow!("Persona '{}' not found", state.config.persona1_id))?
            .clone();
        let persona2 = self.persona_manager.get_persona(&state.config.persona2_id)
            .ok_or_else(|| anyhow::anyhow!("Persona '{}' not found", state.config.persona2_id))?
            .clone();

        let start_round = state.total_rounds_completed + 1;
        let end_round = state.total_rounds_completed + additional_rounds;

        info!(
            "Continuing debate: {} vs {} on '{}' (rounds {}-{})",
            persona1.name, persona2.name, state.config.topic, start_round, end_round
        );

        // Post continuation notice
        let _ = thread_id.send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("Debate Continuing!")
                    .description(format!("Adding {} more rounds...", additional_rounds))
                    .color(0x7289DA)
            })
        }).await;

        let mut history = state.history.clone();
        let mut last_was_p1 = state.last_speaker_was_persona1;

        for round in start_round..=end_round {
            // Alternate starting from where we left off
            last_was_p1 = !last_was_p1;
            let (current_persona, current_persona_id, opponent_persona) = if last_was_p1 {
                (&persona1, &state.config.persona1_id, &persona2)
            } else {
                (&persona2, &state.config.persona2_id, &persona1)
            };

            debug!(
                "Continuation round {}: {} responding",
                round, current_persona.name
            );

            let system_prompt = self.build_debate_prompt(
                current_persona,
                &opponent_persona.name,
                &state.config.topic,
                false, // Never an opening in continuation
            );

            let user_message = format!(
                "Your opponent {} just said their piece. Respond to continue the debate on: {}",
                opponent_persona.name, state.config.topic
            );

            if let Err(e) = thread_id.broadcast_typing(&ctx.http).await {
                debug!("Failed to send typing indicator: {}", e);
            }

            let response = match get_ai_response(
                system_prompt,
                user_message,
                history.clone(),
            ).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to get AI response for round {}: {}", round, e);
                    let _ = thread_id.send_message(&ctx.http, |m| {
                        m.content(format!(
                            "*{} seems lost in thought... (API error, skipping turn)*",
                            current_persona.name
                        ))
                    }).await;
                    continue;
                }
            };

            history.push(("assistant".to_string(), response.clone()));

            let embed = self.build_debate_embed(current_persona, current_persona_id, &response, round, end_round);

            if let Err(e) = thread_id.send_message(&ctx.http, |m| {
                m.set_embed(embed)
            }).await {
                error!("Failed to send debate message: {}", e);
                break;
            }

            if round < end_round {
                sleep(Duration::from_millis(1500)).await;
            }
        }

        // Update saved state
        let updated_state = DebateState {
            config: state.config.clone(),
            history: history.clone(),
            total_rounds_completed: end_round,
            last_speaker_was_persona1: last_was_p1,
        };
        get_active_debates().insert(thread_id.0, updated_state);

        // Send new closing message with continue button
        let closing_embed = self.build_closing_embed(&persona1, &persona2, &state.config.topic, end_round);
        let _ = thread_id.send_message(&ctx.http, |m| {
            m.set_embed(closing_embed)
                .components(|c| {
                    c.create_action_row(|row| {
                        row.create_button(|btn| {
                            btn.custom_id(format!("debate_continue_{}", thread_id.0))
                                .label(format!("Continue (+{} rounds)", CONTINUE_ROUNDS))
                                .style(ButtonStyle::Primary)
                                .emoji('🎭')
                        })
                        .create_button(|btn| {
                            btn.custom_id(format!("debate_end_{}", thread_id.0))
                                .label("End Debate")
                                .style(ButtonStyle::Secondary)
                        })
                    })
                })
        }).await;

        info!("Debate continuation completed: {} vs {} ({} total rounds)",
              persona1.name, persona2.name, end_round);
        Ok(())
    }

    /// End a debate and clean up state
    pub fn end_debate(thread_id: u64) {
        get_active_debates().remove(&thread_id);
        info!("Debate ended and state cleaned up for thread {}", thread_id);
    }

    /// Build the closing embed for the debate
    fn build_closing_embed(&self, persona1: &Persona, persona2: &Persona, topic: &str, rounds: i64) -> CreateEmbed {
        let mut embed = CreateEmbed::default();

        embed.title("Debate Concluded");
        embed.description(format!(
            "**Topic:** {}\n\n**Debaters:** {} vs {}\n**Rounds:** {}",
            topic, persona1.name, persona2.name, rounds
        ));
        embed.color(0x7289DA); // Discord blurple
        embed.footer(|f| f.text("Thank you for watching this debate!"));

        embed
    }
}

impl Default for DebateOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debate_config() {
        let config = DebateConfig {
            persona1_id: "obi".to_string(),
            persona2_id: "muppet".to_string(),
            topic: "Is the Force real?".to_string(),
            rounds: 6,
            initiator_id: "123".to_string(),
            guild_id: Some("456".to_string()),
        };

        assert_eq!(config.persona1_id, "obi");
        assert_eq!(config.persona2_id, "muppet");
        assert_eq!(config.rounds, 6);
    }

    #[test]
    fn test_orchestrator_creation() {
        let orchestrator = DebateOrchestrator::new();
        // Just verify it can be created
        assert!(orchestrator.persona_manager.get_persona("obi").is_some());
    }

    #[test]
    fn test_build_debate_prompt_opening() {
        let orchestrator = DebateOrchestrator::new();
        let persona = orchestrator.persona_manager.get_persona("obi").unwrap();

        let prompt = orchestrator.build_debate_prompt(
            persona,
            "Muppet Friend",
            "Is pineapple on pizza acceptable?",
            true,
        );

        assert!(prompt.contains("OPENING STATEMENT"));
        assert!(prompt.contains("Muppet Friend"));
        assert!(prompt.contains("pineapple on pizza"));
    }

    #[test]
    fn test_build_debate_prompt_response() {
        let orchestrator = DebateOrchestrator::new();
        let persona = orchestrator.persona_manager.get_persona("muppet").unwrap();

        let prompt = orchestrator.build_debate_prompt(
            persona,
            "Obi-Wan",
            "Is pineapple on pizza acceptable?",
            false,
        );

        assert!(!prompt.contains("OPENING STATEMENT"));
        assert!(prompt.contains("Respond to your opponent"));
        assert!(prompt.contains("Obi-Wan"));
    }
}
