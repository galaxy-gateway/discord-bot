//! # Feature: Startup Notification
//!
//! Sends rich embed notifications when bot comes online.
//! Supports DM to bot owner and/or specific guild channels.
//! Configuration is stored in the database and managed via /set_guild_setting.
//!
//! - **Version**: 1.5.0
//! - **Since**: 0.4.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.5.0: Export format_commit_for_thread for /commits slash command thread posting
//! - 1.4.0: Export CommitInfo and get_detailed_commits for /commits slash command
//! - 1.3.0: Multi-column layout for features and plugins to reduce embed verbosity
//! - 1.2.0: Added plugin versions to embed, detailed commit thread for channels, inline commits for DMs
//! - 1.1.0: Moved configuration from env vars to database
//! - 1.0.0: Initial release with DM and channel support, rich embeds

use crate::database::Database;
use crate::features::plugins::Plugin;
use crate::features::{get_bot_version, get_features};
use log::{info, warn};
use serenity::builder::CreateEmbed;
use serenity::http::Http;
use serenity::model::channel::ChannelType;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, UserId};
use serenity::utils::Color;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::Command;

/// Git commits embedded at compile time by build.rs
const RECENT_COMMITS: &str = env!("GIT_RECENT_COMMITS");

/// Tracks whether this is the first Ready event (vs reconnect)
static FIRST_READY: AtomicBool = AtomicBool::new(true);

/// Detailed commit information fetched at runtime
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub subject: String,
    pub body: String,
    pub files: Vec<String>,
}

/// Fetches detailed commit information at runtime via git
///
/// # Arguments
/// * `count` - Number of commits to fetch (1-10 recommended)
pub async fn get_detailed_commits(count: usize) -> Vec<CommitInfo> {
    // Run git log with full info
    let count_arg = format!("-{}", count.clamp(1, 20));
    let output = match Command::new("git")
        .args([
            "log",
            &count_arg,
            "--format=COMMIT_START%n%h%n%s%n%b%nFILES_START",
            "--name-only",
        ])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            warn!("Failed to run git log: {}", e);
            return vec![];
        }
    };

    if !output.status.success() {
        warn!("git log exited with non-zero status");
        return vec![];
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_detailed_commits(&stdout)
}

