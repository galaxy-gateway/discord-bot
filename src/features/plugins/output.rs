//! # Thread Output Handler
//!
//! Create Discord threads for plugin output, handle large responses with file attachments,
//! and generate AI summaries. Supports both single video and playlist transcription.
//!
//! - **Version**: 3.4.0
//! - **Since**: 0.9.0
//!
//! ## Changelog
//! - 3.4.0: Added escape_markdown() for safe embedding of user text in markdown formatting
//! - 3.3.0: Added output_format support, sentence-per-line transcript formatting, word count helpers
//! - 3.2.0: Added UsageTracker integration for tracking AI summary costs per user
//! - 3.1.0: Added public post_file() and generate_summary_for_text() methods for per-chunk summaries
//! - 3.0.0: Added chunked transcription progress posting for streaming long videos
//! - 2.0.0: Added playlist progress tracking, per-video results, and summary posting
//! - 1.1.0: Added structured output posting (URL -> summary -> file)
//! - 1.0.0: Initial release

use crate::features::analytics::{CostBucket, UsageTracker};
use crate::features::plugins::config::OutputConfig;
use anyhow::Result;
use log::{info, warn};
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use serenity::http::Http;
use serenity::model::channel::{AttachmentType, ChannelType, GuildChannel};
use serenity::model::id::{ChannelId, MessageId};
use std::borrow::Cow;
use std::sync::Arc;

/// Context for tracking AI usage per user
#[derive(Clone, Default)]
pub struct UserContext {
    pub user_id: String,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
}

/// Handler for plugin output posting
#[derive(Clone)]
pub struct OutputHandler {
    openai_model: String,
    usage_tracker: Option<UsageTracker>,
}

impl OutputHandler {
    /// Create a new output handler
    pub fn new(openai_model: String) -> Self {
        Self {
            openai_model,
            usage_tracker: None,
        }
    }

    /// Builder method to add a UsageTracker
    pub fn with_usage_tracker(mut self, tracker: UsageTracker) -> Self {
        self.usage_tracker = Some(tracker);
        self
    }

    /// Create a thread for plugin output attached to a message
    pub async fn create_output_thread(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        message_id: MessageId,
        thread_name: &str,
        auto_archive_minutes: u64,
    ) -> Result<GuildChannel> {
        // Map to valid Discord auto-archive durations
        let archive_duration = match auto_archive_minutes {
            0..=60 => 60,
            61..=1440 => 1440,
            1441..=4320 => 4320,
            _ => 10080,
        };

        info!(
            "Creating thread '{thread_name}' in channel {channel_id} from message {message_id} (archive: {archive_duration} min)"
        );

        channel_id
            .create_public_thread(http, message_id, |t| {
                t.name(thread_name)
                    .kind(ChannelType::PublicThread)
                    .auto_archive_duration(archive_duration as u16)
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create thread: {}", e))
    }

    /// Create a thread for plugin output by first sending a starter message
    pub async fn create_thread_with_starter(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        thread_name: &str,
        starter_message: &str,
        auto_archive_minutes: u64,
    ) -> Result<GuildChannel> {
        // First send a message to attach the thread to
        let message = channel_id
            .say(http, starter_message)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send starter message: {}", e))?;

        // Then create a thread from that message
        self.create_output_thread(
            http,
            channel_id,
            message.id,
            thread_name,
            auto_archive_minutes,
        )
        .await
    }

    /// Post result to a channel with optional file attachment
    pub async fn post_result(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        output: &str,
        config: &OutputConfig,
        user_context: Option<&UserContext>,
    ) -> Result<()> {
        if output.is_empty() {
            channel_id.say(http, "*No output*").await?;
            return Ok(());
        }

        // Check if we should use file attachment
        let use_file = config.post_as_file && output.len() > config.max_inline_length;

        if use_file {
            // Generate summary if configured
            let summary = if let Some(ref prompt) = config.summary_prompt {
                match self
                    .generate_summary_with_tracking(
                        output,
                        prompt,
                        user_context,
                        Some("result_summary"),
                    )
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Failed to generate summary: {e}");
                        format!("**Output** ({} characters)", output.len())
                    }
                }
            } else {
                format!("**Output** ({} characters)", output.len())
            };

            // Post summary
            channel_id.say(http, &summary).await?;

            // Generate filename
            let filename = config
                .file_name_template
                .as_deref()
                .unwrap_or("output.txt")
                .replace(
                    "${timestamp}",
                    &chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string(),
                );

            // Post file attachment
            let file_bytes = output.as_bytes().to_vec();
            channel_id
                .send_message(http, |m| {
                    m.add_file(AttachmentType::Bytes {
                        data: Cow::Owned(file_bytes),
                        filename,
                    })
                })
                .await?;

            info!("Posted output as file attachment");
        } else {
            // Post inline, splitting if necessary
            let chunks = split_message(output, 1900);

            for (i, chunk) in chunks.iter().enumerate() {
                if i == 0 {
                    channel_id.say(http, chunk).await?;
                } else {
                    channel_id.say(http, chunk).await?;
                }
            }

            info!("Posted output inline ({} chunks)", chunks.len());
        }

        Ok(())
    }

