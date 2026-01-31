//! # TUI UI Components
//!
//! Ratatui-based UI rendering for each screen.

mod dashboard;
mod channel_watcher;
mod stats;
mod users;
mod settings;
mod errors;
mod help;

pub use dashboard::render_dashboard;
pub use channel_watcher::render_channels;
pub use stats::render_stats;
pub use users::render_users;
pub use settings::render_settings;
pub use errors::render_errors;
pub use help::render_help;

use crate::tui::{App, Screen};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

/// Main render function - dispatches to screen-specific renderers
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tab bar
            Constraint::Min(0),     // Main content
            Constraint::Length(1),  // Status bar
        ])
        .split(frame.area());

    // Render tab bar
    render_tabs(frame, app, chunks[0]);

    // Render current screen
    match app.current_screen {
        Screen::Dashboard => render_dashboard(frame, app, chunks[1]),
        Screen::Channels => render_channels(frame, app, chunks[1]),
        Screen::Stats => render_stats(frame, app, chunks[1]),
        Screen::Users => render_users(frame, app, chunks[1]),
        Screen::Settings => render_settings(frame, app, chunks[1]),
        Screen::Errors => render_errors(frame, app, chunks[1]),
        Screen::Help => render_help(frame, app, chunks[1]),
    }

    // Render status bar
    render_status_bar(frame, app, chunks[2]);
}

/// Render the tab bar
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Screen::all()
        .iter()
        .map(|s| {
            let style = if *s == app.current_screen {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(format!("[{}] {}", s.key(), s.title())).style(style)
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" Obi TUI "))
        .select(Screen::all().iter().position(|s| *s == app.current_screen).unwrap_or(0))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow));

    frame.render_widget(tabs, area);
}

/// Render the status bar
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let connection_status = if app.connected {
        if app.bot_connected {
            Span::styled("● Connected", Style::default().fg(Color::Green))
        } else {
            Span::styled("● IPC Only", Style::default().fg(Color::Yellow))
        }
    } else {
        Span::styled("● Disconnected", Style::default().fg(Color::Red))
    };

    let mode_status = match app.input_mode {
        crate::tui::app::InputMode::Normal => Span::raw(""),
        crate::tui::app::InputMode::Editing => {
            Span::styled(" [EDITING] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        }
    };

    let help_hint = Span::styled(" q:Quit ?:Help ", Style::default().fg(Color::DarkGray));

    // Error or status message
    let message = if let Some(err) = &app.error_message {
        Span::styled(format!(" Error: {} ", err), Style::default().fg(Color::Red))
    } else if let Some(status) = &app.status_message {
        Span::styled(format!(" {} ", status), Style::default().fg(Color::Green))
    } else {
        Span::raw("")
    };

    let status_line = Line::from(vec![
        connection_status,
        Span::raw(" | "),
        mode_status,
        message,
        Span::raw(" "),
        help_hint,
    ]);

    let paragraph = Paragraph::new(status_line)
        .style(Style::default().bg(Color::DarkGray));

    frame.render_widget(paragraph, area);
}

/// Helper to create a block with title
pub fn titled_block(title: &str) -> Block {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
}

/// Helper to format bytes
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Helper to format currency
pub fn format_currency(amount: f64) -> String {
    format!("${:.4}", amount)
}

/// Helper to truncate text
pub fn truncate_text(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
