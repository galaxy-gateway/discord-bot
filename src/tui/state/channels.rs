//! # Channel State
//!
//! State management for watched channels and message buffers.

use crate::ipc::DisplayMessage;
use std::collections::{HashMap, VecDeque};

/// Maximum messages to keep per channel
const MAX_MESSAGES_PER_CHANNEL: usize = 200;

/// Channel state management
pub struct ChannelState {
    /// Set of watched channel IDs
    watched: std::collections::HashSet<u64>,
    /// Message buffers per channel
    messages: HashMap<u64, VecDeque<DisplayMessage>>,
    /// Currently selected channel
    selected_channel: Option<u64>,
    /// Scroll position in message list
    scroll_offset: usize,
}

impl ChannelState {
    pub fn new() -> Self {
        ChannelState {
            watched: std::collections::HashSet::new(),
            messages: HashMap::new(),
            selected_channel: None,
            scroll_offset: 0,
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
}

impl Default for ChannelState {
    fn default() -> Self {
        Self::new()
    }
}
