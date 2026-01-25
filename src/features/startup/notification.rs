//! # Feature: Startup Notification
//!
//! Sends rich embed notifications when bot comes online.
//! Supports DM to bot owner and/or specific guild channels.
//! Configuration is stored in the database and managed via /set_guild_setting.
//!
//! - **Version**: 1.9.0
//! - **Since**: 0.4.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.9.0: Store DM notifications in conversation history for context continuity
//! - 1.8.0: Add configurable commit counts for DM and channel notifications
//! - 1.7.0: Add GitHub commit links to startup notification and thread messages
//! - 1.6.0: Include updateMessage.txt content in startup embed, removed feature version columns
//! - 1.5.0: Export format_commit_for_thread for /commits slash command thread posting
//! - 1.4.0: Export CommitInfo and get_detailed_commits for /commits slash command
//! - 1.3.0: Multi-column layout for features and plugins to reduce embed verbosity
//! - 1.2.0: Added plugin versions to embed, detailed commit thread for channels, inline commits for DMs
//! - 1.1.0: Moved configuration from env vars to database
//! - 1.0.0: Initial release with DM and channel support, rich embeds

use crate::database::Database;
use crate::features::plugins::Plugin;
use crate::features::get_bot_version;
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

/// Gets the GitHub repository URL from git remote origin
///
/// Parses both SSH (git@github.com:user/repo.git) and HTTPS formats
/// Returns the base URL like `https://github.com/user/repo`
pub async fn get_github_repo_url() -> Option<String> {
    let output = match Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            warn!("Failed to get git remote URL: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_github_url(&url)
}

/// Parses a git remote URL into a GitHub web URL
fn parse_github_url(url: &str) -> Option<String> {
    // Handle SSH format: git@github.com:user/repo.git
    if url.starts_with("git@github.com:") {
        let path = url.strip_prefix("git@github.com:")?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        return Some(format!("https://github.com/{}", path));
    }

    // Handle HTTPS format: https://github.com/user/repo.git
    if url.starts_with("https://github.com/") {
        let path = url.strip_prefix("https://github.com/")?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        return Some(format!("https://github.com/{}", path));
    }

    None
}

/// Formats a single commit for thread display
///
/// Creates a nicely formatted message with the commit subject, hash, body, and files changed.
/// This is used by both startup notifications and the /commits command.
/// If repo_url is provided, the hash will be a clickable link to the commit.
pub fn format_commit_for_thread(commit: &CommitInfo, repo_url: Option<&str>) -> String {
    let hash_display = if let Some(url) = repo_url {
        format!("[`{}`]({}/commit/{})", commit.hash, url, commit.hash)
    } else {
        format!("`{}`", commit.hash)
    };

    let mut msg = format!("**{}** ({})\n", commit.subject, hash_display);

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
        let notification_text = Self::build_notification_text(ready, plugins);

        // Send to owner DM (includes commit messages inline since DMs can't have threads)
        if let Some(oid) = owner_id {
            if let Err(e) = self.send_to_owner(http, oid, embed.clone(), &notification_text).await {
                warn!("Failed to send startup DM to owner {}: {}", oid, e);
            }
        }

        // Send to channel (creates a thread with detailed commit info)
        if let Some(cid) = channel_id {
            if let Err(e) = self.send_to_channel(http, cid, embed).await {
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
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut embed = CreateEmbed::default();

        // Title and color
        embed
            .title(format!("{} is Online!", ready.user.name))
            .color(Color::from_rgb(87, 242, 135)); // Discord green

        // Include updateMessage.txt content as description if it exists
        if let Ok(update_message) = std::fs::read_to_string("updateMessage.txt") {
            let trimmed = update_message.trim();
            if !trimmed.is_empty() {
                // Discord embed description limit is 4096 chars
                let description = if trimmed.len() > 4000 {
                    format!("{}...", &trimmed[..4000])
                } else {
                    trimmed.to_string()
                };
                embed.description(description);
            }
        }

        // Basic info fields (inline)
        embed.field("Version", format!("`v{}`", version), true);
        embed.field("Guilds", ready.guilds.len().to_string(), true);

        // Shard info if available
        if let Some(shard) = ready.shard {
            embed.field("Shard", format!("{}/{}", shard[0] + 1, shard[1]), true);
        }

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

    /// Builds a plain text representation of the startup notification for conversation history
    fn build_notification_text(ready: &Ready, plugins: &[Plugin]) -> String {
        let version = get_bot_version();
        let mut text = format!(
            "🟢 **{} is Online!**\n\nVersion: v{}\nConnected to {} guild(s)",
            ready.user.name, version, ready.guilds.len()
        );

        // Include updateMessage.txt content if it exists
        if let Ok(update_message) = std::fs::read_to_string("updateMessage.txt") {
            let trimmed = update_message.trim();
            if !trimmed.is_empty() {
                let description = if trimmed.len() > 500 {
                    format!("{}...", &trimmed[..500])
                } else {
                    trimmed.to_string()
                };
                text.push_str(&format!("\n\n**Update Notes:**\n{}", description));
            }
        }

        // Include enabled plugins
        let enabled_plugins: Vec<_> = plugins.iter().filter(|p| p.enabled).collect();
        if !enabled_plugins.is_empty() {
            let plugin_list: Vec<String> = enabled_plugins
                .iter()
                .map(|p| format!("/{} v{}", p.command.name, p.version))
                .collect();
            text.push_str(&format!("\n\n**Enabled Plugins:** {}", plugin_list.join(", ")));
        }

        // Include recent commits summary
        if !RECENT_COMMITS.is_empty() {
            let changes: String = RECENT_COMMITS
                .lines()
                .take(3)
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(2, '|').collect();
                    if parts.len() == 2 {
                        Some(format!("- {} ({})", parts[1], parts[0]))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !changes.is_empty() {
                text.push_str(&format!("\n\n**Recent Changes:**\n{}", changes));
            }
        }

        text
    }

    /// Sends the embed to the bot owner via DM with inline commit details
    async fn send_to_owner(&self, http: &Http, owner_id: u64, embed: CreateEmbed, notification_text: &str) -> anyhow::Result<()> {
        let user = UserId(owner_id);
        let dm = user.create_dm_channel(http).await?;
        dm.send_message(http, |m| m.set_embed(embed)).await?;
        info!("Sent startup notification to owner {} via DM", owner_id);

        // Store the notification in conversation history so bot can reference it later
        let user_id_str = owner_id.to_string();
        let channel_id_str = dm.id.0.to_string();
        if let Err(e) = self.database.store_message(
            &user_id_str,
            &channel_id_str,
            "assistant",
            notification_text,
            None,
        ).await {
            warn!("Failed to store startup notification in conversation history: {}", e);
        }

        // Get configurable commit count (default 5)
        let commit_count: usize = self
            .database
            .get_bot_setting("startup_dm_commit_count")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        // Send detailed commit info as follow-up message (DMs can't have threads)
        if commit_count > 0 {
            let commits = get_detailed_commits(commit_count).await;
            if !commits.is_empty() {
                let repo_url = get_github_repo_url().await;
                let commit_text = Self::format_commits_for_dm(&commits, repo_url.as_deref());
                if !commit_text.is_empty() {
                    dm.send_message(http, |m| m.content(&commit_text)).await?;
                    info!("Sent detailed commit info to owner {} via DM", owner_id);

                    // Store commit details in conversation history
                    if let Err(e) = self.database.store_message(
                        &user_id_str,
                        &channel_id_str,
                        "assistant",
                        &commit_text,
                        None,
                    ).await {
                        warn!("Failed to store commit details in conversation history: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Formats commit info for inline display in DMs
    fn format_commits_for_dm(commits: &[CommitInfo], repo_url: Option<&str>) -> String {
        let mut result = String::from("**Recent Changes (Detailed):**\n");

        for commit in commits {
            let hash_display = if let Some(url) = repo_url {
                format!("[`{}`]({}/commit/{})", commit.hash, url, commit.hash)
            } else {
                format!("`{}`", commit.hash)
            };
            result.push_str(&format!("\n**{}** ({})\n", commit.subject, hash_display));

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
        &self,
        http: &Http,
        channel_id: u64,
        embed: CreateEmbed,
    ) -> anyhow::Result<()> {
        let channel = ChannelId(channel_id);
        let msg = channel.send_message(http, |m| m.set_embed(embed)).await?;
        info!("Sent startup notification to channel {}", channel_id);

        // Get configurable commit count (default 5)
        let commit_count: usize = self
            .database
            .get_bot_setting("startup_channel_commit_count")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        // Create a thread for detailed commit info
        if commit_count == 0 {
            return Ok(());
        }

        let commits = get_detailed_commits(commit_count).await;
        if !commits.is_empty() {
            let repo_url = get_github_repo_url().await;

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
                    Self::post_detailed_commits_to_thread(http, ChannelId(thread.id.0), &commits, repo_url.as_deref())
                        .await;
                }
                Err(e) => {
                    warn!("Failed to create thread for commit details: {}", e);
                    // Fall back to posting in the channel directly
                    Self::post_detailed_commits_to_channel(http, channel, &commits, repo_url.as_deref()).await;
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
        repo_url: Option<&str>,
    ) {
        for commit in commits {
            let msg = format_commit_for_thread(commit, repo_url);
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
        repo_url: Option<&str>,
    ) {
        // Combine all commits into one message for channel fallback
        let mut combined = String::from("**Recent Changes (Detailed):**\n");
        for commit in commits.iter().take(3) {
            let hash_display = if let Some(url) = repo_url {
                format!("[`{}`]({}/commit/{})", commit.hash, url, commit.hash)
            } else {
                format!("`{}`", commit.hash)
            };
            combined.push_str(&format!("\n**{}** ({})", commit.subject, hash_display));
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

        let formatted = format_commit_for_thread(&commit, None);
        assert!(formatted.contains("feat: add new feature"));
        assert!(formatted.contains("abc1234"));
        assert!(formatted.contains("This is the body"));
        assert!(formatted.contains("src/main.rs"));
        assert!(formatted.contains("src/lib.rs"));
    }

    #[test]
    fn test_format_commit_for_thread_with_link() {
        let commit = CommitInfo {
            hash: "abc1234".to_string(),
            subject: "feat: add new feature".to_string(),
            body: String::new(),
            files: vec![],
        };

        let formatted = format_commit_for_thread(&commit, Some("https://github.com/user/repo"));
        assert!(formatted.contains("feat: add new feature"));
        assert!(formatted.contains("[`abc1234`](https://github.com/user/repo/commit/abc1234)"));
    }

    #[test]
    fn test_format_commits_for_dm() {
        let commits = vec![CommitInfo {
            hash: "abc1234".to_string(),
            subject: "feat: add new feature".to_string(),
            body: String::new(),
            files: vec!["src/main.rs".to_string()],
        }];

        let formatted = StartupNotifier::format_commits_for_dm(&commits, None);
        assert!(formatted.contains("Recent Changes"));
        assert!(formatted.contains("feat: add new feature"));
        assert!(formatted.contains("abc1234"));
        assert!(formatted.contains("src/main.rs"));
    }

    #[test]
    fn test_format_commits_for_dm_with_link() {
        let commits = vec![CommitInfo {
            hash: "abc1234".to_string(),
            subject: "feat: add new feature".to_string(),
            body: String::new(),
            files: vec![],
        }];

        let formatted = StartupNotifier::format_commits_for_dm(&commits, Some("https://github.com/user/repo"));
        assert!(formatted.contains("[`abc1234`](https://github.com/user/repo/commit/abc1234)"));
    }

    #[test]
    fn test_parse_github_url_ssh() {
        let url = "git@github.com:user/repo.git";
        let result = parse_github_url(url);
        assert_eq!(result, Some("https://github.com/user/repo".to_string()));
    }

    #[test]
    fn test_parse_github_url_https() {
        let url = "https://github.com/user/repo.git";
        let result = parse_github_url(url);
        assert_eq!(result, Some("https://github.com/user/repo".to_string()));
    }

    #[test]
    fn test_parse_github_url_https_no_git_suffix() {
        let url = "https://github.com/user/repo";
        let result = parse_github_url(url);
        assert_eq!(result, Some("https://github.com/user/repo".to_string()));
    }

    #[test]
    fn test_parse_github_url_invalid() {
        let url = "https://gitlab.com/user/repo.git";
        let result = parse_github_url(url);
        assert_eq!(result, None);
    }
}
