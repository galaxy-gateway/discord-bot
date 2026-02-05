//! # Feature: OpenAI Usage Tracking
//!
//! Captures and stores OpenAI API usage metrics for cost analysis and monitoring.
//! Supports ChatCompletion tokens, Whisper audio duration, and DALL-E image generation.
//!
//! - **Version**: 1.2.0
//! - **Since**: 0.5.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.2.0: Updated pricing to February 2026, added GPT-5.x, GPT-4.1, O-series,
//!          GPT Image 1.5, Sora, TTS, embeddings, and helper cost functions
//! - 1.1.0: Added CostBucket categorization to track usage by feature purpose
//! - 1.0.0: Initial release with async background logging

use crate::database::Database;
use log::{debug, error, warn};
use tokio::sync::mpsc;

/// OpenAI API pricing constants (per 1K tokens unless noted)
/// Last updated: February 2026 - Standard processing tier
pub mod pricing {
    // ============================================
    // GPT-5.2 Series
    // ============================================
    pub const GPT52_INPUT_PER_1K: f64 = 0.00175; // $1.75/1M input
    pub const GPT52_CACHED_INPUT_PER_1K: f64 = 0.000175; // $0.175/1M cached input
    pub const GPT52_OUTPUT_PER_1K: f64 = 0.014; // $14.00/1M output

    pub const GPT52_PRO_INPUT_PER_1K: f64 = 0.021; // $21.00/1M input
    pub const GPT52_PRO_OUTPUT_PER_1K: f64 = 0.168; // $168.00/1M output

    // ============================================
    // GPT-5.1 Series
    // ============================================
    pub const GPT51_INPUT_PER_1K: f64 = 0.00125; // $1.25/1M input
    pub const GPT51_CACHED_INPUT_PER_1K: f64 = 0.000125; // $0.125/1M cached input
    pub const GPT51_OUTPUT_PER_1K: f64 = 0.01; // $10.00/1M output

    // ============================================
    // GPT-5 Series
    // ============================================
    pub const GPT5_INPUT_PER_1K: f64 = 0.00125; // $1.25/1M input
    pub const GPT5_CACHED_INPUT_PER_1K: f64 = 0.000125; // $0.125/1M cached input
    pub const GPT5_OUTPUT_PER_1K: f64 = 0.01; // $10.00/1M output

    pub const GPT5_PRO_INPUT_PER_1K: f64 = 0.015; // $15.00/1M input
    pub const GPT5_PRO_OUTPUT_PER_1K: f64 = 0.12; // $120.00/1M output

    pub const GPT5_MINI_INPUT_PER_1K: f64 = 0.00025; // $0.25/1M input
    pub const GPT5_MINI_CACHED_INPUT_PER_1K: f64 = 0.000025; // $0.025/1M cached input
    pub const GPT5_MINI_OUTPUT_PER_1K: f64 = 0.002; // $2.00/1M output

    pub const GPT5_NANO_INPUT_PER_1K: f64 = 0.00005; // $0.05/1M input
    pub const GPT5_NANO_CACHED_INPUT_PER_1K: f64 = 0.000005; // $0.005/1M cached input
    pub const GPT5_NANO_OUTPUT_PER_1K: f64 = 0.0004; // $0.40/1M output

    // ============================================
    // GPT-4.1 Series
    // ============================================
    pub const GPT41_INPUT_PER_1K: f64 = 0.002; // $2.00/1M input
    pub const GPT41_CACHED_INPUT_PER_1K: f64 = 0.0005; // $0.50/1M cached input
    pub const GPT41_OUTPUT_PER_1K: f64 = 0.008; // $8.00/1M output

    pub const GPT41_MINI_INPUT_PER_1K: f64 = 0.0004; // $0.40/1M input
    pub const GPT41_MINI_CACHED_INPUT_PER_1K: f64 = 0.0001; // $0.10/1M cached input
    pub const GPT41_MINI_OUTPUT_PER_1K: f64 = 0.0016; // $1.60/1M output

    pub const GPT41_NANO_INPUT_PER_1K: f64 = 0.0001; // $0.10/1M input
    pub const GPT41_NANO_CACHED_INPUT_PER_1K: f64 = 0.000025; // $0.025/1M cached input
    pub const GPT41_NANO_OUTPUT_PER_1K: f64 = 0.0004; // $0.40/1M output

