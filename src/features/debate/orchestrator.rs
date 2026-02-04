//! # Debate Orchestrator
//!
//! Manages the flow of a debate between two personas in a Discord thread.

use anyhow::Result;
use log::{debug, error, info};
use serenity::builder::CreateEmbed;
use serenity::model::id::ChannelId;
use serenity::prelude::Context;
use tokio::time::{sleep, Duration};

use crate::features::personas::{Persona, PersonaManager};

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
        let base_prompt = &persona.system_prompt;

        let debate_instructions = if is_opening {
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
        } else {
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
        };

        format!("{}{}", base_prompt, debate_instructions)
    }

    /// Build an embed for a debate response
    fn build_debate_embed(&self, persona: &Persona, response: &str, round: i64, total_rounds: i64) -> CreateEmbed {
        let mut embed = CreateEmbed::default();

        embed.author(|a| {
            a.name(&persona.name);
            if let Some(portrait_url) = self.persona_manager.get_portrait_url(&persona.name.to_lowercase().replace(" ", "_").replace("-", "_")) {
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

        info!(
            "Starting debate: {} vs {} on '{}' ({} rounds)",
            persona1.name, persona2.name, config.topic, config.rounds
        );

        // Conversation history for context (alternating speakers)
        let mut history: Vec<(String, String)> = Vec::new();

        for round in 1..=config.rounds {
            let is_opening = round == 1;
            let (current_persona, opponent_persona) = if round % 2 == 1 {
                (&persona1, &persona2)
            } else {
                (&persona2, &persona1)
            };

            debug!(
                "Round {}/{}: {} responding",
                round, config.rounds, current_persona.name
            );

            // Build the prompt for this turn
            let system_prompt = self.build_debate_prompt(
                current_persona,
                &opponent_persona.name,
                &config.topic,
                is_opening,
            );

            // Build the user message (context for the AI)
            let user_message = if is_opening {
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
            let embed = self.build_debate_embed(current_persona, &response, round, config.rounds);

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

        // Send closing message
        let closing_embed = self.build_closing_embed(&persona1, &persona2, &config.topic, config.rounds);
        let _ = thread_id.send_message(&ctx.http, |m| {
            m.set_embed(closing_embed)
        }).await;

        info!("Debate completed: {} vs {} on '{}'", persona1.name, persona2.name, config.topic);
        Ok(())
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
