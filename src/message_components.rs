use anyhow::Result;
use log::{error, info};
use serenity::builder::CreateComponents;
use serenity::model::application::component::{ActionRowComponent, ButtonStyle};
use serenity::model::application::interaction::message_component::MessageComponentInteraction;
use serenity::model::application::interaction::modal::ModalSubmitInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;

use crate::commands::CommandHandler;
use crate::database::Database;
use crate::features::personas::PersonaManager;

/// Handler for all message component interactions
pub struct MessageComponentHandler {
    command_handler: CommandHandler,
    persona_manager: PersonaManager,
    database: Database,
}

impl MessageComponentHandler {
    pub fn new(command_handler: CommandHandler, persona_manager: PersonaManager, database: Database) -> Self {
        Self {
            command_handler,
            persona_manager,
            database,
        }
    }

    /// Handle all types of component interactions
    pub async fn handle_component_interaction(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        let custom_id = &interaction.data.custom_id;
        let user_id = interaction.user.id.to_string();
        
        info!("Processing component interaction: {custom_id} from user: {user_id}");

        match custom_id.as_str() {
            "persona_muppet" | "persona_chef" | "persona_obi" | "persona_teacher" | "persona_analyst" | "persona_visionary" => {
                self.handle_persona_button(ctx, interaction).await?;
            }
            id if id.starts_with("confirm_") => {
                self.handle_confirmation(ctx, interaction).await?;
            }
            id if id.starts_with("cancel_") => {
                self.handle_cancellation(ctx, interaction).await?;
            }
            id if id.starts_with("page_") => {
                self.handle_pagination(ctx, interaction).await?;
            }
            id if id.starts_with("debate_continue_") => {
                self.handle_debate_continue(ctx, interaction).await?;
            }
            id if id.starts_with("debate_end_") => {
                self.handle_debate_end(ctx, interaction).await?;
            }
            "show_help_modal" => {
                self.show_help_modal(ctx, interaction).await?;
            }
            "show_persona_modal" => {
                self.show_persona_creation_modal(ctx, interaction).await?;
            }
            _ => {
                interaction
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("Unknown component interaction.")
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle modal submit interactions
    pub async fn handle_modal_submit(&self, ctx: &Context, interaction: &ModalSubmitInteraction) -> Result<()> {
        let custom_id = &interaction.data.custom_id;
        let user_id = interaction.user.id.to_string();
        
        info!("Processing modal submit: {custom_id} from user: {user_id}");

        match custom_id.as_str() {
            "help_feedback_modal" => {
                self.handle_help_feedback_modal(ctx, interaction).await?;
            }
            "persona_creation_modal" => {
                self.handle_persona_creation_modal(ctx, interaction).await?;
            }
            "ai_prompt_modal" => {
                self.handle_ai_prompt_modal(ctx, interaction).await?;
            }
            _ => {
                interaction
                    .create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|message| {
                                message.content("Unknown modal submission.")
                            })
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Create persona selection components (simplified for compatibility)
    pub fn create_persona_select_menu() -> CreateComponents {
        CreateComponents::default()
            .create_action_row(|row| {
                row.create_button(|button| {
                    button
                        .custom_id("persona_muppet")
                        .label("🐸 Muppet")
                        .style(ButtonStyle::Secondary)
                })
                .create_button(|button| {
                    button
                        .custom_id("persona_chef")
                        .label("👨‍🍳 Chef")
                        .style(ButtonStyle::Secondary)
                })
                .create_button(|button| {
                    button
                        .custom_id("persona_obi")
                        .label("⚔️ Obi-Wan")
                        .style(ButtonStyle::Secondary)
                })
                .create_button(|button| {
                    button
                        .custom_id("persona_teacher")
                        .label("📚 Teacher")
                        .style(ButtonStyle::Secondary)
                })
                .create_button(|button| {
                    button
                        .custom_id("persona_analyst")
                        .label("📊 Analyst")
                        .style(ButtonStyle::Secondary)
                })
            })
            .create_action_row(|row| {
                row.create_button(|button| {
                    button
                        .custom_id("persona_visionary")
                        .label("🔮 Visionary")
                        .style(ButtonStyle::Secondary)
                })
            })
            .to_owned()
    }

    /// Create interactive help buttons
    pub fn create_help_buttons() -> CreateComponents {
        CreateComponents::default()
            .create_action_row(|row| {
                row.create_button(|button| {
                    button
                        .custom_id("show_help_modal")
                        .label("❓ Get Detailed Help")
                        .style(ButtonStyle::Primary)
                })
                .create_button(|button| {
                    button
                        .custom_id("show_persona_modal")
                        .label("✨ Create Custom Prompt")
                        .style(ButtonStyle::Secondary)
                })
            })
            .to_owned()
    }

    /// Create confirmation buttons
    pub fn create_confirmation_buttons(action_id: &str) -> CreateComponents {
        CreateComponents::default()
            .create_action_row(|row| {
                row.create_button(|button| {
                    button
                        .custom_id(format!("confirm_{action_id}"))
                        .label("✅ Confirm")
                        .style(ButtonStyle::Success)
                })
                .create_button(|button| {
                    button
                        .custom_id(format!("cancel_{action_id}"))
                        .label("❌ Cancel")
                        .style(ButtonStyle::Danger)
                })
            })
            .to_owned()
    }

    /// Create pagination buttons
    pub fn create_pagination_buttons(current_page: u32, total_pages: u32) -> CreateComponents {
        CreateComponents::default()
            .create_action_row(|row| {
                row.create_button(|button| {
                    button
                        .custom_id("page_first")
                        .label("⏮️")
                        .style(ButtonStyle::Secondary)
                        .disabled(current_page == 1)
                })
                .create_button(|button| {
                    button
                        .custom_id("page_prev")
                        .label("⬅️")
                        .style(ButtonStyle::Secondary)
                        .disabled(current_page == 1)
                })
                .create_button(|button| {
                    button
                        .custom_id("page_info")
                        .label(format!("{current_page}/{total_pages}"))
                        .style(ButtonStyle::Secondary)
                        .disabled(true)
                })
                .create_button(|button| {
                    button
                        .custom_id("page_next")
                        .label("➡️")
                        .style(ButtonStyle::Secondary)
                        .disabled(current_page == total_pages)
                })
                .create_button(|button| {
                    button
                        .custom_id("page_last")
                        .label("⏭️")
                        .style(ButtonStyle::Secondary)
                        .disabled(current_page == total_pages)
                })
            })
            .to_owned()
    }

    /// Handle persona selection from buttons
    async fn handle_persona_button(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        let persona_name = match interaction.data.custom_id.as_str() {
            "persona_muppet" => "muppet",
            "persona_chef" => "chef",
            "persona_obi" => "obi",
            "persona_teacher" => "teacher",
            "persona_analyst" => "analyst",
            "persona_visionary" => "visionary",
            _ => return Ok(()),
        };

        let user_id = interaction.user.id.to_string();
        
        if self.persona_manager.get_persona(persona_name).is_some() {
            self.database.set_user_persona(&user_id, persona_name).await?;
            
            interaction
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::UpdateMessage)
                        .interaction_response_data(|message| {
                            message
                                .content(format!("✅ Your persona has been set to: **{persona_name}**"))
                                .components(|c| c) // Clear components
                        })
                })
                .await?;
        } else {
            interaction
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content("❌ Invalid persona selected.")
                        })
                })
                .await?;
        }
        
        Ok(())
    }

    /// Handle confirmation button clicks
    async fn handle_confirmation(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        let action_id = interaction.data.custom_id.strip_prefix("confirm_").unwrap_or("");
        
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::UpdateMessage)
                    .interaction_response_data(|message| {
                        message
                            .content(format!("✅ Action confirmed: {action_id}"))
                            .components(|c| c) // Clear components
                    })
            })
            .await?;
            
        Ok(())
    }

    /// Handle cancellation button clicks
    async fn handle_cancellation(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::UpdateMessage)
                    .interaction_response_data(|message| {
                        message
                            .content("❌ Action cancelled.")
                            .components(|c| c) // Clear components
                    })
            })
            .await?;
            
        Ok(())
    }

    /// Handle pagination button clicks
    async fn handle_pagination(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        let action = interaction.data.custom_id.strip_prefix("page_").unwrap_or("");
        
        // This is a simple implementation - in a real app you'd track page state
        let message = match action {
            "first" => "📄 Showing first page",
            "prev" => "📄 Showing previous page", 
            "next" => "📄 Showing next page",
            "last" => "📄 Showing last page",
            _ => "📄 Page navigation",
        };
        
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::UpdateMessage)
                    .interaction_response_data(|msg| {
                        msg.content(message)
                            .set_components(Self::create_pagination_buttons(1, 3))
                    })
            })
            .await?;
            
        Ok(())
    }

    /// Show help modal
    async fn show_help_modal(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::Modal)
                    .interaction_response_data(|modal| {
                        modal
                            .custom_id("help_feedback_modal")
                            .title("Help & Feedback")
                            .components(|c| {
                                c.create_action_row(|row| {
                                    row.create_input_text(|input| {
                                        input
                                            .custom_id("help_topic")
                                            .label("What do you need help with?")
                                            .style(serenity::model::application::component::InputTextStyle::Short)
                                            .placeholder("Enter your question...")
                                            .required(true)
                                            .min_length(1)
                                            .max_length(100)
                                    })
                                })
                                .create_action_row(|row| {
                                    row.create_input_text(|input| {
                                        input
                                            .custom_id("help_details")
                                            .label("Additional Details (Optional)")
                                            .style(serenity::model::application::component::InputTextStyle::Paragraph)
                                            .placeholder("Provide more context if needed...")
                                            .required(false)
                                            .max_length(500)
                                    })
                                })
                            })
                    })
            })
            .await?;
            
        Ok(())
    }

    /// Show persona creation modal
    async fn show_persona_creation_modal(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::Modal)
                    .interaction_response_data(|modal| {
                        modal
                            .custom_id("ai_prompt_modal")
                            .title("Custom AI Prompt")
                            .components(|c| {
                                c.create_action_row(|row| {
                                    row.create_input_text(|input| {
                                        input
                                            .custom_id("prompt_text")
                                            .label("Your Custom Prompt")
                                            .style(serenity::model::application::component::InputTextStyle::Paragraph)
                                            .placeholder("Enter your custom prompt for the AI...")
                                            .required(true)
                                            .min_length(10)
                                            .max_length(1000)
                                    })
                                })
                            })
                    })
            })
            .await?;
            
        Ok(())
    }

    /// Handle help feedback modal submission
    async fn handle_help_feedback_modal(&self, ctx: &Context, interaction: &ModalSubmitInteraction) -> Result<()> {
        let mut help_topic = String::new();
        let mut help_details = String::new();
        
        for action_row in &interaction.data.components {
            for component in &action_row.components {
                if let ActionRowComponent::InputText(input) = component {
                    match input.custom_id.as_str() {
                        "help_topic" => help_topic = input.value.clone(),
                        "help_details" => help_details = input.value.clone(),
                        _ => {}
                    }
                }
            }
        }

        let user_id = interaction.user.id.to_string();
        let user_persona = self.database.get_user_persona(&user_id).await?;
        let system_prompt = self.persona_manager.get_system_prompt(&user_persona, Some("explain"));
        
        // Log the help request
        self.database.log_usage(&user_id, "help_modal", Some(&user_persona)).await?;
        
        let combined_message = if help_details.is_empty() {
            help_topic
        } else {
            format!("{help_topic}\n\nAdditional context: {help_details}")
        };

        // Immediately defer the interaction to prevent timeout
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        // Get AI response using the command handler
        match self.command_handler.get_ai_response(&system_prompt, &combined_message).await {
            Ok(ai_response) => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(format!("❓ **Help Response:**\n{ai_response}"))
                    })
                    .await?;
            }
            Err(e) => {
                error!("AI response error in help modal: {e}");
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content("❌ Sorry, I encountered an error processing your help request.")
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle persona creation modal submission
    async fn handle_persona_creation_modal(&self, ctx: &Context, interaction: &ModalSubmitInteraction) -> Result<()> {
        let mut prompt_text = String::new();
        
        for action_row in &interaction.data.components {
            for component in &action_row.components {
                if let ActionRowComponent::InputText(input) = component {
                    if input.custom_id == "prompt_text" {
                        prompt_text = input.value.clone();
                        break;
                    }
                }
            }
        }

        let user_id = interaction.user.id.to_string();
        self.database.log_usage(&user_id, "custom_prompt", None).await?;

        // Immediately defer the interaction to prevent timeout
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await?;

        // Use the custom prompt directly
        match self.command_handler.get_ai_response(&prompt_text, "Please respond according to the instructions provided.").await {
            Ok(ai_response) => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content(format!("✨ **Custom Prompt Response:**\n{ai_response}"))
                    })
                    .await?;
            }
            Err(e) => {
                error!("AI response error in custom prompt: {e}");
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content("❌ Sorry, I encountered an error processing your custom prompt.")
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle AI prompt modal submission
    async fn handle_ai_prompt_modal(&self, ctx: &Context, interaction: &ModalSubmitInteraction) -> Result<()> {
        // This is the same as persona creation modal for now
        self.handle_persona_creation_modal(ctx, interaction).await
    }

    /// Handle debate continue button
    async fn handle_debate_continue(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        use crate::features::debate::{DebateOrchestrator, get_active_debates, CONTINUE_ROUNDS};
        use serenity::model::id::ChannelId;

        // Extract thread ID from custom_id
        let thread_id_str = interaction.data.custom_id
            .strip_prefix("debate_continue_")
            .unwrap_or("0");
        let thread_id: u64 = thread_id_str.parse().unwrap_or(0);

        // Check if debate state exists
        if get_active_debates().get(&thread_id).is_none() {
            interaction
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .content("This debate has expired or already ended.")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Acknowledge the button click
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::UpdateMessage)
                    .interaction_response_data(|message| {
                        message
                            .content("*Continuing the debate...*")
                            .components(|c| c) // Clear buttons
                    })
            })
            .await?;

        // Clone what we need for the async task
        let ctx_clone = ctx.clone();
        let user_id = interaction.user.id.to_string();
        let guild_id = interaction.guild_id.map(|g| g.to_string());
        let channel_id_str = thread_id.to_string();
        let openai_model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        let usage_tracker = self.command_handler.get_usage_tracker();

        // Spawn the continuation
        tokio::spawn(async move {
            let orchestrator = DebateOrchestrator::new();
            let channel_id = ChannelId(thread_id);

            let get_response = |system_prompt: String, user_message: String, history: Vec<(String, String)>| {
                let model = openai_model.clone();
                let tracker = usage_tracker.clone();
                let uid = user_id.clone();
                let gid = guild_id.clone();
                let cid = channel_id_str.clone();

                async move {
                    let mut messages = vec![
                        openai::chat::ChatCompletionMessage {
                            role: openai::chat::ChatCompletionMessageRole::System,
                            content: Some(system_prompt),
                            name: None,
                            function_call: None,
                            tool_call_id: None,
                            tool_calls: None,
                        },
                    ];

                    for (role, content) in history {
                        let message_role = if role == "user" {
                            openai::chat::ChatCompletionMessageRole::User
                        } else {
                            openai::chat::ChatCompletionMessageRole::Assistant
                        };
                        messages.push(openai::chat::ChatCompletionMessage {
                            role: message_role,
                            content: Some(content),
                            name: None,
                            function_call: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                    }

                    messages.push(openai::chat::ChatCompletionMessage {
                        role: openai::chat::ChatCompletionMessageRole::User,
                        content: Some(user_message),
                        name: None,
                        function_call: None,
                        tool_call_id: None,
                        tool_calls: None,
                    });

                    let chat_completion = openai::chat::ChatCompletion::builder(&model, messages)
                        .create()
                        .await
                        .map_err(|e| anyhow::anyhow!("OpenAI API error: {}", e))?;

                    if let Some(usage) = &chat_completion.usage {
                        tracker.log_chat(
                            &model,
                            usage.prompt_tokens,
                            usage.completion_tokens,
                            usage.total_tokens,
                            &uid,
                            gid.as_deref(),
                            Some(&cid),
                            None,
                        );
                    }

                    chat_completion
                        .choices
                        .first()
                        .and_then(|c| c.message.content.clone())
                        .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))
                }
            };

            if let Err(e) = orchestrator.continue_debate(&ctx_clone, channel_id, CONTINUE_ROUNDS, get_response).await {
                error!("Debate continuation failed: {e}");
                let _ = channel_id.send_message(&ctx_clone.http, |m| {
                    m.content("The debate continuation encountered an error.")
                }).await;
            }
        });

        Ok(())
    }

    /// Handle debate end button
    async fn handle_debate_end(&self, ctx: &Context, interaction: &MessageComponentInteraction) -> Result<()> {
        use crate::features::debate::DebateOrchestrator;

        // Extract thread ID from custom_id
        let thread_id_str = interaction.data.custom_id
            .strip_prefix("debate_end_")
            .unwrap_or("0");
        let thread_id: u64 = thread_id_str.parse().unwrap_or(0);

        // Clean up the debate state
        DebateOrchestrator::end_debate(thread_id);

        // Update the message to remove buttons
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::UpdateMessage)
                    .interaction_response_data(|message| {
                        message
                            .embed(|e| {
                                e.title("Debate Concluded")
                                    .description("This debate has ended. Thank you for watching!")
                                    .color(0x7289DA)
                            })
                            .components(|c| c) // Clear buttons
                    })
            })
            .await?;

        info!("Debate ended by user for thread {}", thread_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_persona_select_menu() {
        let components = MessageComponentHandler::create_persona_select_menu();
        // Basic test to ensure components can be created
        // In a real test, you'd verify the structure
        assert!(!components.0.is_empty());
    }

    #[test]
    fn test_create_help_buttons() {
        let components = MessageComponentHandler::create_help_buttons();
        assert!(!components.0.is_empty());
    }

    #[test]
    fn test_create_confirmation_buttons() {
        let components = MessageComponentHandler::create_confirmation_buttons("test_action");
        assert!(!components.0.is_empty());
    }

    #[test]
    fn test_create_pagination_buttons() {
        let components = MessageComponentHandler::create_pagination_buttons(2, 5);
        assert!(!components.0.is_empty());
    }
}