    // ============================================
    // GPT-4o Series
    // ============================================
    pub const GPT4O_INPUT_PER_1K: f64 = 0.0025; // $2.50/1M input
    pub const GPT4O_CACHED_INPUT_PER_1K: f64 = 0.00125; // $1.25/1M cached input
    pub const GPT4O_OUTPUT_PER_1K: f64 = 0.01; // $10.00/1M output

    pub const GPT4O_MINI_INPUT_PER_1K: f64 = 0.00015; // $0.15/1M input
    pub const GPT4O_MINI_CACHED_INPUT_PER_1K: f64 = 0.000075; // $0.075/1M cached input
    pub const GPT4O_MINI_OUTPUT_PER_1K: f64 = 0.0006; // $0.60/1M output

    // ============================================
    // O-Series Reasoning Models
    // ============================================
    pub const O1_INPUT_PER_1K: f64 = 0.015; // $15.00/1M input
    pub const O1_CACHED_INPUT_PER_1K: f64 = 0.0075; // $7.50/1M cached input
    pub const O1_OUTPUT_PER_1K: f64 = 0.06; // $60.00/1M output

    pub const O1_PRO_INPUT_PER_1K: f64 = 0.15; // $150.00/1M input
    pub const O1_PRO_OUTPUT_PER_1K: f64 = 0.6; // $600.00/1M output

    pub const O1_MINI_INPUT_PER_1K: f64 = 0.0011; // $1.10/1M input
    pub const O1_MINI_CACHED_INPUT_PER_1K: f64 = 0.00055; // $0.55/1M cached input
    pub const O1_MINI_OUTPUT_PER_1K: f64 = 0.0044; // $4.40/1M output

    pub const O3_INPUT_PER_1K: f64 = 0.002; // $2.00/1M input
    pub const O3_CACHED_INPUT_PER_1K: f64 = 0.0005; // $0.50/1M cached input
    pub const O3_OUTPUT_PER_1K: f64 = 0.008; // $8.00/1M output

    pub const O3_PRO_INPUT_PER_1K: f64 = 0.02; // $20.00/1M input
    pub const O3_PRO_OUTPUT_PER_1K: f64 = 0.08; // $80.00/1M output

    pub const O3_MINI_INPUT_PER_1K: f64 = 0.0011; // $1.10/1M input
    pub const O3_MINI_CACHED_INPUT_PER_1K: f64 = 0.00055; // $0.55/1M cached input
    pub const O3_MINI_OUTPUT_PER_1K: f64 = 0.0044; // $4.40/1M output

    pub const O3_DEEP_RESEARCH_INPUT_PER_1K: f64 = 0.01; // $10.00/1M input
    pub const O3_DEEP_RESEARCH_CACHED_INPUT_PER_1K: f64 = 0.0025; // $2.50/1M cached input
    pub const O3_DEEP_RESEARCH_OUTPUT_PER_1K: f64 = 0.04; // $40.00/1M output

    pub const O4_MINI_INPUT_PER_1K: f64 = 0.0011; // $1.10/1M input
    pub const O4_MINI_CACHED_INPUT_PER_1K: f64 = 0.000275; // $0.275/1M cached input
    pub const O4_MINI_OUTPUT_PER_1K: f64 = 0.0044; // $4.40/1M output

    pub const O4_MINI_DEEP_RESEARCH_INPUT_PER_1K: f64 = 0.002; // $2.00/1M input
    pub const O4_MINI_DEEP_RESEARCH_CACHED_INPUT_PER_1K: f64 = 0.0005; // $0.50/1M cached input
    pub const O4_MINI_DEEP_RESEARCH_OUTPUT_PER_1K: f64 = 0.008; // $8.00/1M output

    // ============================================
    // Realtime API (Text)
    // ============================================
    pub const GPT_REALTIME_TEXT_INPUT_PER_1K: f64 = 0.004; // $4.00/1M input
    pub const GPT_REALTIME_TEXT_CACHED_INPUT_PER_1K: f64 = 0.0004; // $0.40/1M cached input
    pub const GPT_REALTIME_TEXT_OUTPUT_PER_1K: f64 = 0.016; // $16.00/1M output

