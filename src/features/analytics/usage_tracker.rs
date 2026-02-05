//! # Feature: OpenAI Usage Tracking
//!
//! Captures and stores OpenAI API usage metrics for cost analysis and monitoring.
//! Supports ChatCompletion tokens, Whisper audio duration, and DALL-E image generation.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.5.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Added CostBucket categorization to track usage by feature purpose
//! - 1.0.0: Initial release with async background logging

use crate::database::Database;
use log::{debug, error, warn};
use tokio::sync::mpsc;

/// OpenAI API pricing constants (as of January 2025)
pub mod pricing {
    // GPT-4o pricing (per 1K tokens)
    pub const GPT4O_INPUT_PER_1K: f64 = 0.0025; // $2.50/1M input
    pub const GPT4O_OUTPUT_PER_1K: f64 = 0.01; // $10/1M output

    // GPT-4o-mini pricing (per 1K tokens)
    pub const GPT4O_MINI_INPUT_PER_1K: f64 = 0.00015; // $0.15/1M input
    pub const GPT4O_MINI_OUTPUT_PER_1K: f64 = 0.0006; // $0.60/1M output

    // GPT-4 Turbo pricing (per 1K tokens)
    pub const GPT4_TURBO_INPUT_PER_1K: f64 = 0.01; // $10/1M input
    pub const GPT4_TURBO_OUTPUT_PER_1K: f64 = 0.03; // $30/1M output

    // GPT-4 pricing (per 1K tokens)
    pub const GPT4_INPUT_PER_1K: f64 = 0.03; // $30/1M input
    pub const GPT4_OUTPUT_PER_1K: f64 = 0.06; // $60/1M output

    // GPT-3.5 Turbo pricing (per 1K tokens)
    pub const GPT35_TURBO_INPUT_PER_1K: f64 = 0.0005; // $0.50/1M input
    pub const GPT35_TURBO_OUTPUT_PER_1K: f64 = 0.0015; // $1.50/1M output

    // Whisper pricing (per minute)
    pub const WHISPER_PER_MINUTE: f64 = 0.006; // $0.006/minute

    // DALL-E 3 pricing (per image)
    pub const DALLE3_STANDARD_1024: f64 = 0.04; // $0.04/image (1024x1024)
    pub const DALLE3_STANDARD_WIDE: f64 = 0.08; // $0.08/image (1792x1024 or 1024x1792)
    pub const DALLE3_HD_1024: f64 = 0.08; // $0.08/image HD (1024x1024)
    pub const DALLE3_HD_WIDE: f64 = 0.12; // $0.12/image HD (1792x1024 or 1024x1792)

    /// Calculate cost for ChatCompletion based on model
    pub fn calculate_chat_cost(model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        let model_lower = model.to_lowercase();

        let (input_rate, output_rate) = if model_lower.contains("gpt-4o-mini") {
            (GPT4O_MINI_INPUT_PER_1K, GPT4O_MINI_OUTPUT_PER_1K)
        } else if model_lower.contains("gpt-4o") {
            (GPT4O_INPUT_PER_1K, GPT4O_OUTPUT_PER_1K)
        } else if model_lower.contains("gpt-4-turbo") {
            (GPT4_TURBO_INPUT_PER_1K, GPT4_TURBO_OUTPUT_PER_1K)
        } else if model_lower.contains("gpt-4") {
            (GPT4_INPUT_PER_1K, GPT4_OUTPUT_PER_1K)
        } else {
            // Default to GPT-3.5 Turbo pricing
            (GPT35_TURBO_INPUT_PER_1K, GPT35_TURBO_OUTPUT_PER_1K)
        };

        (input_tokens as f64 / 1000.0 * input_rate) + (output_tokens as f64 / 1000.0 * output_rate)
    }

    /// Calculate cost for Whisper transcription
    pub fn calculate_whisper_cost(duration_seconds: f64) -> f64 {
        (duration_seconds / 60.0) * WHISPER_PER_MINUTE
    }

    /// Calculate cost for DALL-E image generation
    pub fn calculate_dalle_cost(size: &str, quality: &str, count: u32) -> f64 {
        let is_wide = size.contains("1792") || (size.contains("1024x1792"));
        let is_hd = quality.to_lowercase() == "hd";

        let base_price = match (is_wide, is_hd) {
            (false, false) => DALLE3_STANDARD_1024,
            (false, true) => DALLE3_HD_1024,
            (true, false) => DALLE3_STANDARD_WIDE,
            (true, true) => DALLE3_HD_WIDE,
        };

        base_price * count as f64
    }
}

/// Categorizes API usage by the feature/purpose that triggered it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostBucket {
    /// Generic /ask responses
    Ask,
    /// /introspect command
    Introspect,
    /// Conflict mediation
    Mediation,
    /// /debate turns
    Debate,
    /// /council responses
    Council,
    /// Scheduled reminders
    Reminder,
    /// CLI plugin responses
    Plugin,
    /// Whisper audio transcription
    Transcription,
    /// DALL-E image generation
    Imagine,
    /// Legacy data or unknown source
    Unknown,
}

impl CostBucket {
    /// Get the string representation for database storage
    pub fn as_str(&self) -> &'static str {
        match self {
            CostBucket::Ask => "ask",
            CostBucket::Introspect => "introspect",
            CostBucket::Mediation => "mediation",
            CostBucket::Debate => "debate",
            CostBucket::Council => "council",
            CostBucket::Reminder => "reminder",
            CostBucket::Plugin => "plugin",
            CostBucket::Transcription => "transcription",
            CostBucket::Imagine => "imagine",
            CostBucket::Unknown => "unknown",
        }
    }
}

