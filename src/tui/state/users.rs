//! # Users State
//!
//! State management for user analytics screen.

use crate::ipc::{UserSummary, UserStats, DmSessionInfo};
use std::time::Instant;

/// User analytics state
pub struct UsersState {
    /// List of users with summary stats
    pub users: Vec<UserSummary>,
    /// Currently selected user index in list
    pub selected_index: usize,
    /// Selected user's detailed stats
    pub selected_user_stats: Option<UserStats>,
    /// Selected user's DM sessions
    pub selected_user_sessions: Vec<DmSessionInfo>,
    /// Whether viewing user details (vs list)
    pub viewing_details: bool,
    /// Last refresh time
    pub last_refresh: Option<Instant>,
    /// Whether a refresh is in progress
    pub refreshing: bool,
}

impl UsersState {
    pub fn new() -> Self {
        UsersState {
            users: Vec::new(),
            selected_index: 0,
            selected_user_stats: None,
            selected_user_sessions: Vec::new(),
            viewing_details: false,
            last_refresh: None,
            refreshing: false,
        }
    }

    /// Set the user list
    pub fn set_users(&mut self, users: Vec<UserSummary>) {
        self.users = users;
        self.last_refresh = Some(Instant::now());
        self.refreshing = false;
    }

    /// Set user details for selected user
    pub fn set_user_details(&mut self, user_id: String, stats: UserStats, dm_sessions: Vec<DmSessionInfo>) {
        if self.selected_user().map(|u| &u.user_id) == Some(&user_id) {
            self.selected_user_stats = Some(stats);
            self.selected_user_sessions = dm_sessions;
        }
    }

    /// Get currently selected user
    pub fn selected_user(&self) -> Option<&UserSummary> {
        self.users.get(self.selected_index)
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.clear_details();
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.selected_index < self.users.len().saturating_sub(1) {
            self.selected_index += 1;
            self.clear_details();
        }
    }

    /// Enter details view for selected user
    pub fn enter_details(&mut self) {
        if self.selected_user().is_some() {
            self.viewing_details = true;
        }
    }

    /// Exit details view
    pub fn exit_details(&mut self) {
        self.viewing_details = false;
    }

    /// Clear details (when selection changes)
    fn clear_details(&mut self) {
        self.selected_user_stats = None;
        self.selected_user_sessions.clear();
        self.viewing_details = false;
    }

    /// Check if needs refresh (every 60 seconds)
    pub fn needs_refresh(&self) -> bool {
        match self.last_refresh {
            None => true,
            Some(t) => t.elapsed().as_secs() >= 60,
        }
    }

    /// Start refresh
    pub fn start_refresh(&mut self) {
        self.refreshing = true;
    }
}

impl Default for UsersState {
    fn default() -> Self {
        Self::new()
    }
}