    /// Post an error message to a channel
    pub async fn post_error(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        error: &str,
        error_template: Option<&str>,
    ) -> Result<()> {
        let message = if let Some(template) = error_template {
            template.replace("${error}", error)
        } else {
            format!("**Error:** {error}")
        };

        // Truncate if too long
        let message = if message.len() > 1900 {
            format!("{}...", &message[..1897])
        } else {
            message
        };

        channel_id.say(http, &message).await?;
        Ok(())
    }

    /// Post structured result: URL -> Summary -> File
    /// Used for transcription-style plugins where we want the source first
    ///
    /// Set `url_already_posted` to true if the URL was already posted (e.g., as thread starter)
    pub async fn post_structured_result(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        source_url: &str,
        output: &str,
        config: &OutputConfig,
        url_already_posted: bool,
        user_context: Option<&UserContext>,
    ) -> Result<()> {
        // 1. Post the source URL first (so it can embed/preview), unless already posted
        if !url_already_posted {
            channel_id.say(http, source_url).await?;
            info!("Posted source URL: {source_url}");
        }

        if output.is_empty() {
            channel_id.say(http, "*No transcript generated*").await?;
            return Ok(());
        }

        // 2. Generate and post the summary
        if let Some(ref prompt) = config.summary_prompt {
            match self
                .generate_summary_with_tracking(output, prompt, user_context, Some("video_summary"))
                .await
            {
                Ok(summary) => {
                    // Post summary, splitting if needed
                    let summary_chunks = split_message(&summary, 1900);
                    for chunk in summary_chunks {
                        channel_id.say(http, &chunk).await?;
                    }
                    info!("Posted AI summary");
                }
                Err(e) => {
                    warn!("Failed to generate summary: {e}");
                    channel_id.say(http, "*Summary generation failed*").await?;
                }
            }
        }

        // 3. Post the full transcript as a file attachment
        let filename = config
            .file_name_template
            .as_deref()
            .unwrap_or("transcript.txt")
            .replace(
                "${timestamp}",
                &chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string(),
            );

        let file_bytes = output.as_bytes().to_vec();
        channel_id
            .send_message(http, |m| {
                m.content(format!(
                    "📄 **Full transcript** ({} characters)",
                    output.len()
                ))
                .add_file(AttachmentType::Bytes {
                    data: Cow::Owned(file_bytes),
                    filename,
                })
            })
            .await?;

        info!("Posted transcript file attachment");
        Ok(())
    }

    // Playlist-specific methods

    /// Post or update a progress message for playlist processing
    ///
    /// If `progress_message_id` is Some, edits that message. Otherwise posts a new message
    /// and returns the new message ID.
    pub async fn post_playlist_progress(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        progress_message_id: Option<MessageId>,
        current_index: u32,
        total: u32,
        current_title: &str,
        eta: Option<std::time::Duration>,
    ) -> Result<MessageId> {
        let eta_str = eta
            .map(crate::features::plugins::youtube::format_duration)
            .unwrap_or_else(|| "calculating...".to_string());

        let content = format!(
            "⏳ **Processing playlist:** {}/{} videos | Currently: \"{}\" | ETA: {}",
            current_index,
            total,
            truncate_str(current_title, 40),
            eta_str
        );

        if let Some(msg_id) = progress_message_id {
            // Edit existing message
            channel_id
                .edit_message(http, msg_id, |m| m.content(&content))
                .await?;
            Ok(msg_id)
        } else {
            // Post new message
            let msg = channel_id.say(http, &content).await?;
            info!("Posted playlist progress message");
            Ok(msg.id)
        }
    }

