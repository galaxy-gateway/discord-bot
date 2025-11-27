//! # Feature: Startup Notification
//!
//! Sends rich embed notifications when bot comes online.
//! Supports DM to bot owner and/or specific guild channels.
//! Configuration is stored in the database and managed via /set_guild_setting.
//!
//! - **Version**: 1.1.0
//! - **Since**: 0.4.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.1.0: Moved configuration from env vars to database
//! - 1.0.0: Initial release with DM and channel support, rich embeds

use crate::database::Database;
use crate::features::{get_bot_version, get_features};
use log::{info, warn};
use serenity::builder::CreateEmbed;
use serenity::http::Http;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, UserId};
use serenity::utils::Color;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Git commits embedded at compile time by build.rs
const RECENT_COMMITS: &str = env!("GIT_RECENT_COMMITS");

/// Tracks whether this is the first Ready event (vs reconnect)
static FIRST_READY: AtomicBool = AtomicBool::new(true);

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
    pub async fn send_if_enabled(&self, http: &Http, ready: &Ready) {
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

        let embed = Self::build_embed(ready);

        // Send to owner DM
        if let Some(oid) = owner_id {
            if let Err(e) = Self::send_to_owner(http, oid, embed.clone()).await {
                warn!("Failed to send startup DM to owner {}: {}", oid, e);
            }
        }

        // Send to channel
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
    fn build_embed(ready: &Ready) -> CreateEmbed {
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

        // Feature versions (non-inline for more space)
        let feature_list: String = features
            .iter()
            .map(|f| format!("{} `v{}`", f.name, f.version))
            .collect::<Vec<_>>()
            .join("\n");
        embed.field("Features", feature_list, false);

        // Recent changes from git commits
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

    /// Sends the embed to the bot owner via DM
    async fn send_to_owner(http: &Http, owner_id: u64, embed: CreateEmbed) -> anyhow::Result<()> {
        let user = UserId(owner_id);
        let dm = user.create_dm_channel(http).await?;
        dm.send_message(http, |m| m.set_embed(embed)).await?;
        info!("Sent startup notification to owner {} via DM", owner_id);
        Ok(())
    }

    /// Sends the embed to a specific channel
    async fn send_to_channel(
        http: &Http,
        channel_id: u64,
        embed: CreateEmbed,
    ) -> anyhow::Result<()> {
        let channel = ChannelId(channel_id);
        channel.send_message(http, |m| m.set_embed(embed)).await?;
        info!("Sent startup notification to channel {}", channel_id);
        Ok(())
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
}
