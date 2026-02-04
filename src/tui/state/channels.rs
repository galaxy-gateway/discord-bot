//! # Channel State
//!
//! State management for watched channels and message buffers.
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.18.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Add guild_id to ChannelMetadata for hierarchical grouping
//! - 1.0.0: Initial release

use crate::ipc::DisplayMessage;
use std::collections::{HashMap, VecDeque};

/// Maximum messages to keep per channel
const MAX_MESSAGES_PER_CHANNEL: usize = 200;

/// Channel metadata
#[derive(Debug, Clone, Default)]
pub struct ChannelMetadata {
    pub guild_id: Option<u64>,
    pub name: String,
    pub guild_name: Option<String>,
    pub message_count: u64,
}

/// Channel state management
pub struct ChannelState {
    /// Set of watched channel IDs
    watched: std::collections::HashSet<u64>,
    /// Message buffers per channel
    messages: HashMap<u64, VecDeque<DisplayMessage>>,
    /// Channel metadata
    metadata: HashMap<u64, ChannelMetadata>,
    /// Currently selected channel
    selected_channel: Option<u64>,
    /// Scroll position in message list
    scroll_offset: usize,
    /// Whether we're fetching history
    fetching_history: bool,
}

impl ChannelState {
    pub fn new() -> Self {
        ChannelState {
            watched: std::collections::HashSet::new(),
            messages: HashMap::new(),
            metadata: HashMap::new(),
            selected_channel: None,
            scroll_offset: 0,
            fetching_history: false,
        }
    }

    /// Watch a channel
    pub fn watch(&mut self, channel_id: u64) {
        self.watched.insert(channel_id);
        self.messages.entry(channel_id).or_insert_with(VecDeque::new);
    }

    /// Stop watching a channel
    pub fn unwatch(&mut self, channel_id: u64) {
        self.watched.remove(&channel_id);
        self.messages.remove(&channel_id);
        self.metadata.remove(&channel_id);
        if self.selected_channel == Some(channel_id) {
            self.selected_channel = None;
        }
    }

    /// Check if a channel is being watched
    pub fn is_watching(&self, channel_id: u64) -> bool {
        self.watched.contains(&channel_id)
    }

    /// Get list of watched channel IDs
    pub fn watched_channels(&self) -> Vec<u64> {
        self.watched.iter().copied().collect()
    }

    /// Add a message to a channel
    pub fn add_message(&mut self, channel_id: u64, message: DisplayMessage) {
        if !self.watched.contains(&channel_id) {
            return;
        }

        let buffer = self.messages.entry(channel_id).or_insert_with(VecDeque::new);
        buffer.push_back(message);

        // Trim old messages
        while buffer.len() > MAX_MESSAGES_PER_CHANNEL {
            buffer.pop_front();
        }
    }

    /// Remove a message from a channel
    pub fn remove_message(&mut self, channel_id: u64, message_id: u64) {
        if let Some(buffer) = self.messages.get_mut(&channel_id) {
            buffer.retain(|m| m.id != message_id);
        }
    }

    /// Get messages for a channel
    pub fn get_messages(&self, channel_id: u64) -> Option<&VecDeque<DisplayMessage>> {
        self.messages.get(&channel_id)
    }

    /// Get messages for the selected channel
    pub fn get_selected_messages(&self) -> Option<&VecDeque<DisplayMessage>> {
        self.selected_channel.and_then(|id| self.messages.get(&id))
    }

    /// Select a channel
    pub fn select(&mut self, channel_id: u64) {
        self.selected_channel = Some(channel_id);
        self.scroll_offset = 0;
    }

    /// Get selected channel ID
    pub fn selected(&self) -> Option<u64> {
        self.selected_channel
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.selected_channel = None;
    }

    /// Get scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Scroll up
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll down
    pub fn scroll_down(&mut self, amount: usize, max: usize) {
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        if let Some(messages) = self.get_selected_messages() {
            self.scroll_offset = messages.len().saturating_sub(1);
        }
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get message count for a channel
    pub fn message_count(&self, channel_id: u64) -> usize {
        self.messages.get(&channel_id).map(|m| m.len()).unwrap_or(0)
    }

    /// Set channel metadata (from ChannelInfoResponse)
    pub fn set_channel_info(&mut self, channel_id: u64, name: String, guild_id: Option<u64>, guild_name: Option<String>, message_count: u64) {
        self.metadata.insert(channel_id, ChannelMetadata {
            guild_id,
            name,
            guild_name,
            message_count,
        });
    }

    /// Get channel metadata
    pub fn get_metadata(&self, channel_id: u64) -> Option<&ChannelMetadata> {
        self.metadata.get(&channel_id)
    }

    /// Get selected channel metadata
    pub fn get_selected_metadata(&self) -> Option<&ChannelMetadata> {
        self.selected_channel.and_then(|id| self.metadata.get(&id))
    }

    /// Set history messages (from ChannelHistoryResponse)
    pub fn set_history(&mut self, channel_id: u64, messages: Vec<DisplayMessage>) {
        let buffer = self.messages.entry(channel_id).or_insert_with(VecDeque::new);

        // Prepend history messages (they come first chronologically)
        for msg in messages.into_iter().rev() {
            buffer.push_front(msg);
        }

        // Trim to max
        while buffer.len() > MAX_MESSAGES_PER_CHANNEL {
            buffer.pop_front();
        }

        self.fetching_history = false;
    }

    /// Check if fetching history
    pub fn is_fetching_history(&self) -> bool {
        self.fetching_history
    }

    /// Mark as fetching history
    pub fn start_fetching_history(&mut self) {
        self.fetching_history = true;
    }

    /// Check if channel needs history fetch (empty buffer)
    pub fn needs_history(&self, channel_id: u64) -> bool {
        self.messages.get(&channel_id).map(|m| m.is_empty()).unwrap_or(true)
    }
}

impl Default for ChannelState {
    fn default() -> Self {
        Self::new()
    }
}