    pub const GPT_REALTIME_MINI_TEXT_INPUT_PER_1K: f64 = 0.0006; // $0.60/1M input
    pub const GPT_REALTIME_MINI_TEXT_CACHED_INPUT_PER_1K: f64 = 0.00006; // $0.06/1M cached input
    pub const GPT_REALTIME_MINI_TEXT_OUTPUT_PER_1K: f64 = 0.0024; // $2.40/1M output

    // ============================================
    // Realtime API (Audio)
    // ============================================
    pub const GPT_REALTIME_AUDIO_INPUT_PER_1K: f64 = 0.032; // $32.00/1M input
    pub const GPT_REALTIME_AUDIO_CACHED_INPUT_PER_1K: f64 = 0.0004; // $0.40/1M cached input
    pub const GPT_REALTIME_AUDIO_OUTPUT_PER_1K: f64 = 0.064; // $64.00/1M output

    pub const GPT_REALTIME_MINI_AUDIO_INPUT_PER_1K: f64 = 0.01; // $10.00/1M input
    pub const GPT_REALTIME_MINI_AUDIO_CACHED_INPUT_PER_1K: f64 = 0.0003; // $0.30/1M cached input
    pub const GPT_REALTIME_MINI_AUDIO_OUTPUT_PER_1K: f64 = 0.02; // $20.00/1M output

    // ============================================
    // Legacy Models
    // ============================================
    pub const GPT4_TURBO_INPUT_PER_1K: f64 = 0.01; // $10.00/1M input
    pub const GPT4_TURBO_OUTPUT_PER_1K: f64 = 0.03; // $30.00/1M output

    pub const GPT4_INPUT_PER_1K: f64 = 0.03; // $30.00/1M input
    pub const GPT4_OUTPUT_PER_1K: f64 = 0.06; // $60.00/1M output

    pub const GPT4_32K_INPUT_PER_1K: f64 = 0.06; // $60.00/1M input
    pub const GPT4_32K_OUTPUT_PER_1K: f64 = 0.12; // $120.00/1M output

    pub const GPT35_TURBO_INPUT_PER_1K: f64 = 0.0005; // $0.50/1M input
    pub const GPT35_TURBO_OUTPUT_PER_1K: f64 = 0.0015; // $1.50/1M output

    // ============================================
    // Whisper (Transcription)
    // ============================================
    pub const WHISPER_PER_MINUTE: f64 = 0.006; // $0.006/minute

    // ============================================
    // TTS (Text to Speech)
    // ============================================
    pub const TTS_PER_1M_CHARS: f64 = 15.0; // $15.00/1M characters
    pub const TTS_HD_PER_1M_CHARS: f64 = 30.0; // $30.00/1M characters

    // ============================================
    // Embeddings (per 1K tokens)
    // ============================================
    pub const EMBEDDING_3_SMALL_PER_1K: f64 = 0.00002; // $0.02/1M
    pub const EMBEDDING_3_LARGE_PER_1K: f64 = 0.00013; // $0.13/1M
    pub const EMBEDDING_ADA_002_PER_1K: f64 = 0.0001; // $0.10/1M

    // ============================================
    // DALL-E 3 (per image)
    // ============================================
    pub const DALLE3_STANDARD_1024: f64 = 0.04; // $0.04/image (1024x1024)
    pub const DALLE3_STANDARD_WIDE: f64 = 0.08; // $0.08/image (1024x1792 or 1792x1024)
    pub const DALLE3_HD_1024: f64 = 0.08; // $0.08/image HD (1024x1024)
    pub const DALLE3_HD_WIDE: f64 = 0.12; // $0.12/image HD (1024x1792 or 1792x1024)

    // ============================================
    // GPT Image 1.5 (per image)
    // ============================================
    pub const GPT_IMAGE_15_LOW_1024: f64 = 0.009; // $0.009/image low (1024x1024)
    pub const GPT_IMAGE_15_LOW_WIDE: f64 = 0.013; // $0.013/image low (1024x1536 or 1536x1024)
    pub const GPT_IMAGE_15_MEDIUM_1024: f64 = 0.034; // $0.034/image medium (1024x1024)
    pub const GPT_IMAGE_15_MEDIUM_WIDE: f64 = 0.05; // $0.05/image medium (1024x1536 or 1536x1024)
    pub const GPT_IMAGE_15_HIGH_1024: f64 = 0.133; // $0.133/image high (1024x1024)
    pub const GPT_IMAGE_15_HIGH_WIDE: f64 = 0.2; // $0.20/image high (1024x1536 or 1536x1024)