    /// Post a separator and video result within a playlist thread
    pub async fn post_video_result(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        index: u32,
        total: u32,
        video_title: &str,
        video_url: &str,
        output: &str,
        config: &OutputConfig,
        user_context: Option<&UserContext>,
    ) -> Result<()> {
        // Post video header with separator
        let header = format!(
            "---\n**[{index}/{total}] {video_title}**\n{video_url}"
        );
        channel_id.say(http, &header).await?;

        if output.is_empty() {
            channel_id.say(http, "*No transcript generated*").await?;
            return Ok(());
        }

        // Generate and post summary if configured
        if let Some(ref prompt) = config.summary_prompt {
            match self
                .generate_summary_with_tracking(
                    output,
                    prompt,
                    user_context,
                    Some("playlist_video_summary"),
                )
                .await
            {
                Ok(summary) => {
                    let summary_chunks = split_message(&summary, 1900);
                    for chunk in summary_chunks {
                        channel_id.say(http, &chunk).await?;
                    }
                }
                Err(e) => {
                    warn!("Failed to generate summary for video {index}: {e}");
                    channel_id.say(http, "*Summary unavailable*").await?;
                }
            }
        }

        // Post transcript file
        let filename = format!(
            "transcript_{:03}_{}.txt",
            index,
            sanitize_filename(video_title)
        );
        let file_bytes = output.as_bytes().to_vec();

        channel_id
            .send_message(http, |m| {
                m.content(format!("📄 **Transcript** ({} chars)", output.len()))
                    .add_file(AttachmentType::Bytes {
                        data: Cow::Owned(file_bytes),
                        filename,
                    })
            })
            .await?;

        info!("Posted video {index} result");
        Ok(())
    }

    /// Post a failed video notice in the playlist thread
    pub async fn post_video_failed(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        index: u32,
        total: u32,
        video_title: &str,
        video_url: &str,
        error: &str,
    ) -> Result<()> {
        let content = format!(
            "---\n**[{}/{}] {}** ❌\n{}\n*Error: {}*",
            index,
            total,
            video_title,
            video_url,
            truncate_str(error, 200)
        );
        channel_id.say(http, &content).await?;
        Ok(())
    }

    /// Post the final playlist summary
    pub async fn post_playlist_summary(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        playlist_title: &str,
        completed: u32,
        failed: u32,
        skipped: u32,
        total: u32,
        runtime: std::time::Duration,
        combined_transcript: Option<&str>,
    ) -> Result<()> {
        let status_emoji = if failed == 0 { "✅" } else { "⚠️" };

        let runtime_str = crate::features::plugins::youtube::format_duration(runtime);

        let summary = format!(
            "---\n\n{status_emoji} **Playlist Complete: {playlist_title}**\n\n\
             • Successful: {completed} | Failed: {failed} | Skipped: {skipped}\n\
             • Total videos: {total}\n\
             • Runtime: {runtime_str}"
        );

        channel_id.say(http, &summary).await?;

        // Post combined transcript file if provided
        if let Some(transcript) = combined_transcript {
            let filename = format!("all_transcripts_{}.txt", sanitize_filename(playlist_title));
            let file_bytes = transcript.as_bytes().to_vec();

            channel_id
                .send_message(http, |m| {
                    m.content(format!(
                        "📚 **Combined transcripts** ({} chars)",
                        transcript.len()
                    ))
                    .add_file(AttachmentType::Bytes {
                        data: Cow::Owned(file_bytes),
                        filename,
                    })
                })
                .await?;

            info!("Posted combined transcript file");
        }

        info!("Posted playlist summary");
        Ok(())
    }

