//! # Feature: Reminders
//!
//! Scheduled reminder system with persona-aware delivery. Background task checks
//! for due reminders every 60 seconds and delivers them in the user's preferred
//! persona style.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.1.0: Added OpenAI usage tracking for reminder message generation
//! - 1.0.0: Initial release with time parsing (30m, 2h, 1d, 1h30m) and persona delivery

use crate::database::{Database, DEFAULT_BOT_ID};
use crate::personas::PersonaManager;
use crate::usage_tracker::UsageTracker;
use anyhow::Result;
use log::{debug, error, info, warn};
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use serenity::http::Http;
use serenity::model::id::{ChannelId, UserId};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

pub struct ReminderScheduler {
    bot_id: String,
    database: Database,
    persona_manager: PersonaManager,
    openai_model: String,
    usage_tracker: UsageTracker,
}

impl ReminderScheduler {
    pub fn new(database: Database, openai_model: String, usage_tracker: UsageTracker) -> Self {
        Self::with_bot_id(DEFAULT_BOT_ID.to_string(), database, openai_model, usage_tracker)
    }

    pub fn with_bot_id(bot_id: String, database: Database, openai_model: String, usage_tracker: UsageTracker) -> Self {
        Self {
            bot_id,
            database,
            persona_manager: PersonaManager::new(),
            openai_model,
            usage_tracker,
        }
    }

    /// Start the reminder scheduler loop
    /// This should be spawned as a tokio task
    pub async fn run(&self, http: Arc<Http>) {
        let mut check_interval = interval(Duration::from_secs(60)); // Check every minute

        info!("⏰ Reminder scheduler started");

        loop {
            check_interval.tick().await;

            if let Err(e) = self.process_due_reminders(&http).await {
                error!("❌ Error processing reminders: {e}");
            }
        }
    }

    async fn process_due_reminders(&self, http: &Arc<Http>) -> Result<()> {
        let reminders = self.database.get_pending_reminders(&self.bot_id).await?;

        if reminders.is_empty() {
            debug!("⏰ No pending reminders to process");
            return Ok(());
        }

        info!("⏰ Processing {} due reminder(s)", reminders.len());

        for (id, user_id, channel_id, reminder_text) in reminders {
            match self.deliver_reminder(http, id, &user_id, &channel_id, &reminder_text).await {
                Ok(_) => {
                    info!("✅ Delivered reminder #{id} to user {user_id}");
                }
                Err(e) => {
                    warn!("⚠️ Failed to deliver reminder #{id}: {e}");
                    // Still mark as complete to avoid spam - user can set a new reminder
                    if let Err(e) = self.database.complete_reminder(&self.bot_id, id).await {
                        error!("❌ Failed to mark reminder {id} as complete: {e}");
                    }
                }
            }
        }

        Ok(())
    }

    async fn deliver_reminder(
        &self,
        http: &Arc<Http>,
        reminder_id: i64,
        user_id: &str,
        channel_id: &str,
        reminder_text: &str,
    ) -> Result<()> {
        // Get user's preferred persona
        let persona_name = self.database.get_user_persona(&self.bot_id, user_id).await.unwrap_or_else(|_| "obi".to_string());

        // Get the persona's system prompt
        let persona = self.persona_manager.get_persona(&persona_name);
        let system_prompt = persona.map(|p| p.system_prompt.as_str()).unwrap_or("");

        // Generate a persona-flavored reminder message
        let reminder_message = self.generate_reminder_message(&persona_name, system_prompt, reminder_text, user_id, channel_id).await?;

        // Parse channel ID
        let channel = ChannelId(channel_id.parse::<u64>()?);
        let user = UserId(user_id.parse::<u64>()?);

        // Send the reminder with a user mention
        let message = format!("<@{user}>\n\n{reminder_message}");

        channel.say(http, &message).await?;

        // Mark reminder as complete
        self.database.complete_reminder(&self.bot_id, reminder_id).await?;

        Ok(())
    }

    async fn generate_reminder_message(
        &self,
        persona_name: &str,
        persona_prompt: &str,
        reminder_text: &str,
        user_id: &str,
        channel_id: &str,
    ) -> Result<String> {
        // Create a prompt to generate a persona-flavored reminder
        let system_prompt = format!(
            "{persona_prompt}\n\n\
            Your task is to deliver a reminder to the user in your characteristic style. \
            Keep it brief (1-2 sentences max) but in-character. \
            Make it feel personal and warm, not robotic. \
            The reminder message is: \"{reminder_text}\""
        );

        let chat_completion = ChatCompletion::builder(&self.openai_model, vec![
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some(system_prompt),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            },
            ChatCompletionMessage {
                role: ChatCompletionMessageRole::User,
                content: Some("Please deliver this reminder to me now.".to_string()),
                name: None,
                function_call: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ])
        .create()
        .await;

        match chat_completion {
            Ok(completion) => {
                // Log usage if available
                if let Some(usage) = &completion.usage {
                    self.usage_tracker.log_chat(
                        &self.openai_model,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.total_tokens,
                        user_id,
                        None, // Reminders don't have guild context stored
                        Some(channel_id),
                        None,
                    );
                }

                let response = completion
                    .choices
                    .first()
                    .and_then(|choice| choice.message.content.clone())
                    .unwrap_or_else(|| self.fallback_reminder(persona_name, reminder_text));
                Ok(response)
            }
            Err(e) => {
                warn!("⚠️ Failed to generate persona reminder, using fallback: {e}");
                Ok(self.fallback_reminder(persona_name, reminder_text))
            }
        }
    }

    fn fallback_reminder(&self, persona_name: &str, reminder_text: &str) -> String {
        match persona_name {
            "obi" => format!("The Force whispers that the time has come, young one. You asked me to remind you: **{reminder_text}**"),
            "muppet" => format!("*waves arms excitedly* Hey hey hey! Time for your reminder! You said: **{reminder_text}**"),
            "chef" => format!("*taps spoon on counter* Just like checking on a dish in the oven, here's your reminder: **{reminder_text}**"),
            "teacher" => format!("Time for your reminder! Here's what you wanted to remember: **{reminder_text}**"),
            "analyst" => format!("Reminder notification: **{reminder_text}**"),
            _ => format!("⏰ Reminder: **{reminder_text}**"),
        }
    }
}
