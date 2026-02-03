//! # TUI Application Core
//!
//! Main application state and screen navigation.

use crate::ipc::{BotEvent, GuildInfo, ChannelHistorySummary};
use crate::tui::state::{ChannelState, StatsCache, UsersState, ErrorsState};
use crate::tui::ui::SettingsTab;
use std::collections::HashMap;

/// Available screens in the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Channels,
    Stats,
    Users,
    Settings,
    Errors,
    Help,
}

impl Screen {
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Channels => "Channel Watcher",
            Screen::Stats => "Usage Stats",
            Screen::Users => "User Analytics",
            Screen::Settings => "Settings",
            Screen::Errors => "Error Logs",
            Screen::Help => "Help",
        }
    }

    pub fn key(&self) -> char {
        match self {
            Screen::Dashboard => '1',
            Screen::Channels => '2',
            Screen::Stats => '3',
            Screen::Users => '4',
            Screen::Settings => '5',
            Screen::Errors => '6',
            Screen::Help => '?',
        }
    }

    pub fn all() -> &'static [Screen] {
        &[
            Screen::Dashboard,
            Screen::Channels,
            Screen::Stats,
            Screen::Users,
            Screen::Settings,
            Screen::Errors,
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

/// Purpose of the current input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputPurpose {
    /// Adding a channel ID to watch
    #[default]
    AddChannel,
    /// Sending a message to a channel
    SendMessage,
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
    /// Users state (user analytics)
    pub users_state: UsersState,
    /// Errors state (error logs)
    pub errors_state: ErrorsState,
    /// Current input mode
    pub input_mode: InputMode,
    /// Purpose of the current input
    pub input_purpose: InputPurpose,
    /// Input buffer for text entry
    pub input_buffer: String,
    /// Selected index for lists
    pub selected_index: usize,
    /// Current settings tab
    pub settings_tab: SettingsTab,
    /// Selection index for Features tab
    pub settings_feature_index: usize,
    /// Selection index for Personas tab
    pub settings_persona_index: usize,
    /// Selection index for Guild Settings tab
    pub settings_guild_index: usize,
    /// Feature enabled/disabled states (feature_id -> enabled)
    pub feature_states: HashMap<String, bool>,
    /// Error message to display
    pub error_message: Option<String>,
    /// Status message to display
    pub status_message: Option<String>,
    /// Last heartbeat timestamp
    pub last_heartbeat: Option<i64>,
    /// Activity log (recent events)
    pub activity_log: Vec<String>,
    /// Browse mode for channel selection
    pub browse_mode: bool,
    /// Selected guild index in browse mode
    pub browse_guild_index: usize,
    /// Selected channel index in browse mode
    pub browse_channel_index: usize,
    /// Whether channel pane is active in browse mode (false=guilds, true=channels)
    pub browse_channel_pane_active: bool,
    /// Channels with conversation history from database (channel_id -> summary)
    pub db_channel_history: HashMap<u64, ChannelHistorySummary>,
    /// Channels pending IPC watch subscription (auto-watched from DB history)
    pub pending_watches: Vec<u64>,
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
            users_state: UsersState::new(),
            errors_state: ErrorsState::new(),
            input_mode: InputMode::Normal,
            input_purpose: InputPurpose::default(),
            input_buffer: String::new(),
            selected_index: 0,
            settings_tab: SettingsTab::Features,
            settings_feature_index: 0,
            settings_persona_index: 0,
            settings_guild_index: 0,
            feature_states: HashMap::new(),
            error_message: None,
            status_message: None,
            last_heartbeat: None,
            activity_log: Vec::new(),
            browse_mode: false,
            browse_guild_index: 0,
            browse_channel_index: 0,
            browse_channel_pane_active: false,
            db_channel_history: HashMap::new(),
            pending_watches: Vec::new(),
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
            BotEvent::MessageCreate { channel_id, guild_id: _, message } => {
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
            BotEvent::StatusUpdate { connected, uptime_seconds, guild_count: _, active_sessions } => {
                self.bot_connected = connected;
                self.uptime_seconds = uptime_seconds;
                self.active_sessions = active_sessions;
            }
            BotEvent::CommandResponse { success, message, .. } => {
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
            BotEvent::ChannelInfoResponse {
                channel_id,
                name,
                guild_name,
                message_count,
                last_activity: _,
            } => {
                self.channel_state.set_channel_info(channel_id, name, guild_name, message_count);
            }
            BotEvent::ChannelHistoryResponse {
                channel_id,
                messages,
            } => {
                let msg_count = messages.len();
                self.channel_state.set_history(channel_id, messages);
                self.add_activity(format!("Loaded {} messages for channel", msg_count));
            }
            BotEvent::HistoricalMetricsResponse {
                metric_type,
                data_points,
            } => {
                self.stats_cache.set_historical_data(&metric_type, data_points);
            }
            BotEvent::UserListResponse { users } => {
                self.users_state.set_users(users);
            }
            BotEvent::UserDetailsResponse {
                user_id,
                stats,
                dm_sessions,
            } => {
                self.users_state.set_user_details(user_id, stats, dm_sessions);
            }
            BotEvent::RecentErrorsResponse { errors } => {
                self.errors_state.set_errors(errors);
            }
            BotEvent::FeatureStatesResponse { states, guild_id: _ } => {
                self.feature_states = states;
                self.add_activity(format!("Loaded {} feature states", self.feature_states.len()));
            }
            BotEvent::ChannelsWithHistoryResponse { channels } => {
                let count = channels.len();
                self.db_channel_history.clear();
                self.pending_watches.clear();

                for channel in channels {
                    let channel_id = channel.channel_id;
                    self.db_channel_history.insert(channel_id, channel);

                    // Auto-watch if not already watching
                    if !self.channel_state.is_watching(channel_id) {
                        self.channel_state.watch(channel_id);
                        self.pending_watches.push(channel_id);
                    }
                }
                self.add_activity(format!("Auto-watching {} channels with history", count));
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
        self.input_purpose = InputPurpose::AddChannel;
    }

    /// Enter editing mode for sending a message
    pub fn start_message_input(&mut self) {
        self.input_mode = InputMode::Editing;
        self.input_purpose = InputPurpose::SendMessage;
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

    /// Move to the previous settings tab
    pub fn settings_tab_left(&mut self) {
        self.settings_tab = match self.settings_tab {
            SettingsTab::Features => SettingsTab::Guild,  // Wrap around
            SettingsTab::Personas => SettingsTab::Features,
            SettingsTab::Guild => SettingsTab::Personas,
        };
    }

    /// Move to the next settings tab
    pub fn settings_tab_right(&mut self) {
        self.settings_tab = match self.settings_tab {
            SettingsTab::Features => SettingsTab::Personas,
            SettingsTab::Personas => SettingsTab::Guild,
            SettingsTab::Guild => SettingsTab::Features,  // Wrap around
        };
    }

    /// Get the current settings tab's selection index
    pub fn settings_current_index(&self) -> usize {
        match self.settings_tab {
            SettingsTab::Features => self.settings_feature_index,
            SettingsTab::Personas => self.settings_persona_index,
            SettingsTab::Guild => self.settings_guild_index,
        }
    }

    /// Get the mutable reference to current settings tab's selection index
    pub fn settings_current_index_mut(&mut self) -> &mut usize {
        match self.settings_tab {
            SettingsTab::Features => &mut self.settings_feature_index,
            SettingsTab::Personas => &mut self.settings_persona_index,
            SettingsTab::Guild => &mut self.settings_guild_index,
        }
    }

    /// Get the list length for the current settings tab
    pub fn settings_list_len(&self) -> usize {
        match self.settings_tab {
            SettingsTab::Features => crate::features::FEATURES.len(),
            SettingsTab::Personas => crate::features::PersonaManager::new().list_personas().len(),
            SettingsTab::Guild => 10,     // Number of settings in render_guild_settings()
        }
    }

    /// Check if a feature is enabled (defaults to true if not in state map)
    pub fn is_feature_enabled(&self, feature_id: &str) -> bool {
        self.feature_states.get(feature_id).copied().unwrap_or(true)
    }

    /// Toggle a feature state locally (call IPC to persist)
    pub fn toggle_feature_state(&mut self, feature_id: &str) -> bool {
        let current = self.is_feature_enabled(feature_id);
        let new_state = !current;
        self.feature_states.insert(feature_id.to_string(), new_state);
        new_state
    }

    /// Enter browse mode for channel selection
    pub fn start_browse_mode(&mut self) {
        self.browse_mode = true;
        self.browse_guild_index = 0;
        self.browse_channel_index = 0;
        self.browse_channel_pane_active = false;
    }

    /// Exit browse mode
    pub fn stop_browse_mode(&mut self) {
        self.browse_mode = false;
    }

    /// Get the currently selected guild in browse mode
    pub fn browse_selected_guild(&self) -> Option<&GuildInfo> {
        self.guilds.get(self.browse_guild_index)
    }

    /// Get available text channels for the selected guild in browse mode (Discord cache only)
    pub fn browse_available_channels(&self) -> Vec<&crate::ipc::ChannelInfo> {
        use crate::ipc::ChannelType;
        self.browse_selected_guild()
            .map(|g| {
                g.channels
                    .iter()
                    .filter(|c| matches!(c.channel_type, ChannelType::Text | ChannelType::News | ChannelType::Thread | ChannelType::Forum))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get merged channel count for browse mode (Discord cache + DB-only channels)
    pub fn browse_merged_channel_count(&self) -> usize {
        use std::collections::HashSet;

        let discord_channels = self.browse_available_channels();
        let discord_ids: HashSet<u64> = discord_channels.iter().map(|c| c.id).collect();
        let guild_id = self.browse_selected_guild().map(|g| g.id);

        let mut count = discord_channels.len();

        // Count DB-only channels for this guild
        if let Some(gid) = guild_id {
            for (channel_id, summary) in &self.db_channel_history {
                if summary.guild_id == Some(gid) && !discord_ids.contains(channel_id) {
                    count += 1;
                }
            }
        }

        count
    }

    /// Get the currently selected channel ID and name in browse mode (from merged list)
    /// Returns (channel_id, channel_name) if selected
    pub fn browse_selected_channel_info(&self) -> Option<(u64, String)> {
        use std::collections::HashSet;

        let discord_channels = self.browse_available_channels();
        let discord_ids: HashSet<u64> = discord_channels.iter().map(|c| c.id).collect();
        let guild_id = self.browse_selected_guild().map(|g| g.id)?;

        // Build merged list in same order as UI
        struct MergedEntry {
            id: u64,
            name: String,
            message_count: Option<u64>,
        }

        let mut merged: Vec<MergedEntry> = discord_channels.iter().map(|c| {
            let msg_count = self.db_channel_history.get(&c.id).map(|s| s.message_count);
            MergedEntry {
                id: c.id,
                name: c.name.clone(),
                message_count: msg_count,
            }
        }).collect();

        // Add DB-only channels
        for (channel_id, summary) in &self.db_channel_history {
            if summary.guild_id == Some(guild_id) && !discord_ids.contains(channel_id) {
                let name = summary.channel_name.clone().unwrap_or_else(|| format!("{}", channel_id));
                merged.push(MergedEntry {
                    id: *channel_id,
                    name,
                    message_count: Some(summary.message_count),
                });
            }
        }

        // Sort same as UI
        merged.sort_by(|a, b| {
            match (a.message_count, b.message_count) {
                (Some(ac), Some(bc)) => bc.cmp(&ac),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.name.cmp(&b.name),
            }
        });

        merged.get(self.browse_channel_index).map(|e| (e.id, e.name.clone()))
    }

    /// Get the currently selected channel in browse mode (legacy - Discord cache only)
    pub fn browse_selected_channel(&self) -> Option<&crate::ipc::ChannelInfo> {
        let channels = self.browse_available_channels();
        channels.get(self.browse_channel_index).copied()
    }

    /// Navigate up in browse mode (in current pane)
    pub fn browse_up(&mut self) {
        if self.browse_channel_pane_active {
            if self.browse_channel_index > 0 {
                self.browse_channel_index -= 1;
            }
        } else if self.browse_guild_index > 0 {
            self.browse_guild_index -= 1;
            self.browse_channel_index = 0; // Reset channel selection when guild changes
        }
    }

    /// Navigate down in browse mode (in current pane)
    pub fn browse_down(&mut self) {
        if self.browse_channel_pane_active {
            let max = self.browse_merged_channel_count();
            if self.browse_channel_index < max.saturating_sub(1) {
                self.browse_channel_index += 1;
            }
        } else {
            let max = self.guilds.len();
            if self.browse_guild_index < max.saturating_sub(1) {
                self.browse_guild_index += 1;
                self.browse_channel_index = 0; // Reset channel selection when guild changes
            }
        }
    }

    /// Switch to guild pane in browse mode
    pub fn browse_pane_left(&mut self) {
        self.browse_channel_pane_active = false;
    }

    /// Switch to channel pane in browse mode
    pub fn browse_pane_right(&mut self) {
        if !self.guilds.is_empty() {
            self.browse_channel_pane_active = true;
        }
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
