//! Fetch command handler
//!
//! Handles: fetch
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.2.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use scraper::{Html, Selector};
use serenity::builder::CreateEmbed;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::prelude::Context;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::commands::context::CommandContext;
use crate::commands::handler::SlashCommandHandler;
use crate::commands::slash::get_string_option;
use crate::core::{chunk_for_embed, truncate_for_embed};
use crate::features::analytics::CostBucket;
use crate::features::personas::Persona;

/// Maximum size of raw HTML to download (5 MB)
const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;

/// Maximum characters of extracted text to send to OpenAI
const MAX_EXTRACTED_CHARS: usize = 100_000;

/// HTTP request timeout in seconds
const FETCH_TIMEOUT_SECS: u64 = 15;

pub struct FetchHandler;

#[async_trait]
impl SlashCommandHandler for FetchHandler {
    fn command_names(&self) -> &'static [&'static str] {
        &["fetch"]
    }

    async fn handle(
        &self,
        ctx: Arc<CommandContext>,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
    ) -> Result<()> {
        let request_id = Uuid::new_v4();
        self.handle_fetch(&ctx, serenity_ctx, command, request_id)
            .await
    }
}

impl FetchHandler {
    async fn handle_fetch(
        &self,
        ctx: &CommandContext,
        serenity_ctx: &Context,
        command: &ApplicationCommandInteraction,
        request_id: Uuid,
    ) -> Result<()> {
        let start_time = Instant::now();

        // Extract options
        let url = get_string_option(&command.data.options, "url")
            .ok_or_else(|| anyhow::anyhow!("Missing url argument"))?;
        let question = get_string_option(&command.data.options, "question");

        let user_id = command.user.id.to_string();
        let channel_id = command.channel_id.to_string();
        let guild_id = command.guild_id.map(|id| id.to_string());

        info!(
            "[{request_id}] /fetch command | URL: {} | Question: {} | User: {}",
            url,
            question.as_deref().unwrap_or("(summary)"),
            user_id
        );

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            command
                .create_interaction_response(&serenity_ctx.http, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|m| {
                            m.content("URL must start with `http://` or `https://`")
                                .ephemeral(true)
                        })
                })
                .await?;
            return Ok(());
        }

        // Defer response (fetching + AI call will take time)
        info!("[{request_id}] Deferring interaction response");
        command
            .create_interaction_response(&serenity_ctx.http, |r| {
                r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .map_err(|e| {
                error!("[{request_id}] Failed to defer interaction: {e}");
                anyhow::anyhow!("Failed to defer interaction: {e}")
            })?;

        // Fetch the webpage
        info!("[{request_id}] Fetching URL: {url}");
        let html_content = match Self::fetch_url(&url, request_id).await {
            Ok(html) => html,
            Err(e) => {
                error!("[{request_id}] Failed to fetch URL: {e}");
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |r| {
                        r.content(format!("Failed to fetch the webpage: {e}"))
                    })
                    .await?;
                return Ok(());
            }
        };

        // Extract text from HTML
        let extracted_text = Self::extract_text(&html_content, request_id);
        if extracted_text.trim().is_empty() {
            warn!("[{request_id}] No text content extracted from URL");
            command
                .edit_original_interaction_response(&serenity_ctx.http, |r| {
                    r.content("Could not extract any readable text from that webpage.")
                })
                .await?;
            return Ok(());
        }

        info!(
            "[{request_id}] Extracted {} characters of text",
            extracted_text.len()
        );

        // Resolve user's active persona (channel -> user -> guild -> env -> fallback)
        let persona_id = if let Some(gid) = guild_id.as_deref() {
            ctx.database
                .get_persona_with_channel(&user_id, gid, &channel_id)
                .await?
        } else {
            ctx.database
                .get_user_persona_with_guild(&user_id, None)
                .await?
        };

        let persona = ctx.persona_manager.get_persona_with_portrait(&persona_id);
        let system_prompt = ctx.persona_manager.get_system_prompt(&persona_id, None);

        debug!("[{request_id}] Using persona: {persona_id}");

        // Build user message with page content
        let user_message = Self::build_user_message(&url, &extracted_text, question.as_deref());

        // Log usage
        ctx.database
            .log_usage(&user_id, "fetch", Some(&persona_id))
            .await?;

        // Get AI response
        info!("[{request_id}] Calling OpenAI API");
        let ai_response = ctx
            .get_ai_response(
                &system_prompt,
                &user_message,
                Vec::new(),
                request_id,
                Some(&user_id),
                guild_id.as_deref(),
                Some(&channel_id),
                CostBucket::Fetch,
            )
            .await;

        match ai_response {
            Ok(response) => {
                let processing_time = start_time.elapsed();
                info!(
                    "[{request_id}] Response received | Time: {:?} | Length: {}",
                    processing_time,
                    response.len()
                );

                if let Some(ref p) = persona {
                    let chunks = chunk_for_embed(&response);
                    if chunks.len() > 1 {
                        debug!("[{request_id}] Response split into {} chunks", chunks.len());

                        if let Some(first_chunk) = chunks.first() {
                            let embed =
                                Self::build_fetch_embed(p, first_chunk, &url, question.as_deref());
                            command
                                .edit_original_interaction_response(&serenity_ctx.http, |r| {
                                    r.set_embed(embed)
                                })
                                .await?;
                        }

                        for chunk in chunks.iter().skip(1) {
                            if !chunk.trim().is_empty() {
                                let embed = Self::build_continuation_embed(p, chunk);
                                command
                                    .create_followup_message(&serenity_ctx.http, |m| {
                                        m.set_embed(embed)
                                    })
                                    .await?;
                            }
                        }
                    } else {
                        let embed =
                            Self::build_fetch_embed(p, &response, &url, question.as_deref());
                        command
                            .edit_original_interaction_response(&serenity_ctx.http, |r| {
                                r.set_embed(embed)
                            })
                            .await?;
                    }
                } else {
                    // Fallback: no persona found, plain text
                    command
                        .edit_original_interaction_response(&serenity_ctx.http, |r| {
                            r.content(&response)
                        })
                        .await?;
                }

                info!("[{request_id}] /fetch response sent successfully");
            }
            Err(e) => {
                error!("[{request_id}] AI response failed: {e}");
                command
                    .edit_original_interaction_response(&serenity_ctx.http, |r| {
                        r.content("Sorry, I could not process that webpage. Please try again.")
                    })
                    .await?;
            }
        }

        Ok(())
    }

    /// Fetch HTML content from a URL
    async fn fetch_url(url: &str, request_id: Uuid) -> Result<String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("Mozilla/5.0 (compatible; PersonaBot/1.0)")
            .build()?;

        let response = client.get(url).send().await.map_err(|e| {
            if e.is_timeout() {
                anyhow::anyhow!("Request timed out after {FETCH_TIMEOUT_SECS} seconds")
            } else if e.is_connect() {
                anyhow::anyhow!("Could not connect to the server")
            } else {
                anyhow::anyhow!("HTTP request failed: {e}")
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("Server returned HTTP {status}"));
        }

        // Check content length before downloading
        if let Some(content_length) = response.content_length() {
            if content_length > MAX_HTML_BYTES as u64 {
                return Err(anyhow::anyhow!(
                    "Page is too large ({} bytes, max {} bytes)",
                    content_length,
                    MAX_HTML_BYTES
                ));
            }
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.is_empty()
            && !content_type.contains("text/html")
            && !content_type.contains("text/plain")
            && !content_type.contains("application/xhtml")
        {
            debug!("[{request_id}] Non-HTML content type: {content_type}");
        }

        let bytes = response.bytes().await?;
        if bytes.len() > MAX_HTML_BYTES {
            return Err(anyhow::anyhow!(
                "Page is too large ({} bytes, max {} bytes)",
                bytes.len(),
                MAX_HTML_BYTES
            ));
        }

        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    /// Extract meaningful text content from HTML
    fn extract_text(html: &str, request_id: Uuid) -> String {
        let document = Html::parse_document(html);

        // Try to find main content areas first
        let main_selectors = ["main", "article", "[role=main]", "#content", ".content"];
        let mut text = String::new();

        for selector_str in &main_selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                for element in document.select(&selector) {
                    let element_text = Self::extract_element_text(&element);
                    if element_text.len() > 100 {
                        text.push_str(&element_text);
                        text.push('\n');
                    }
                }
            }
            if text.len() > 200 {
                break;
            }
        }

        // Fallback: extract from body, skipping nav/header/footer/script/style
        if text.len() < 200 {
            debug!("[{request_id}] Falling back to body text extraction");
            text.clear();
            if let Ok(body_selector) = Selector::parse("body") {
                for body in document.select(&body_selector) {
                    text = Self::extract_element_text(&body);
                }
            }
        }

        // Truncate if too long
        if text.len() > MAX_EXTRACTED_CHARS {
            text.truncate(MAX_EXTRACTED_CHARS);
            text.push_str(&format!(
                "\n\n[... truncated to first {MAX_EXTRACTED_CHARS} characters ...]"
            ));
        }

        // Collapse excessive whitespace
        let collapsed = regex::Regex::new(r"\n{3,}")
            .unwrap()
            .replace_all(&text, "\n\n");
        collapsed.trim().to_string()
    }

    /// Extract text from an HTML element, skipping unwanted tags
    fn extract_element_text(element: &scraper::ElementRef) -> String {
        use scraper::Node;

        let skip_tags = [
            "script", "style", "nav", "header", "footer", "aside", "noscript", "svg", "iframe",
            "form",
        ];

        let mut text = String::new();

        for node in element.descendants() {
            match node.value() {
                Node::Text(t) => {
                    // Check if any ancestor is a skip tag
                    let should_skip = node.ancestors().any(|ancestor| {
                        ancestor
                            .value()
                            .as_element()
                            .map(|el| skip_tags.contains(&el.name.local.as_ref()))
                            .unwrap_or(false)
                    });

                    if !should_skip {
                        let trimmed = t.trim();
                        if !trimmed.is_empty() {
                            text.push_str(trimmed);
                            text.push(' ');
                        }
                    }
                }
                Node::Element(el) => {
                    let block_tags = [
                        "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "br", "tr",
                        "blockquote", "pre",
                    ];
                    if block_tags.contains(&el.name.local.as_ref()) {
                        text.push('\n');
                    }
                }
                _ => {}
            }
        }

        text
    }

    /// Build the user message containing page content and instructions
    fn build_user_message(url: &str, content: &str, question: Option<&str>) -> String {
        let instruction = match question {
            Some(q) => format!(
                "A user has asked the following question about this webpage: {q}\n\n\
                 Answer the question based on the webpage content below. Stay in character."
            ),
            None => "Provide a helpful summary of the following webpage content. \
                     Highlight the key points, main topics, and any important details. \
                     Stay in character."
                .to_string(),
        };

        format!(
            "{instruction}\n\n\
             ---\n\
             Webpage URL: {url}\n\
             Webpage Content:\n\
             {content}\n\
             ---"
        )
    }

    /// Build embed with fetch-specific footer showing URL
    fn build_fetch_embed(
        persona: &Persona,
        response_text: &str,
        url: &str,
        question: Option<&str>,
    ) -> CreateEmbed {
        let mut embed = CreateEmbed::default();
        embed.author(|a| {
            a.name(&persona.name);
            if let Some(portrait_url) = &persona.portrait_url {
                a.icon_url(portrait_url);
            }
            a
        });
        embed.color(persona.color);
        embed.description(truncate_for_embed(response_text));

        // Truncate URL for footer if needed (Discord footer limit is 2048 chars)
        let display_url: String = if url.len() > 200 {
            format!("{}...", &url[..197])
        } else {
            url.to_string()
        };

        if let Some(q) = question {
            let truncated_q: String = q.chars().take(100).collect();
            embed.footer(|f| f.text(format!("Q: {truncated_q} | {display_url}")));
        } else {
            embed.footer(|f| f.text(format!("Summary of {display_url}")));
        }

        embed
    }

    fn build_continuation_embed(persona: &Persona, response_text: &str) -> CreateEmbed {
        let mut embed = CreateEmbed::default();
        embed.color(persona.color);
        embed.description(response_text);
        embed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_handler_commands() {
        let handler = FetchHandler;
        let names = handler.command_names();
        assert!(names.contains(&"fetch"));
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn test_extract_text_basic() {
        let html = "<html><body><p>Hello world</p><script>var x = 1;</script></body></html>";
        let text = FetchHandler::extract_text(html, Uuid::new_v4());
        assert!(text.contains("Hello world"));
        assert!(!text.contains("var x"));
    }

    #[test]
    fn test_extract_text_skips_nav() {
        let html =
            "<html><body><nav>Menu items</nav><main><p>Main content here</p></main></body></html>";
        let text = FetchHandler::extract_text(html, Uuid::new_v4());
        assert!(text.contains("Main content"));
        assert!(!text.contains("Menu items"));
    }

    #[test]
    fn test_extract_text_empty_page() {
        let html = "<html><body></body></html>";
        let text = FetchHandler::extract_text(html, Uuid::new_v4());
        assert!(text.trim().is_empty());
    }

    #[test]
    fn test_build_user_message_summary() {
        let msg = FetchHandler::build_user_message("https://example.com", "Some content", None);
        assert!(msg.contains("summary"));
        assert!(msg.contains("https://example.com"));
        assert!(msg.contains("Some content"));
    }

    #[test]
    fn test_build_user_message_question() {
        let msg = FetchHandler::build_user_message(
            "https://example.com",
            "Some content",
            Some("What is this about?"),
        );
        assert!(msg.contains("What is this about?"));
        assert!(msg.contains("https://example.com"));
    }
}