    /// Post a cancellation notice
    pub async fn post_playlist_cancelled(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        completed: u32,
        total: u32,
        cancelled_by: &str,
    ) -> Result<()> {
        let content = format!(
            "---\n\n🛑 **Playlist Cancelled**\n\n\
             • Processed: {completed}/{total} videos before cancellation\n\
             • Cancelled by: {cancelled_by}"
        );
        channel_id.say(http, &content).await?;
        info!("Posted playlist cancellation notice");
        Ok(())
    }

    // Chunked transcription methods

    /// Post or update a progress message for chunked transcription
    ///
    /// If `progress_message_id` is Some, edits that message. Otherwise posts a new message
    /// and returns the new message ID.
    pub async fn post_chunk_progress(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        progress_message_id: Option<MessageId>,
        current_chunk: usize,
        total_chunks: usize,
        status: &str,
        eta: Option<std::time::Duration>,
    ) -> Result<MessageId> {
        let eta_str = eta
            .map(crate::features::plugins::youtube::format_duration)
            .unwrap_or_else(|| "calculating...".to_string());

        let progress_bar = create_progress_bar(current_chunk, total_chunks);

        let content = format!(
            "⏳ **Transcribing:** {current_chunk}/{total_chunks} parts | {status} | ETA: {eta_str}\n{progress_bar}"
        );

        if let Some(msg_id) = progress_message_id {
            // Edit existing message
            channel_id
                .edit_message(http, msg_id, |m| m.content(&content))
                .await?;
            Ok(msg_id)
        } else {
            // Post new message
            let msg = channel_id.say(http, &content).await?;
            info!("Posted chunk progress message");
            Ok(msg.id)
        }
    }

    /// Post a chunk completion status
    pub async fn post_chunk_completed(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        chunk_num: usize,
        total_chunks: usize,
        transcript_preview: Option<&str>,
    ) -> Result<()> {
        let preview = transcript_preview
            .map(|t| {
                let truncated = if t.len() > 200 {
                    format!("{}...", &t[..200])
                } else {
                    t.to_string()
                };
                format!("\n> {}", truncated.replace('\n', "\n> "))
            })
            .unwrap_or_default();

        let content = format!("**Part {chunk_num} of {total_chunks}**{preview}");

        channel_id.say(http, &content).await?;
        Ok(())
    }

    /// Post a chunk failure notice
    pub async fn post_chunk_failed(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        chunk_num: usize,
        total_chunks: usize,
        error: &str,
    ) -> Result<()> {
        let content = format!(
            "⚠️ **Part {}/{}** failed: {}",
            chunk_num,
            total_chunks,
            truncate_str(error, 200)
        );

        channel_id.say(http, &content).await?;
        Ok(())
    }

    /// Post the final chunked transcription summary
    pub async fn post_chunked_summary(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        video_title: &str,
        completed_chunks: usize,
        failed_chunks: usize,
        total_chunks: usize,
        runtime: std::time::Duration,
        combined_transcript: Option<&str>,
        config: &OutputConfig,
        user_context: Option<&UserContext>,
    ) -> Result<()> {
        let status_emoji = if failed_chunks == 0 { "📝" } else { "⚠️" };
        let runtime_str = crate::features::plugins::youtube::format_duration(runtime);
        let word_count = combined_transcript.map(count_words).unwrap_or(0);
        let word_count_str = format_word_count(word_count);

        let summary = format!(
            "---\n\n{} **Transcription Complete: {}**\n\n\
             **Stats**\n\
             • Parts: {}/{} successful\n\
             • Words: {}\n\
             • Runtime: {}",
            status_emoji,
            truncate_str(video_title, 60),
            completed_chunks,
            total_chunks,
            word_count_str,
            runtime_str
        );

        channel_id.say(http, &summary).await?;

        // Generate and post AI summary if we have transcript
        if let Some(transcript) = combined_transcript {
            if let Some(ref prompt) = config.summary_prompt {
                match self
                    .generate_summary_with_tracking(
                        transcript,
                        prompt,
                        user_context,
                        Some("chunked_final_summary"),
                    )
                    .await
                {
                    Ok(ai_summary) => {
                        let summary_chunks = split_message(&ai_summary, 1900);
                        for chunk in summary_chunks {
                            channel_id.say(http, &chunk).await?;
                        }
                        info!("Posted AI summary for chunked transcription");
                    }
                    Err(e) => {
                        warn!("Failed to generate AI summary: {e}");
                    }
                }
            }

            // Post full transcript as file with sentence-per-line formatting
            let filename = config
                .file_name_template
                .as_deref()
                .unwrap_or("transcript.txt")
                .replace(
                    "${timestamp}",
                    &chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string(),
                );

            let formatted = format_transcript_sentences(transcript);
            let file_bytes = formatted.as_bytes().to_vec();
            channel_id
                .send_message(http, |m| {
                    m.content(format!("📄 **Full transcript** ({word_count} words)"))
                        .add_file(AttachmentType::Bytes {
                            data: Cow::Owned(file_bytes),
                            filename,
                        })
                })
                .await?;

            info!("Posted combined transcript file");
        }

        Ok(())
    }

