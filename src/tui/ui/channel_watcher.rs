//! # Channel Watcher UI
//!
//! Real-time channel message viewing and posting.

use crate::tui::App;
use crate::tui::app::InputMode;
use crate::tui::ui::{titled_block, truncate_text};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

/// Render the channel watcher screen
pub fn render_channels(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30),  // Channel list
            Constraint::Min(0),      // Messages
        ])
        .split(area);

    // Channel list
    render_channel_list(frame, app, chunks[0]);

    // Message area
    render_message_area(frame, app, chunks[1]);
}

fn render_channel_list(frame: &mut Frame, app: &App, area: Rect) {
    let watched = app.channel_state.watched_channels();
    let selected = app.channel_state.selected();

    let items: Vec<ListItem> = watched.iter().enumerate().map(|(i, channel_id)| {
        let msg_count = app.channel_state.message_count(*channel_id);
        let is_selected = selected == Some(*channel_id);

        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        // Try to find channel name from guilds
        let channel_name = app.guilds.iter()
            .flat_map(|g| &g.channels)
            .find(|c| c.id == *channel_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("{}", channel_id));

        let prefix = if is_selected { "> " } else { "  " };
        ListItem::new(format!("{}#{} ({})", prefix, channel_name, msg_count)).style(style)
    }).collect();

    let list = List::new(items)
        .block(titled_block("Watched Channels"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, area);

    // Show hint if no channels
    if watched.is_empty() {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from("No channels watched."),
            Line::from(""),
            Line::from("Press 'i' to enter a"),
            Line::from("channel ID to watch."),
        ])
        .block(Block::default())
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        let inner = area.inner(Margin { vertical: 3, horizontal: 2 });
        frame.render_widget(hint, inner);
    }
}

fn render_message_area(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),      // Messages
            Constraint::Length(3),   // Input
        ])
        .split(area);

    // Messages
    render_messages(frame, app, chunks[0]);

    // Input box
    render_input(frame, app, chunks[1]);
}

fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    let selected = app.channel_state.selected();

    let title = if let Some(channel_id) = selected {
        format!("Messages ({})", channel_id)
    } else {
        "Messages".to_string()
    };
    let block = titled_block(&title);

    if let Some(messages) = app.channel_state.get_selected_messages() {
        let items: Vec<ListItem> = messages.iter().map(|msg| {
            let timestamp = msg.timestamp.format("%H:%M:%S").to_string();
            let author_style = if msg.is_bot {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Yellow)
            };

            let line = Line::from(vec![
                Span::styled(format!("[{}] ", timestamp), Style::default().fg(Color::DarkGray)),
                Span::styled(&msg.author_name, author_style),
                Span::raw(": "),
                Span::raw(&msg.content),
            ]);

            ListItem::new(line)
        }).collect();

        let list = List::new(items)
            .block(block)
            .style(Style::default().fg(Color::White));

        frame.render_widget(list, area);
    } else {
        // No channel selected
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from("Select a channel from the list"),
            Line::from("or press 'i' to add one."),
        ])
        .block(block)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        frame.render_widget(hint, area);
    }
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let (title, style) = match app.input_mode {
        InputMode::Normal => ("Press 'i' to type", Style::default().fg(Color::DarkGray)),
        InputMode::Editing => ("Type message (Enter to send, Esc to cancel)", Style::default().fg(Color::Cyan)),
    };

    let input = Paragraph::new(app.input_buffer.as_str())
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(style);

    frame.render_widget(input, area);

    // Show cursor in edit mode
    if app.input_mode == InputMode::Editing {
        frame.set_cursor_position(Position::new(
            area.x + app.input_buffer.len() as u16 + 1,
            area.y + 1,
        ));
    }
}
