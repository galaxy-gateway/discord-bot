//! # Errors State
//!
//! State management for error logs screen.

use crate::ipc::ErrorInfo;
use std::time::Instant;

/// Error diagnostics state
pub struct ErrorsState {
    /// List of recent errors
    pub errors: Vec<ErrorInfo>,
    /// Currently selected error index
    pub selected_index: usize,
    /// Whether viewing error details (stack trace)
    pub viewing_details: bool,
    /// Scroll offset in details view
    pub details_scroll: usize,
    /// Last refresh time
    pub last_refresh: Option<Instant>,
    /// Whether a refresh is in progress
    pub refreshing: bool,
}

impl ErrorsState {
    pub fn new() -> Self {
        ErrorsState {
            errors: Vec::new(),
            selected_index: 0,
            viewing_details: false,
            details_scroll: 0,
            last_refresh: None,
            refreshing: false,
        }
    }

    /// Set the error list
    pub fn set_errors(&mut self, errors: Vec<ErrorInfo>) {
        self.errors = errors;
        self.last_refresh = Some(Instant::now());
        self.refreshing = false;
    }

    /// Get currently selected error
    pub fn selected_error(&self) -> Option<&ErrorInfo> {
        self.errors.get(self.selected_index)
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.viewing_details {
            if self.details_scroll > 0 {
                self.details_scroll -= 1;
            }
        } else if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.viewing_details {
            self.details_scroll += 1;
        } else if self.selected_index < self.errors.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Enter details view for selected error
    pub fn enter_details(&mut self) {
        if self.selected_error().is_some() {
            self.viewing_details = true;
            self.details_scroll = 0;
        }
    }

    /// Exit details view
    pub fn exit_details(&mut self) {
        self.viewing_details = false;
        self.details_scroll = 0;
    }

    /// Check if needs refresh (every 30 seconds)
    pub fn needs_refresh(&self) -> bool {
        match self.last_refresh {
            None => true,
            Some(t) => t.elapsed().as_secs() >= 30,
        }
    }

    /// Start refresh
    pub fn start_refresh(&mut self) {
        self.refreshing = true;
    }

    /// Get error count by type
    pub fn error_counts(&self) -> Vec<(String, usize)> {
        use std::collections::HashMap;
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for err in &self.errors {
            *counts.entry(&err.error_type).or_insert(0) += 1;
        }
        let mut result: Vec<_> = counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }
}

impl Default for ErrorsState {
    fn default() -> Self {
        Self::new()
    }
}