    /// Post initial chunking status (download started)
    pub async fn post_chunking_started(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        video_title: &str,
    ) -> Result<MessageId> {
        let content = format!(
            "📥 **Downloading:** {} for transcription...",
            truncate_str(video_title, 60)
        );

        let msg = channel_id.say(http, &content).await?;
        Ok(msg.id)
    }

    /// Update status after download/split
    pub async fn post_chunks_ready(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        progress_message_id: Option<MessageId>,
        total_chunks: usize,
        estimated_duration: Option<std::time::Duration>,
    ) -> Result<MessageId> {
        let eta_str = estimated_duration
            .map(crate::features::plugins::youtube::format_duration)
            .unwrap_or_else(|| "calculating...".to_string());

        let content = format!(
            "📦 **Ready:** Split into {total_chunks} parts | Estimated time: {eta_str}"
        );

        if let Some(msg_id) = progress_message_id {
            channel_id
                .edit_message(http, msg_id, |m| m.content(&content))
                .await?;
            Ok(msg_id)
        } else {
            let msg = channel_id.say(http, &content).await?;
            Ok(msg.id)
        }
    }

    /// Post a file attachment to a channel
    pub async fn post_file(
        &self,
        http: &Arc<Http>,
        channel_id: ChannelId,
        content: &str,
        filename: &str,
    ) -> Result<()> {
        let file_bytes = content.as_bytes().to_vec();
        channel_id
            .send_message(http, |m| {
                m.add_file(AttachmentType::Bytes {
                    data: Cow::Owned(file_bytes),
                    filename: filename.to_string(),
                })
            })
            .await?;
        info!("Posted file attachment: {filename}");
        Ok(())
    }

    /// Generate an AI summary for a text (public wrapper)
    ///
    /// Returns None if summary generation fails.
    pub async fn generate_summary_for_text(
        &self,
        text: &str,
        prompt_template: &str,
    ) -> Option<String> {
        self.generate_summary_for_text_with_context(text, prompt_template, None, None)
            .await
    }

    /// Generate an AI summary for a text with user context for usage tracking
    ///
    /// Returns None if summary generation fails.
    pub async fn generate_summary_for_text_with_context(
        &self,
        text: &str,
        prompt_template: &str,
        user_context: Option<&UserContext>,
        request_context: Option<&str>,
    ) -> Option<String> {
        match self
            .generate_summary_with_tracking(text, prompt_template, user_context, request_context)
            .await
        {
            Ok(summary) => Some(summary),
            Err(e) => {
                warn!("Failed to generate summary: {e}");
                None
            }
        }
    }

