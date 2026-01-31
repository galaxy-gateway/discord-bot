//! # TUI Application Core
//!
//! Main application state and screen navigation.

use crate::ipc::{IpcClient, BotEvent, TuiCommand, GuildInfo, DisplayMessage};
use crate::tui::state::{ChannelState, StatsCache};
use anyhow::Result;
use std::collections::HashMap;

/// Available screens in the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Channels,
    Stats,
    Settings,
    Help,
}

impl Screen {
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Channels => "Channel Watcher",
            Screen::Stats => "Usage Stats",
            Screen::Settings => "Settings",
            Screen::Help => "Help",
        }
    }

    pub fn key(&self) -> char {
        match self {
            Screen::Dashboard => '1',
            Screen::Channels => '2',
            Screen::Stats => '3',
            Screen::Settings => '4',
            Screen::Help => '?',
        }
    }

    pub fn all() -> &'static [Screen] {
        &[
            Screen::Dashboard,
            Screen::Channels,
            Screen::Stats,
            Screen::Settings,
            Screen::Help,
        ]
    }
}

/// Input mode for text entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

/// Main application state
pub struct App {
    /// Current screen
    pub current_screen: Screen,
    /// Whether the app should quit
    pub should_quit: bool,
    /// IPC connection status
    pub connected: bool,
    /// Bot connection status (to Discord)
    pub bot_connected: bool,
    /// Known guilds
    pub guilds: Vec<GuildInfo>,
    /// Bot username
    pub bot_username: Option<String>,
    /// Bot user ID
    pub bot_user_id: Option<u64>,
    /// Uptime in seconds
    pub uptime_seconds: u64,
    /// Active session count
    pub active_sessions: usize,
    /// Channel state (watched channels, messages)
    pub channel_state: ChannelState,
    /// Stats cache
    pub stats_cache: StatsCache,
    /// Current input mode
    pub input_mode: InputMode,
    /// Input buffer for text entry
    pub input_buffer: String,
    /// Selected index for lists
    pub selected_index: usize,
    /// Error message to display
    pub error_message: Option<String>,
    /// Status message to display
    pub status_message: Option<String>,
    /// Last heartbeat timestamp
    pub last_heartbeat: Option<i64>,
    /// Activity log (recent events)
    pub activity_log: Vec<String>,
}

impl App {
    pub fn new() -> Self {
        App {
            current_screen: Screen::Dashboard,
            should_quit: false,
            connected: false,
            bot_connected: false,
            guilds: Vec::new(),
            bot_username: None,
            bot_user_id: None,
            uptime_seconds: 0,
            active_sessions: 0,
            channel_state: ChannelState::new(),
            stats_cache: StatsCache::new(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            selected_index: 0,
            error_message: None,
            status_message: None,
            last_heartbeat: None,
            activity_log: Vec::new(),
        }
    }

    /// Switch to a different screen
    pub fn switch_screen(&mut self, screen: Screen) {
        self.current_screen = screen;
        self.selected_index = 0;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    /// Handle an IPC event from the bot
    pub fn handle_bot_event(&mut self, event: BotEvent) {
        match event {
            BotEvent::Ready { guilds, bot_user_id, bot_username } => {
                self.bot_connected = true;
                self.guilds = guilds;
                self.bot_user_id = Some(bot_user_id);
                self.bot_username = Some(bot_username.clone());
                self.add_activity(format!("Bot connected: {}", bot_username));
            }
            BotEvent::Disconnected { reason } => {
                self.bot_connected = false;
                let msg = reason.unwrap_or_else(|| "Unknown reason".to_string());
                self.add_activity(format!("Bot disconnected: {}", msg));
            }
            BotEvent::MessageCreate { channel_id, guild_id, message } => {
                self.channel_state.add_message(channel_id, message.clone());
                self.add_activity(format!(
                    "#{}: {} - {}",
                    channel_id,
                    message.author_name,
                    truncate(&message.content, 50)
                ));
            }
            BotEvent::MessageDelete { channel_id, message_id } => {
                self.channel_state.remove_message(channel_id, message_id);
            }
            BotEvent::StatusUpdate { connected, uptime_seconds, guild_count, active_sessions } => {
                self.bot_connected = connected;
                self.uptime_seconds = uptime_seconds;
                self.active_sessions = active_sessions;
            }
            BotEvent::CommandResponse { request_id, success, message, .. } => {
                if success {
                    self.status_message = message;
                } else {
                    self.error_message = message;
                }
            }
            BotEvent::Heartbeat { timestamp } => {
                self.last_heartbeat = Some(timestamp);
            }
            BotEvent::PresenceUpdate { .. } => {
                // TODO: Track presence updates
            }
            BotEvent::UsageStatsUpdate {
                total_cost,
                period_cost,
                total_tokens,
                total_calls,
                cost_by_service,
                daily_breakdown,
                top_users,
                period_days: _,
            } => {
                self.stats_cache.usage.total_cost = total_cost;
                self.stats_cache.usage.today_cost = period_cost;
                self.stats_cache.usage.total_tokens = total_tokens;
                self.stats_cache.usage.total_calls = total_calls;
                self.stats_cache.usage.cost_by_service = cost_by_service;
                self.stats_cache.usage.daily_breakdown = daily_breakdown;
                self.stats_cache.usage.top_users = top_users;
                self.stats_cache.complete_refresh();
            }
            BotEvent::SystemMetricsUpdate {
                cpu_percent,
                memory_bytes,
                memory_total,
                db_size,
                uptime_seconds,
            } => {
                self.stats_cache.system.cpu_percent = cpu_percent;
                self.stats_cache.system.memory_bytes = memory_bytes;
                self.stats_cache.system.memory_total = memory_total;
                self.stats_cache.system.db_size = db_size;
                self.stats_cache.system.uptime_seconds = uptime_seconds;
            }
        }
    }

    /// Add an activity log entry
    pub fn add_activity(&mut self, msg: String) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.activity_log.push(format!("[{}] {}", timestamp, msg));

        // Keep only last 100 entries
        if self.activity_log.len() > 100 {
            self.activity_log.remove(0);
        }
    }

    /// Set connection status
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
        if !connected {
            self.bot_connected = false;
        }
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Clear status message
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Get the currently selected guild (if any)
    pub fn selected_guild(&self) -> Option<&GuildInfo> {
        self.guilds.get(self.selected_index)
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self, max: usize) {
        if self.selected_index < max.saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Enter editing mode
    pub fn start_editing(&mut self) {
        self.input_mode = InputMode::Editing;
    }

    /// Exit editing mode
    pub fn stop_editing(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    /// Add character to input buffer
    pub fn input_char(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    /// Remove last character from input buffer
    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    /// Clear input buffer
    pub fn input_clear(&mut self) {
        self.input_buffer.clear();
    }

    /// Get and clear input buffer
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input_buffer)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