    // ============================================
    // Sora Video (per second)
    // ============================================
    pub const SORA2_PER_SECOND: f64 = 0.10; // $0.10/second (720p)
    pub const SORA2_PRO_720P_PER_SECOND: f64 = 0.30; // $0.30/second (720p)
    pub const SORA2_PRO_1024P_PER_SECOND: f64 = 0.50; // $0.50/second (1024x1792)

    // ============================================
    // Computer Use Preview
    // ============================================
    pub const COMPUTER_USE_PREVIEW_INPUT_PER_1K: f64 = 0.003; // $3.00/1M input
    pub const COMPUTER_USE_PREVIEW_OUTPUT_PER_1K: f64 = 0.012; // $12.00/1M output

    /// Calculate cost for ChatCompletion based on model
    pub fn calculate_chat_cost(model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        let model_lower = model.to_lowercase();

        let (input_rate, output_rate) = match model_lower.as_str() {
            // GPT-5.2 Series
            m if m.contains("gpt-5.2-pro") => (GPT52_PRO_INPUT_PER_1K, GPT52_PRO_OUTPUT_PER_1K),
            m if m.contains("gpt-5.2") => (GPT52_INPUT_PER_1K, GPT52_OUTPUT_PER_1K),

            // GPT-5.1 Series
            m if m.contains("gpt-5.1") => (GPT51_INPUT_PER_1K, GPT51_OUTPUT_PER_1K),

            // GPT-5 Series (order matters: check specific variants first)
            m if m.contains("gpt-5-pro") => (GPT5_PRO_INPUT_PER_1K, GPT5_PRO_OUTPUT_PER_1K),
            m if m.contains("gpt-5-nano") => (GPT5_NANO_INPUT_PER_1K, GPT5_NANO_OUTPUT_PER_1K),
            m if m.contains("gpt-5-mini") => (GPT5_MINI_INPUT_PER_1K, GPT5_MINI_OUTPUT_PER_1K),
            m if m.contains("gpt-5") => (GPT5_INPUT_PER_1K, GPT5_OUTPUT_PER_1K),

            // GPT-4.1 Series
            m if m.contains("gpt-4.1-nano") => (GPT41_NANO_INPUT_PER_1K, GPT41_NANO_OUTPUT_PER_1K),
            m if m.contains("gpt-4.1-mini") => (GPT41_MINI_INPUT_PER_1K, GPT41_MINI_OUTPUT_PER_1K),
            m if m.contains("gpt-4.1") => (GPT41_INPUT_PER_1K, GPT41_OUTPUT_PER_1K),

            // GPT-4o Series
            m if m.contains("gpt-4o-mini") => (GPT4O_MINI_INPUT_PER_1K, GPT4O_MINI_OUTPUT_PER_1K),
            m if m.contains("gpt-4o") => (GPT4O_INPUT_PER_1K, GPT4O_OUTPUT_PER_1K),

            // O-Series Reasoning Models
            m if m.contains("o4-mini-deep-research") => {
                (O4_MINI_DEEP_RESEARCH_INPUT_PER_1K, O4_MINI_DEEP_RESEARCH_OUTPUT_PER_1K)
            }
            m if m.contains("o4-mini") => (O4_MINI_INPUT_PER_1K, O4_MINI_OUTPUT_PER_1K),
            m if m.contains("o3-deep-research") => {
                (O3_DEEP_RESEARCH_INPUT_PER_1K, O3_DEEP_RESEARCH_OUTPUT_PER_1K)
            }
            m if m.contains("o3-pro") => (O3_PRO_INPUT_PER_1K, O3_PRO_OUTPUT_PER_1K),
            m if m.contains("o3-mini") => (O3_MINI_INPUT_PER_1K, O3_MINI_OUTPUT_PER_1K),
            m if m.contains("o3") => (O3_INPUT_PER_1K, O3_OUTPUT_PER_1K),
            m if m.contains("o1-pro") => (O1_PRO_INPUT_PER_1K, O1_PRO_OUTPUT_PER_1K),
            m if m.contains("o1-mini") => (O1_MINI_INPUT_PER_1K, O1_MINI_OUTPUT_PER_1K),
            m if m.contains("o1") => (O1_INPUT_PER_1K, O1_OUTPUT_PER_1K),

            // Legacy Models
            m if m.contains("gpt-4-turbo") => (GPT4_TURBO_INPUT_PER_1K, GPT4_TURBO_OUTPUT_PER_1K),
            m if m.contains("gpt-4-32k") => (GPT4_32K_INPUT_PER_1K, GPT4_32K_OUTPUT_PER_1K),
            m if m.contains("gpt-4") => (GPT4_INPUT_PER_1K, GPT4_OUTPUT_PER_1K),

            // Default to GPT-3.5 Turbo pricing for unknown models
            _ => (GPT35_TURBO_INPUT_PER_1K, GPT35_TURBO_OUTPUT_PER_1K),
        };

        (input_tokens as f64 / 1000.0 * input_rate) + (output_tokens as f64 / 1000.0 * output_rate)
    }