    /// Generate an AI summary with usage tracking
    async fn generate_summary_with_tracking(
        &self,
        output: &str,
        prompt_template: &str,
        user_context: Option<&UserContext>,
        request_context: Option<&str>,
    ) -> Result<String> {
        // Truncate output for summary to avoid token limits
        let truncated = if output.len() > 8000 {
            format!(
                "{}...\n\n[Content truncated for summarization]",
                &output[..8000]
            )
        } else {
            output.to_string()
        };

        // Build the prompt
        let prompt = prompt_template.replace("${output}", &truncated);

        info!("Generating AI summary for output ({} chars)", output.len());

        let completion = ChatCompletion::builder(
            &self.openai_model,
            vec![
                ChatCompletionMessage {
                    role: ChatCompletionMessageRole::System,
                    content: Some(
                        "You are a helpful assistant that creates concise summaries. \
                         Keep summaries brief and focused on the key points."
                            .to_string(),
                    ),
                    name: None,
                    function_call: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
                ChatCompletionMessage {
                    role: ChatCompletionMessageRole::User,
                    content: Some(prompt),
                    name: None,
                    function_call: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
        )
        .create()
        .await?;

        // Log usage if tracker and user context are available
        if let (Some(tracker), Some(ctx), Some(usage)) =
            (&self.usage_tracker, user_context, &completion.usage)
        {
            let request_id = request_context.map(|c| format!("plugin_{c}"));
            tracker.log_chat(
                &self.openai_model,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                &ctx.user_id,
                ctx.guild_id.as_deref(),
                ctx.channel_id.as_deref(),
                request_id.as_deref(),
                CostBucket::Plugin,
            );
            info!(
                "Logged plugin AI usage: {} tokens for user {} ({})",
                usage.total_tokens,
                ctx.user_id,
                request_context.unwrap_or("summary")
            );
        }

        let summary = completion
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_else(|| "Summary unavailable.".to_string());

        Ok(summary)
    }
}

/// Escape markdown special characters in text
///
/// Characters escaped: * _ ` [ ] ( ) ~ > #
/// This is used to safely embed user-provided text (like video titles) in markdown
/// without breaking link syntax or other formatting.
pub fn escape_markdown(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '*' | '_' | '`' | '[' | ']' | '(' | ')' | '~' | '>' | '#' => {
                format!("\\{c}")
            }
            _ => c.to_string(),
        })
        .collect()
}

/// Truncate a string to max length, adding ellipsis if needed
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Create a text-based progress bar
fn create_progress_bar(current: usize, total: usize) -> String {
    const BAR_LENGTH: usize = 20;
    let filled = if total > 0 {
        (current * BAR_LENGTH) / total
    } else {
        0
    };
    let empty = BAR_LENGTH - filled;

    format!(
        "[{}{}] {}%",
        "█".repeat(filled),
        "░".repeat(empty),
        if total > 0 {
            (current * 100) / total
        } else {
            0
        }
    )
}

/// Sanitize a string for use as a filename
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .take(50)
        .collect::<String>()
        .trim()
        .replace(' ', "_")
        .to_lowercase()
}

/// Output format for transcripts and summaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Always post as Discord messages (split long content)
    Text,
    /// Always upload as files
    Files,
    /// Auto-detect based on length (default)
    #[default]
    Auto,
}

impl OutputFormat {
    /// Parse from string (from plugin parameter)
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "text" => OutputFormat::Text,
            "files" => OutputFormat::Files,
            _ => OutputFormat::Auto,
        }
    }

    /// Determine if content should be posted as a file based on format and length
    pub fn should_use_file(&self, content_len: usize) -> bool {
        match self {
            OutputFormat::Text => false,
            OutputFormat::Files => true,
            OutputFormat::Auto => content_len > 2000,
        }
    }
}

/// Format transcript text with one sentence per line
///
/// This creates a cleaner format for transcript files, making them easier to read
/// and process. The function handles common sentence terminators and preserves
/// paragraph breaks.
pub fn format_transcript_sentences(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 50);
    let mut buffer = String::new();

    for ch in text.chars() {
        buffer.push(ch);

        // Check for sentence endings
        if ch == '.' || ch == '!' || ch == '?' {
            // Trim and add as a line if non-empty
            let trimmed = buffer.trim();
            if !trimmed.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(trimmed);
            }
            buffer.clear();
        } else if ch == '\n' {
            // Preserve paragraph breaks (double newlines)
            let trimmed = buffer.trim();
            if !trimmed.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(trimmed);
            }
            buffer.clear();
        }
    }

    // Don't forget any remaining content
    let trimmed = buffer.trim();
    if !trimmed.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(trimmed);
    }

    result
}