/// Parse git log output into CommitInfo structs
fn parse_detailed_commits(output: &str) -> Vec<CommitInfo> {
    let mut commits = Vec::new();

    // Split by COMMIT_START marker
    for commit_block in output.split("COMMIT_START").skip(1) {
        // Filter out empty lines and collect non-empty ones
        let lines: Vec<&str> = commit_block
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() || *l == "FILES_START")
            .collect();

        if lines.len() < 2 {
            continue;
        }

        let hash = lines[0].to_string();
        let subject = lines.get(1).map(|s| s.to_string()).unwrap_or_default();

        // Find where FILES_START begins
        let files_start_idx = lines.iter().position(|l| *l == "FILES_START");

        // Body is everything between subject and FILES_START
        let body = if let Some(idx) = files_start_idx {
            if idx > 2 {
                lines[2..idx].join("\n")
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Files are everything after FILES_START
        let files: Vec<String> = if let Some(idx) = files_start_idx {
            lines[idx + 1..]
                .iter()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        } else {
            vec![]
        };

        if !hash.is_empty() {
            commits.push(CommitInfo {
                hash,
                subject,
                body,
                files,
            });
        }
    }

    commits
}

/// Formats a single commit for thread display
///
/// Creates a nicely formatted message with the commit subject, hash, body, and files changed.
/// This is used by both startup notifications and the /commits command.
pub fn format_commit_for_thread(commit: &CommitInfo) -> String {
    let mut msg = format!("**{}** (`{}`)\n", commit.subject, commit.hash);

    if !commit.body.is_empty() {
        msg.push_str(&format!("\n{}\n", commit.body));
    }

    if !commit.files.is_empty() {
        // Show all files, but use code block for better formatting
        msg.push_str("\n**Files changed:**\n```\n");
        for file in &commit.files {
            msg.push_str(file);
            msg.push('\n');
        }
        msg.push_str("```");
    }

    // Truncate if too long for Discord
    if msg.len() > 1900 {
        msg.truncate(1900);
        msg.push_str("\n... (truncated)");
    }

    msg
}

/// Handles sending startup notifications to configured destinations
pub struct StartupNotifier {
    database: Arc<Database>,
}

impl StartupNotifier {
    /// Creates a new StartupNotifier with database access
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    /// Sends startup notifications if enabled and this is the first Ready event
    pub async fn send_if_enabled(&self, http: &Http, ready: &Ready, plugins: &[Plugin]) {
        // Only send on first Ready (not reconnects)
        if !FIRST_READY.swap(false, Ordering::SeqCst) {
            info!("Skipping startup notification (reconnect, not initial startup)");
            return;
        }

        // Read settings from database
        let enabled = self
            .database
            .get_bot_setting("startup_notification")
            .await
            .ok()
            .flatten()
            .map(|v| v == "enabled")
            .unwrap_or(false);

        if !enabled {
            info!("Startup notifications disabled");
            return;
        }

        let owner_id = self
            .database
            .get_bot_setting("startup_notify_owner_id")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());

        let channel_id = self
            .database
            .get_bot_setting("startup_notify_channel_id")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());

        if owner_id.is_none() && channel_id.is_none() {
            info!("Startup notifications enabled but no destinations configured");
            return;
        }

        let embed = Self::build_embed(ready, plugins);

        // Send to owner DM (includes commit messages inline since DMs can't have threads)
        if let Some(oid) = owner_id {
            if let Err(e) = Self::send_to_owner(http, oid, embed.clone()).await {
                warn!("Failed to send startup DM to owner {}: {}", oid, e);
            }
        }

        // Send to channel (creates a thread with detailed commit info)
        if let Some(cid) = channel_id {
            if let Err(e) = Self::send_to_channel(http, cid, embed).await {
                warn!(
                    "Failed to send startup notification to channel {}: {}",
                    cid, e
                );
            }
        }
    }

    /// Builds the rich embed for the startup notification
    fn build_embed(ready: &Ready, plugins: &[Plugin]) -> CreateEmbed {
        let version = get_bot_version();
        let features = get_features();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut embed = CreateEmbed::default();

        // Title and color
        embed
            .title(format!("{} is Online!", ready.user.name))
            .color(Color::from_rgb(87, 242, 135)); // Discord green

        // Basic info fields (inline)
        embed.field("Version", format!("`v{}`", version), true);
        embed.field("Guilds", ready.guilds.len().to_string(), true);

        // Shard info if available
        if let Some(shard) = ready.shard {
            embed.field("Shard", format!("{}/{}", shard[0] + 1, shard[1]), true);
        }

        // Feature versions in multi-column layout (2-3 columns)
        let feature_items: Vec<String> = features
            .iter()
            .map(|f| format!("{} `{}`", f.name, f.version))
            .collect();
        add_multi_column_fields(&mut embed, "Features", &feature_items, 3);

        // Plugin versions in multi-column layout
        let enabled_plugins: Vec<_> = plugins.iter().filter(|p| p.enabled).collect();
        if !enabled_plugins.is_empty() {
            let plugin_items: Vec<String> = enabled_plugins
                .iter()
                .map(|p| format!("/{} `{}`", p.command.name, p.version))
                .collect();
            add_multi_column_fields(&mut embed, "Plugins", &plugin_items, 3);
        }

        // Recent changes from git commits (summary only - detailed view in thread/DM follow-up)
        if !RECENT_COMMITS.is_empty() {
            let changes: String = RECENT_COMMITS
                .lines()
                .take(3)
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(2, '|').collect();
                    if parts.len() == 2 {
                        Some(format!("- {} (`{}`)", parts[1], parts[0]))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !changes.is_empty() {
                embed.field("Recent Changes", changes, false);
            }
        }

        // Footer with timestamp
        embed.footer(|f| f.text(format!("Started <t:{}:R>", timestamp)));

        // Bot avatar as thumbnail
        if let Some(url) = ready.user.avatar_url() {
            embed.thumbnail(url);
        }

        embed
    }

    /// Sends the embed to the bot owner via DM with inline commit details
    async fn send_to_owner(http: &Http, owner_id: u64, embed: CreateEmbed) -> anyhow::Result<()> {
        let user = UserId(owner_id);
        let dm = user.create_dm_channel(http).await?;
        dm.send_message(http, |m| m.set_embed(embed)).await?;
        info!("Sent startup notification to owner {} via DM", owner_id);

        // Send detailed commit info as follow-up message (DMs can't have threads)
        let commits = get_detailed_commits(5).await;
        if !commits.is_empty() {
            let commit_text = Self::format_commits_for_dm(&commits);
            if !commit_text.is_empty() {
                dm.send_message(http, |m| m.content(&commit_text)).await?;
                info!("Sent detailed commit info to owner {} via DM", owner_id);
            }
        }

        Ok(())
    }

    /// Formats commit info for inline display in DMs
    fn format_commits_for_dm(commits: &[CommitInfo]) -> String {
        let mut result = String::from("**Recent Changes (Detailed):**\n");

        for commit in commits {
            result.push_str(&format!("\n**{}** (`{}`)\n", commit.subject, commit.hash));

            if !commit.body.is_empty() {
                // Truncate body if too long for DM
                let body = if commit.body.len() > 200 {
                    format!("{}...", &commit.body[..200])
                } else {
                    commit.body.clone()
                };
                result.push_str(&format!("{}\n", body));
            }

            if !commit.files.is_empty() {
                let files_preview: Vec<_> = commit.files.iter().take(5).collect();
                let files_str = files_preview
                    .iter()
                    .map(|f| format!("  `{}`", f))
                    .collect::<Vec<_>>()
                    .join("\n");

                if commit.files.len() > 5 {
                    result.push_str(&format!(
                        "Files changed:\n{}\n  ... and {} more\n",
                        files_str,
                        commit.files.len() - 5
                    ));
                } else {
                    result.push_str(&format!("Files changed:\n{}\n", files_str));
                }
            }
        }

        // Truncate entire message if too long for Discord
        if result.len() > 1900 {
            result.truncate(1900);
            result.push_str("\n... (truncated)");
        }

        result
    }

    /// Sends the embed to a specific channel and creates a thread with detailed commit info
    async fn send_to_channel(
        http: &Http,
        channel_id: u64,
        embed: CreateEmbed,
    ) -> anyhow::Result<()> {
        let channel = ChannelId(channel_id);
        let msg = channel.send_message(http, |m| m.set_embed(embed)).await?;
        info!("Sent startup notification to channel {}", channel_id);

        // Create a thread for detailed commit info
        let commits = get_detailed_commits(5).await;
        if !commits.is_empty() {
            // Create thread from the message
            match channel
                .create_public_thread(http, msg.id, |t| {
                    t.name("Recent Changes")
                        .kind(ChannelType::PublicThread)
                        .auto_archive_duration(1440) // 24 hours
                })
                .await
            {
                Ok(thread) => {
                    info!(
                        "Created thread '{}' for commit details in channel {}",
                        thread.name(),
                        channel_id
                    );

                    // Post each commit as a separate message in the thread
                    Self::post_detailed_commits_to_thread(http, ChannelId(thread.id.0), &commits)
                        .await;
                }
                Err(e) => {
                    warn!("Failed to create thread for commit details: {}", e);
                    // Fall back to posting in the channel directly
                    Self::post_detailed_commits_to_channel(http, channel, &commits).await;
                }
            }
        }

        Ok(())
    }

    /// Posts detailed commit information to a thread
    async fn post_detailed_commits_to_thread(
        http: &Http,
        thread_id: ChannelId,
        commits: &[CommitInfo],
    ) {
        for commit in commits {
            let msg = format_commit_for_thread(commit);
            if let Err(e) = thread_id.say(http, &msg).await {
                warn!("Failed to post commit {} to thread: {}", commit.hash, e);
            }
        }
    }

    /// Posts detailed commit information directly to a channel (fallback)
    async fn post_detailed_commits_to_channel(
        http: &Http,
        channel_id: ChannelId,
        commits: &[CommitInfo],
    ) {
        // Combine all commits into one message for channel fallback
        let mut combined = String::from("**Recent Changes (Detailed):**\n");
        for commit in commits.iter().take(3) {
            combined.push_str(&format!("\n**{}** (`{}`)", commit.subject, commit.hash));
            if !commit.files.is_empty() {
                let files_preview: Vec<_> = commit.files.iter().take(3).collect();
                combined.push_str(&format!(
                    " - {} file(s)",
                    if commit.files.len() > 3 {
                        format!("{} (showing 3)", commit.files.len())
                    } else {
                        commit.files.len().to_string()
                    }
                ));
                combined.push_str(&format!(
                    "\n  `{}`",
                    files_preview
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join("`, `")
                ));
            }
        }

        if combined.len() > 1900 {
            combined.truncate(1900);
            combined.push_str("\n... (truncated)");
        }

        if let Err(e) = channel_id.say(http, &combined).await {
            warn!("Failed to post commit details to channel: {}", e);
        }
    }

}