    /// Calculate cost for Whisper transcription
    pub fn calculate_whisper_cost(duration_seconds: f64) -> f64 {
        (duration_seconds / 60.0) * WHISPER_PER_MINUTE
    }

    /// Calculate cost for DALL-E image generation
    pub fn calculate_dalle_cost(size: &str, quality: &str, count: u32) -> f64 {
        let is_wide = size.contains("1792") || size.contains("1024x1792");
        let is_hd = quality.to_lowercase() == "hd";

        let base_price = match (is_wide, is_hd) {
            (false, false) => DALLE3_STANDARD_1024,
            (false, true) => DALLE3_HD_1024,
            (true, false) => DALLE3_STANDARD_WIDE,
            (true, true) => DALLE3_HD_WIDE,
        };

        base_price * count as f64
    }

    /// Calculate cost for GPT Image 1.5 generation
    pub fn calculate_gpt_image_cost(size: &str, quality: &str, count: u32) -> f64 {
        let is_wide = size.contains("1536");
        let quality_lower = quality.to_lowercase();

        let base_price = match (is_wide, quality_lower.as_str()) {
            (false, "low") => GPT_IMAGE_15_LOW_1024,
            (true, "low") => GPT_IMAGE_15_LOW_WIDE,
            (false, "medium") => GPT_IMAGE_15_MEDIUM_1024,
            (true, "medium") => GPT_IMAGE_15_MEDIUM_WIDE,
            (false, "high") => GPT_IMAGE_15_HIGH_1024,
            (true, "high") => GPT_IMAGE_15_HIGH_WIDE,
            // Default to medium quality
            (false, _) => GPT_IMAGE_15_MEDIUM_1024,
            (true, _) => GPT_IMAGE_15_MEDIUM_WIDE,
        };

        base_price * count as f64
    }

    /// Calculate cost for TTS (text to speech)
    pub fn calculate_tts_cost(characters: u64, hd: bool) -> f64 {
        let rate = if hd { TTS_HD_PER_1M_CHARS } else { TTS_PER_1M_CHARS };
        (characters as f64 / 1_000_000.0) * rate
    }

    /// Calculate cost for embeddings
    pub fn calculate_embedding_cost(model: &str, tokens: u32) -> f64 {
        let model_lower = model.to_lowercase();
        let rate = if model_lower.contains("text-embedding-3-small") {
            EMBEDDING_3_SMALL_PER_1K
        } else if model_lower.contains("text-embedding-3-large") {
            EMBEDDING_3_LARGE_PER_1K
        } else {
            EMBEDDING_ADA_002_PER_1K
        };
        (tokens as f64 / 1000.0) * rate
    }

    /// Calculate cost for Sora video generation
    pub fn calculate_sora_cost(duration_seconds: f64, model: &str, resolution: &str) -> f64 {
        let model_lower = model.to_lowercase();
        let rate = if model_lower.contains("pro") {
            if resolution.contains("1024") {
                SORA2_PRO_1024P_PER_SECOND
            } else {
                SORA2_PRO_720P_PER_SECOND
            }
        } else {
            SORA2_PER_SECOND
        };
        duration_seconds * rate
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