/// Count words in text
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Format word count for display (e.g., "~12,345")
pub fn format_word_count(count: usize) -> String {
    if count >= 1000 {
        format!("~{},{:03}", count / 1000, count % 1000)
    } else {
        format!("~{count}")
    }
}

/// Split a message into chunks that fit within Discord's character limit
fn split_message(content: &str, max_len: usize) -> Vec<String> {
    if content.len() <= max_len {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        // Check if adding this line would exceed the limit
        if current.len() + line.len() + 1 > max_len {
            if !current.is_empty() {
                chunks.push(current);
            }

            // If a single line is too long, split it
            if line.len() > max_len {
                let mut remaining = line;
                while remaining.len() > max_len {
                    chunks.push(remaining[..max_len].to_string());
                    remaining = &remaining[max_len..];
                }
                current = remaining.to_string();
            } else {
                current = line.to_string();
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("hello world", 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hello world");
    }

    #[test]
    fn test_split_message_long() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";
        let chunks = split_message(content, 15);
        assert!(chunks.len() > 1);

        // Verify all content is preserved
        let rejoined: String = chunks.join("\n");
        // Some newlines may differ but content should be there
        assert!(rejoined.contains("line 1"));
        assert!(rejoined.contains("line 5"));
    }

    #[test]
    fn test_split_message_very_long_line() {
        let content = "a".repeat(100);
        let chunks = split_message(&content, 30);
        assert!(chunks.len() > 1);

        // Total length should match
        let total_len: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total_len, 100);
    }

    #[test]
    fn test_format_transcript_sentences() {
        let input = "Hello world. This is a test. Another sentence!";
        let result = format_transcript_sentences(input);
        assert_eq!(result, "Hello world.\nThis is a test.\nAnother sentence!");
    }

    #[test]
    fn test_format_transcript_sentences_with_questions() {
        let input = "What is this? It's a test. Really? Yes!";
        let result = format_transcript_sentences(input);
        assert_eq!(result, "What is this?\nIt's a test.\nReally?\nYes!");
    }

    #[test]
    fn test_format_transcript_sentences_preserves_incomplete() {
        let input = "This is complete. But this is not";
        let result = format_transcript_sentences(input);
        assert_eq!(result, "This is complete.\nBut this is not");
    }

    #[test]
    fn test_count_words() {
        assert_eq!(count_words("hello world"), 2);
        assert_eq!(count_words("one two three four five"), 5);
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("   spaced   out   "), 2);
    }

    #[test]
    fn test_format_word_count() {
        assert_eq!(format_word_count(500), "~500");
        assert_eq!(format_word_count(1000), "~1,000");
        assert_eq!(format_word_count(12345), "~12,345");
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("text"), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("TEXT"), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("files"), OutputFormat::Files);
        assert_eq!(OutputFormat::from_str("FILES"), OutputFormat::Files);
        assert_eq!(OutputFormat::from_str("auto"), OutputFormat::Auto);
        assert_eq!(OutputFormat::from_str("anything"), OutputFormat::Auto);
    }

    #[test]
    fn test_output_format_should_use_file() {
        assert!(!OutputFormat::Text.should_use_file(5000));
        assert!(OutputFormat::Files.should_use_file(100));
        assert!(!OutputFormat::Auto.should_use_file(1000));
        assert!(OutputFormat::Auto.should_use_file(3000));
    }

    #[test]
    fn test_escape_markdown() {
        // Basic text should be unchanged
        assert_eq!(escape_markdown("Hello World"), "Hello World");

        // Brackets and parens should be escaped (video title in link syntax)
        assert_eq!(
            escape_markdown("[Tutorial] Learn Rust (Part 1)"),
            "\\[Tutorial\\] Learn Rust \\(Part 1\\)"
        );

        // Asterisks and underscores should be escaped
        assert_eq!(
            escape_markdown("Test *emphasis* and _underline_"),
            "Test \\*emphasis\\* and \\_underline\\_"
        );

        // All special characters
        assert_eq!(
            escape_markdown("*_`[]()~>#"),
            "\\*\\_\\`\\[\\]\\(\\)\\~\\>\\#"
        );

        // Empty string
        assert_eq!(escape_markdown(""), "");
    }
}