/// Types of OpenAI API usage events
#[derive(Debug, Clone)]
pub enum UsageEvent {
    /// ChatCompletion API (GPT models)
    Chat {
        model: String,
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
        user_id: String,
        guild_id: Option<String>,
        channel_id: Option<String>,
        request_id: Option<String>,
        cost_bucket: CostBucket,
    },
    /// Whisper transcription API
    Whisper {
        audio_duration_seconds: f64,
        user_id: String,
        guild_id: Option<String>,
        channel_id: Option<String>,
        cost_bucket: CostBucket,
    },
    /// DALL-E image generation API
    DallE {
        size: String,
        quality: String,
        image_count: u32,
        user_id: String,
        guild_id: Option<String>,
        channel_id: Option<String>,
        cost_bucket: CostBucket,
    },
}

/// Handles async logging of OpenAI usage without blocking API responses
#[derive(Clone)]
pub struct UsageTracker {
    sender: mpsc::UnboundedSender<UsageEvent>,
}

impl UsageTracker {
    /// Create a new UsageTracker with a background logging task
    pub fn new(database: Database) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        // Spawn background task for non-blocking writes
        tokio::spawn(Self::background_logger(database, receiver));

        UsageTracker { sender }
    }

    /// Log a ChatCompletion usage event (non-blocking)
    #[allow(clippy::too_many_arguments)]
    pub fn log_chat(
        &self,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
        request_id: Option<&str>,
        cost_bucket: CostBucket,
    ) {
        let event = UsageEvent::Chat {
            model: model.to_string(),
            input_tokens,
            output_tokens,
            total_tokens,
            user_id: user_id.to_string(),
            guild_id: guild_id.map(String::from),
            channel_id: channel_id.map(String::from),
            request_id: request_id.map(String::from),
            cost_bucket,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue chat usage event: {e}");
        }
    }

    /// Log a Whisper transcription usage event (non-blocking)
    pub fn log_whisper(
        &self,
        audio_duration_seconds: f64,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
        cost_bucket: CostBucket,
    ) {
        let event = UsageEvent::Whisper {
            audio_duration_seconds,
            user_id: user_id.to_string(),
            guild_id: guild_id.map(String::from),
            channel_id: channel_id.map(String::from),
            cost_bucket,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue Whisper usage event: {e}");
        }
    }

    /// Log a DALL-E image generation usage event (non-blocking)
    pub fn log_dalle(
        &self,
        size: &str,
        quality: &str,
        image_count: u32,
        user_id: &str,
        guild_id: Option<&str>,
        channel_id: Option<&str>,
        cost_bucket: CostBucket,
    ) {
        let event = UsageEvent::DallE {
            size: size.to_string(),
            quality: quality.to_string(),
            image_count,
            user_id: user_id.to_string(),
            guild_id: guild_id.map(String::from),
            channel_id: channel_id.map(String::from),
            cost_bucket,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue DALL-E usage event: {e}");
        }
    }

    /// Background task that processes usage events
    async fn background_logger(
        database: Database,
        mut receiver: mpsc::UnboundedReceiver<UsageEvent>,
    ) {
        while let Some(event) = receiver.recv().await {
            if let Err(e) = Self::store_event(&database, &event).await {
                error!("Failed to store usage event: {e}");
            }
        }
    }

    /// Store a usage event in the database
    async fn store_event(database: &Database, event: &UsageEvent) -> anyhow::Result<()> {
        match event {
            UsageEvent::Chat {
                model,
                input_tokens,
                output_tokens,
                total_tokens,
                user_id,
                guild_id,
                channel_id,
                request_id,
                cost_bucket,
            } => {
                let cost = pricing::calculate_chat_cost(model, *input_tokens, *output_tokens);

                database
                    .log_openai_chat_usage(
                        model,
                        *input_tokens,
                        *output_tokens,
                        *total_tokens,
                        cost,
                        user_id,
                        guild_id.as_deref(),
                        channel_id.as_deref(),
                        request_id.as_deref(),
                        cost_bucket.as_str(),
                    )
                    .await?;

                debug!(
                    "Logged chat usage: {} tokens (model: {}, bucket: {}, cost: ${:.6})",
                    total_tokens, model, cost_bucket.as_str(), cost
                );
            }
            UsageEvent::Whisper {
                audio_duration_seconds,
                user_id,
                guild_id,
                channel_id,
                cost_bucket,
            } => {
                let cost = pricing::calculate_whisper_cost(*audio_duration_seconds);

                database
                    .log_openai_whisper_usage(
                        *audio_duration_seconds,
                        cost,
                        user_id,
                        guild_id.as_deref(),
                        channel_id.as_deref(),
                        cost_bucket.as_str(),
                    )
                    .await?;

                debug!(
                    "Logged Whisper usage: {:.1}s audio (bucket: {}, cost: ${:.6})",
                    audio_duration_seconds, cost_bucket.as_str(), cost
                );
            }
            UsageEvent::DallE {
                size,
                quality,
                image_count,
                user_id,
                guild_id,
                channel_id,
                cost_bucket,
            } => {
                let cost = pricing::calculate_dalle_cost(size, quality, *image_count);

                database
                    .log_openai_dalle_usage(
                        size,
                        *image_count,
                        cost,
                        user_id,
                        guild_id.as_deref(),
                        channel_id.as_deref(),
                        cost_bucket.as_str(),
                    )
                    .await?;

                debug!(
                    "Logged DALL-E usage: {} image(s) at {} (bucket: {}, cost: ${:.4})",
                    image_count, size, cost_bucket.as_str(), cost
                );
            }
        }
        Ok(())
    }
}
