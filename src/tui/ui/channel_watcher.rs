//! # Channel Watcher UI
//!
//! Real-time channel message viewing and posting.

use crate::tui::App;
use crate::tui::app::InputMode;
use crate::tui::ui::titled_block;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

/// Render the channel watcher screen
pub fn render_channels(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Help bar
            Constraint::Min(0),      // Main content
        ])
        .split(area);

    // Help bar at top
    render_help_bar(frame, app, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(32),  // Channel list
            Constraint::Min(0),      // Messages
        ])
        .split(chunks[1]);

    // Channel list
    render_channel_list(frame, app, main_chunks[0]);

    // Message area
    render_message_area(frame, app, main_chunks[1]);
}

fn render_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let has_channels = !app.channel_state.watched_channels().is_empty();
    let has_selection = app.channel_state.selected().is_some();

    let help_text = match app.input_mode {
        InputMode::Editing => {
            if has_selection {
                "Type message | Enter: Send | Esc: Cancel"
            } else {
                "Type channel ID | Enter: Watch | Esc: Cancel"
            }
        }
        InputMode::Normal => {
            if has_selection {
                "j/k: Scroll | m: Send message | Esc: Back | d: Unwatch | i: Add channel"
            } else if has_channels {
                "j/k: Navigate | Enter: View channel | d: Remove | i: Add channel"
            } else {
                "Press 'i' to add a channel ID to watch"
            }
        }
    };

    let paragraph = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title(" Controls "))
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, area);
}

fn render_channel_list(frame: &mut Frame, app: &App, area: Rect) {
    let watched = app.channel_state.watched_channels();
    let viewing_channel = app.channel_state.selected();

    let items: Vec<ListItem> = watched.iter().enumerate().map(|(i, channel_id)| {
        let msg_count = app.channel_state.message_count(*channel_id);
        let is_viewing = viewing_channel == Some(*channel_id);
        let is_cursor = i == app.selected_index && viewing_channel.is_none();

        let style = if is_viewing {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if is_cursor {
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

        let prefix = if is_viewing {
            "> "
        } else if is_cursor {
            "> "
        } else {
            "  "
        };

        let status = if is_viewing { " [viewing]" } else { "" };
        ListItem::new(format!("{}#{} ({}){}", prefix, channel_name, msg_count, status)).style(style)
    }).collect();

    let title = format!("Channels ({})", watched.len());
    let list = List::new(items)
        .block(titled_block(&title))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, area);

    // Show hint if no channels
    if watched.is_empty() {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from("No channels watched."),
            Line::from(""),
            Line::from("Press 'i' to add a"),
            Line::from("Discord channel ID."),
            Line::from(""),
            Line::from("Find channel IDs in"),
            Line::from("Discord's developer mode."),
        ])
        .block(Block::default())
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        let inner = area.inner(Margin { vertical: 2, horizontal: 1 });
        frame.render_widget(hint, inner);
    }
}

fn render_message_area(frame: &mut Frame, app: &App, area: Rect) {
    // Only show input box when in editing mode
    if app.input_mode == InputMode::Editing {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),      // Messages
                Constraint::Length(3),   // Input
            ])
            .split(area);

        render_messages(frame, app, chunks[0]);
        render_input(frame, app, chunks[1]);
    } else {
        render_messages(frame, app, area);
    }
}

fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    let selected = app.channel_state.selected();

    let title = if let Some(channel_id) = selected {
        // Try to get channel metadata
        if let Some(meta) = app.channel_state.get_selected_metadata() {
            let msg_count = app.channel_state.message_count(channel_id);
            if let Some(guild) = &meta.guild_name {
                format!("#{} in {} - {} messages", meta.name, guild, msg_count)
            } else {
                format!("#{} - {} messages", meta.name, msg_count)
            }
        } else {
            let msg_count = app.channel_state.message_count(channel_id);
            format!("Channel {} - {} messages", channel_id, msg_count)
        }
    } else {
        "Messages (select a channel)".to_string()
    };
    let block = titled_block(&title);

    if let Some(messages) = app.channel_state.get_selected_messages() {
        if messages.is_empty() {
            // Channel selected but no messages yet
            let hint_text = if app.channel_state.is_fetching_history() {
                "Loading messages..."
            } else {
                "No messages yet. New messages will appear here in real-time."
            };
            let hint = Paragraph::new(hint_text)
                .block(block)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            frame.render_widget(hint, area);
            return;
        }

        // Calculate visible range based on scroll offset and area height
        let visible_height = area.height.saturating_sub(2) as usize; // account for borders
        let scroll = app.channel_state.scroll_offset();
        let total = messages.len();

        let items: Vec<ListItem> = messages.iter()
            .skip(scroll)
            .take(visible_height)
            .map(|msg| {
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

        // Show scroll position in title if scrolled
        let final_title = if scroll > 0 || total > visible_height {
            format!("{} [{}-{}/{}]", title, scroll + 1, (scroll + visible_height).min(total), total)
        } else {
            title.clone()
        };

        let list = List::new(items)
            .block(titled_block(&final_title))
            .style(Style::default().fg(Color::White));

        frame.render_widget(list, area);
    } else {
        // No channel selected - show instructions
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No channel selected", Style::default().add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("Use j/k to navigate the channel list"),
            Line::from("Press Enter to view a channel"),
            Line::from(""),
            Line::from("Or press 'i' to add a new channel ID"),
        ])
        .block(block)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        frame.render_widget(hint, area);
    }
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    use crate::tui::app::InputPurpose;

    let (title, style) = match app.input_purpose {
        InputPurpose::SendMessage => {
            (" Message (Enter to send, Esc to cancel) ", Style::default().fg(Color::Cyan))
        }
        InputPurpose::AddChannel => {
            (" Channel ID (Enter to watch, Esc to cancel) ", Style::default().fg(Color::Yellow))
        }
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
