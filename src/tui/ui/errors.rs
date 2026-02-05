//! # Errors UI
//!
//! Error logs and diagnostics display.

use crate::tui::ui::titled_block;
use crate::tui::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

/// Render the errors screen
pub fn render_errors(frame: &mut Frame, app: &App, area: Rect) {
    if app.errors_state.viewing_details {
        render_error_details(frame, app, area);
    } else {
        render_error_list(frame, app, area);
    }
}

fn render_error_list(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(6), // Error summary
            Constraint::Min(0),    // Error list
        ])
        .split(area);

    // Header
    let header_text = format!(
        "Error Diagnostics - {} errors (press Enter for details, r to refresh)",
        app.errors_state.errors.len()
    );
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Red))
        .alignment(Alignment::Center);
    frame.render_widget(header, chunks[0]);

    // Error counts by type
    render_error_summary(frame, app, chunks[1]);

    // Error list
    if app.errors_state.errors.is_empty() {
        let empty_msg = if app.errors_state.refreshing {
            "Loading errors..."
        } else {
            "No errors recorded. That's great!"
        };
        let paragraph = Paragraph::new(empty_msg)
            .block(titled_block("Recent Errors"))
            .style(Style::default().fg(Color::Green))
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, chunks[2]);
        return;
    }

    let items: Vec<ListItem> = app
        .errors_state
        .errors
        .iter()
        .enumerate()
        .map(|(i, err)| {
            let is_selected = i == app.errors_state.selected_index;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_selected { "> " } else { "  " };
            let timestamp = err.timestamp.format("%m-%d %H:%M:%S").to_string();

            let type_style = match err.error_type.as_str() {
                "panic" | "critical" => {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                }
                "warning" => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::Magenta),
            };

            let line = Line::from(vec![
                Span::raw(prefix),
                Span::styled(
                    format!("[{}] ", timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:<12} ", &err.error_type), type_style),
                Span::raw(truncate(&err.error_message, 60)),
            ]);

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Recent Errors"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, chunks[2]);
}

fn render_error_summary(frame: &mut Frame, app: &App, area: Rect) {
    let counts = app.errors_state.error_counts();

    if counts.is_empty() {
        let paragraph = Paragraph::new("No errors by type")
            .block(titled_block("Error Summary"))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = counts
        .iter()
        .take(3)
        .map(|(error_type, count)| {
            let bar_width =
                (*count as f64 / app.errors_state.errors.len().max(1) as f64 * 20.0) as usize;
            let bar: String = "█".repeat(bar_width);

            let type_style = match error_type.as_str() {
                "panic" | "critical" => Style::default().fg(Color::Red),
                "warning" => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::Magenta),
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<15}", error_type), type_style),
                Span::styled(format!("{:>4} ", count), Style::default().fg(Color::White)),
                Span::styled(bar, Style::default().fg(Color::Red)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Error Summary"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, area);
}

fn render_error_details(frame: &mut Frame, app: &App, area: Rect) {
    let err = match app.errors_state.selected_error() {
        Some(e) => e,
        None => {
            let paragraph = Paragraph::new("No error selected")
                .block(titled_block("Error Details"))
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            frame.render_widget(paragraph, area);
            return;
        }
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Error info
            Constraint::Min(0),     // Stack trace
        ])
        .split(area);

    // Error info
    let mut lines = vec![];

    lines.push(Line::from(vec![
        Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &err.error_type,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Time: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            err.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            Style::default().fg(Color::White),
        ),
    ]));

    if let Some(user_id) = &err.user_id {
        lines.push(Line::from(vec![
            Span::styled("User: ", Style::default().fg(Color::DarkGray)),
            Span::styled(user_id, Style::default().fg(Color::Cyan)),
        ]));
    }

    if let Some(channel_id) = &err.channel_id {
        lines.push(Line::from(vec![
            Span::styled("Channel: ", Style::default().fg(Color::DarkGray)),
            Span::styled(channel_id, Style::default().fg(Color::Cyan)),
        ]));
    }

    if let Some(command) = &err.command {
        lines.push(Line::from(vec![
            Span::styled("Command: ", Style::default().fg(Color::DarkGray)),
            Span::styled(command, Style::default().fg(Color::Yellow)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Message:",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        &err.error_message,
        Style::default().fg(Color::White),
    )));

    let info = Paragraph::new(lines).block(titled_block("Error Info"));
    frame.render_widget(info, chunks[0]);

    // Stack trace
    let stack_content = err
        .stack_trace
        .as_deref()
        .unwrap_or("No stack trace available");

    let stack_lines: Vec<Line> = stack_content
        .lines()
        .skip(app.errors_state.details_scroll)
        .take(chunks[1].height as usize - 2)
        .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::DarkGray))))
        .collect();

    let stack = Paragraph::new(stack_lines)
        .block(titled_block("Stack Trace (j/k to scroll, Esc to go back)"))
        .wrap(Wrap { trim: false });
    frame.render_widget(stack, chunks[1]);
}

/// Truncate a string
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