/// Adds items as multiple inline embed fields for column layout
///
/// Splits items into the specified number of columns (max 3 for Discord inline).
/// Each column gets a portion of the items, creating a multi-column appearance.
fn add_multi_column_fields(embed: &mut CreateEmbed, title: &str, items: &[String], max_columns: usize) {
    if items.is_empty() {
        return;
    }

    // For small lists, use fewer columns
    let num_columns = if items.len() <= 3 {
        1
    } else if items.len() <= 6 {
        2.min(max_columns)
    } else {
        max_columns.min(3) // Discord max inline is effectively 3
    };

    let items_per_column = (items.len() + num_columns - 1) / num_columns;

    for (col_idx, chunk) in items.chunks(items_per_column).enumerate() {
        let column_content = chunk.join("\n");
        let field_title = if col_idx == 0 {
            title.to_string()
        } else {
            "\u{200B}".to_string() // Zero-width space for continuation columns
        };
        embed.field(&field_title, column_content, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recent_commits_parsing() {
        // Test that the compile-time commits are available
        // (may be empty if built without git)
        let _ = RECENT_COMMITS;
    }

    #[test]
    fn test_commit_line_parsing() {
        let line = "abc1234|feat: add new feature";
        let parts: Vec<&str> = line.splitn(2, '|').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "abc1234");
        assert_eq!(parts[1], "feat: add new feature");
    }

    #[test]
    fn test_parse_detailed_commits_basic() {
        let output = "COMMIT_START\nabc1234\nfeat: add new feature\n\nFILES_START\nsrc/main.rs\nsrc/lib.rs\n";
        let commits = parse_detailed_commits(output);

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].subject, "feat: add new feature");
        assert!(commits[0].body.is_empty());
        assert_eq!(commits[0].files.len(), 2);
        assert_eq!(commits[0].files[0], "src/main.rs");
        assert_eq!(commits[0].files[1], "src/lib.rs");
    }

    #[test]
    fn test_parse_detailed_commits_with_body() {
        let output = "COMMIT_START\ndef5678\nfix: resolve bug\nThis commit fixes a bug\nin the authentication flow.\nFILES_START\nsrc/auth.rs\n";
        let commits = parse_detailed_commits(output);

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "def5678");
        assert_eq!(commits[0].subject, "fix: resolve bug");
        assert!(commits[0].body.contains("authentication flow"));
        assert_eq!(commits[0].files.len(), 1);
    }

    #[test]
    fn test_parse_detailed_commits_multiple() {
        let output = "COMMIT_START\nabc1234\nfeat: first\n\nFILES_START\na.rs\nCOMMIT_START\ndef5678\nfix: second\n\nFILES_START\nb.rs\n";
        let commits = parse_detailed_commits(output);

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].subject, "feat: first");
        assert_eq!(commits[1].hash, "def5678");
        assert_eq!(commits[1].subject, "fix: second");
    }

    #[test]
    fn test_parse_detailed_commits_empty() {
        let commits = parse_detailed_commits("");
        assert!(commits.is_empty());
    }

    #[test]
    fn test_format_commit_for_thread() {
        let commit = CommitInfo {
            hash: "abc1234".to_string(),
            subject: "feat: add new feature".to_string(),
            body: "This is the body".to_string(),
            files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
        };

        let formatted = format_commit_for_thread(&commit);
        assert!(formatted.contains("feat: add new feature"));
        assert!(formatted.contains("abc1234"));
        assert!(formatted.contains("This is the body"));
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("src/lib.rs"));
    }

    #[test]
    fn test_format_commits_for_dm() {
        let commits = vec![CommitInfo {
            hash: "abc1234".to_string(),
            subject: "feat: add new feature".to_string(),
            body: String::new(),
            files: vec!["src/main.rs".to_string()],
        }];

        let formatted = StartupNotifier::format_commits_for_dm(&commits);
        assert!(formatted.contains("Recent Changes"));
        assert!(formatted.contains("feat: add new feature"));
        assert!(formatted.contains("abc1234"));
        assert!(formatted.contains("src/main.rs"));
    }
}
