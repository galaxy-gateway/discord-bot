//! # Users UI
//!
//! User analytics and DM session tracking.

use crate::tui::ui::{format_currency, titled_block};
use crate::tui::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

/// Render the users screen
pub fn render_users(frame: &mut Frame, app: &App, area: Rect) {
    if app.users_state.viewing_details {
        render_user_details(frame, app, area);
    } else {
        render_user_list(frame, app, area);
    }
}

fn render_user_list(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // User list
        ])
        .split(area);

    // Header
    let header_text = format!(
        "User Analytics - {} users (press Enter to view details, r to refresh)",
        app.users_state.users.len()
    );
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Center);
    frame.render_widget(header, chunks[0]);

    // User list as table
    if app.users_state.users.is_empty() {
        let empty_msg = if app.users_state.refreshing {
            "Loading users..."
        } else {
            "No user data available. Press 'r' to refresh."
        };
        let paragraph = Paragraph::new(empty_msg)
            .block(titled_block("Users"))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, chunks[1]);
        return;
    }

    let header_row = Row::new(vec![
        Cell::from(" "),
        Cell::from("User").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Cost").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Tokens").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Sessions").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Last Active").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = app
        .users_state
        .users
        .iter()
        .enumerate()
        .map(|(i, user)| {
            let is_selected = i == app.users_state.selected_index;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_selected { ">" } else { " " };
            let last_active = user
                .last_activity
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "-".to_string());

            // Display username if available, otherwise show truncated user_id
            let display_name = user
                .username
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_else(|| truncate_user_id(&user.user_id));

            Row::new(vec![
                Cell::from(prefix),
                Cell::from(display_name),
                Cell::from(format_currency(user.total_cost))
                    .style(Style::default().fg(Color::Green)),
                Cell::from(format_tokens(user.total_tokens)),
                Cell::from(user.session_count.to_string()),
                Cell::from(last_active),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(header_row.style(Style::default().fg(Color::Cyan)))
        .block(titled_block("Users by Cost"));

    frame.render_widget(table, chunks[1]);
}

fn render_user_details(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // Stats
            Constraint::Percentage(60), // DM Sessions
        ])
        .split(area);

    // Stats panel
    render_user_stats(frame, app, chunks[0]);

    // DM Sessions panel
    render_dm_sessions(frame, app, chunks[1]);
}

fn render_user_stats(frame: &mut Frame, app: &App, area: Rect) {
    let stats = match &app.users_state.selected_user_stats {
        Some(s) => s,
        None => {
            let loading = Paragraph::new("Loading user stats...")
                .block(titled_block("User Stats"))
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            frame.render_widget(loading, area);
            return;
        }
    };

    let mut lines = vec![];

    // Display username prominently if available
    if let Some(username) = &stats.username {
        lines.push(Line::from(vec![
            Span::styled("Username: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                username,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("User ID: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &stats.user_id,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "-- API Usage --",
        Style::default().fg(Color::Cyan),
    )));

    lines.push(Line::from(vec![
        Span::raw("Total Cost:     "),
        Span::styled(
            format_currency(stats.total_cost),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::raw("Total Tokens:   "),
        Span::styled(
            format_tokens(stats.total_tokens),
            Style::default().fg(Color::Yellow),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::raw("API Calls:      "),
        Span::styled(
            stats.total_api_calls.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "-- Call Breakdown --",
        Style::default().fg(Color::Cyan),
    )));

    lines.push(Line::from(vec![
        Span::raw("Chat:           "),
        Span::styled(
            stats.chat_calls.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::raw("Whisper:        "),
        Span::styled(
            stats.whisper_calls.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::raw("DALL-E:         "),
        Span::styled(
            stats.dalle_calls.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "-- Activity --",
        Style::default().fg(Color::Cyan),
    )));

    lines.push(Line::from(vec![
        Span::raw("Messages:       "),
        Span::styled(
            stats.message_count.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::raw("DM Sessions:    "),
        Span::styled(
            stats.dm_session_count.to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    if let Some(persona) = &stats.favorite_persona {
        lines.push(Line::from(vec![
            Span::raw("Fav Persona:    "),
            Span::styled(persona, Style::default().fg(Color::Magenta)),
        ]));
    }

    if let Some(first) = &stats.first_seen {
        lines.push(Line::from(vec![
            Span::raw("First Seen:     "),
            Span::styled(
                first.format("%Y-%m-%d").to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    if let Some(last) = &stats.last_activity {
        lines.push(Line::from(vec![
            Span::raw("Last Active:    "),
            Span::styled(
                last.format("%Y-%m-%d").to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press Esc to go back",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).block(titled_block("User Stats"));
    frame.render_widget(paragraph, area);
}

fn render_dm_sessions(frame: &mut Frame, app: &App, area: Rect) {
    let sessions = &app.users_state.selected_user_sessions;

    if sessions.is_empty() {
        let empty = Paragraph::new("No DM sessions recorded")
            .block(titled_block("DM Sessions"))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty, area);
        return;
    }

    let header_row = Row::new(vec![
        Cell::from("Started").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Duration").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Msgs").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Cost").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Tokens").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = sessions
        .iter()
        .map(|session| {
            let started = session.started_at.format("%m-%d %H:%M").to_string();
            let duration = match session.ended_at {
                Some(end) => {
                    let dur = end - session.started_at;
                    format!("{}m", dur.num_minutes())
                }
                None => "ongoing".to_string(),
            };

            Row::new(vec![
                Cell::from(started),
                Cell::from(duration),
                Cell::from(session.message_count.to_string()),
                Cell::from(format_currency(session.api_cost))
                    .style(Style::default().fg(Color::Green)),
                Cell::from(format_tokens(session.total_tokens)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths).header(header_row.style(Style::default().fg(Color::Cyan)));
    let dm_sessions_title = format!("DM Sessions ({})", sessions.len());
    let table = table.block(titled_block(&dm_sessions_title));

    frame.render_widget(table, area);
}

/// Truncate user ID for display
fn truncate_user_id(id: &str) -> String {
    if id.len() > 18 {
        format!("{}...", &id[..15])
    } else {
        id.to_string()
    }
}

/// Format token count with K/M suffix
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
