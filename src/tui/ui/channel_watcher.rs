//! # Channel Watcher UI
//!
//! Real-time channel message viewing and posting.
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.18.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Add guild names to watched channel list, add browse mode for guild/channel selection
//! - 1.0.0: Initial release with channel watching and message display

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

    // Check if we're in browse mode
    if app.browse_mode {
        render_browse_mode(frame, app, chunks[1]);
        return;
    }

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(36),  // Channel list (wider for guild names)
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
            if app.browse_mode {
                "j/k: Navigate | h/l: Switch pane | Enter: Watch | Esc: Cancel"
            } else if has_selection {
                "j/k: Scroll | m: Send message | b: Back | d: Unwatch | i: Add channel"
            } else if has_channels {
                "j/k: Navigate | Enter: View | b: Browse guilds | d: Remove | i: Add ID"
            } else {
                "Press 'b' to browse guilds or 'i' to enter a channel ID"
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

        // Try to find channel name and guild name from guilds
        let (channel_name, guild_name) = app.guilds.iter()
            .find_map(|g| {
                g.channels.iter()
                    .find(|c| c.id == *channel_id)
                    .map(|c| (c.name.clone(), Some(g.name.clone())))
            })
            .unwrap_or_else(|| (format!("{}", channel_id), None));

        let prefix = if is_viewing {
            "> "
        } else if is_cursor {
            "> "
        } else {
            "  "
        };

        // Format: #channel [Guild] (count) or #channel (count) if no guild
        let display = if let Some(guild) = guild_name {
            // Truncate guild name if too long
            let guild_short = if guild.len() > 12 {
                format!("{}...", &guild[..9])
            } else {
                guild
            };
            format!("{}#{} [{}] ({})", prefix, channel_name, guild_short, msg_count)
        } else {
            format!("{}#{} ({})", prefix, channel_name, msg_count)
        };

        let status = if is_viewing { " [viewing]" } else { "" };
        ListItem::new(format!("{}{}", display, status)).style(style)
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
            Line::from("Press 'b' to browse"),
            Line::from("available guilds."),
            Line::from(""),
            Line::from("Or press 'i' to enter"),
            Line::from("a channel ID directly."),
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

/// Render browse mode with two-pane guild/channel browser
fn render_browse_mode(frame: &mut Frame, app: &App, area: Rect) {
    use crate::ipc::ChannelType;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),  // Guild list
            Constraint::Percentage(60),  // Channel list
        ])
        .split(area);

    // Left pane: Guilds
    let guild_border_style = if !app.browse_channel_pane_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };

    let guild_items: Vec<ListItem> = app.guilds.iter().enumerate().map(|(i, guild)| {
        let is_selected = i == app.browse_guild_index;
        let channel_count = guild.channels.iter()
            .filter(|c| matches!(c.channel_type, ChannelType::Text | ChannelType::News | ChannelType::Thread | ChannelType::Forum))
            .count();

        let style = if is_selected && !app.browse_channel_pane_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_selected { "> " } else { "  " };
        ListItem::new(format!("{}{} ({} ch)", prefix, guild.name, channel_count)).style(style)
    }).collect();

    let guild_title = format!(" Guilds ({}) ", app.guilds.len());
    let guild_list = List::new(guild_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(guild_title)
            .border_style(guild_border_style));

    frame.render_widget(guild_list, chunks[0]);

    // Right pane: Channels
    let channel_border_style = if app.browse_channel_pane_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };

    let channels = app.browse_available_channels();
    let watched = app.channel_state.watched_channels();

    let channel_items: Vec<ListItem> = channels.iter().enumerate().map(|(i, channel)| {
        let is_selected = i == app.browse_channel_index && app.browse_channel_pane_active;
        let is_watched = watched.contains(&channel.id);

        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_watched {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        // Channel type icon
        let icon = match channel.channel_type {
            ChannelType::Text => "#",
            ChannelType::News => "!",
            ChannelType::Thread => "@",
            ChannelType::Forum => "F",
            _ => "#",
        };

        let prefix = if is_selected { "> " } else { "  " };
        let watched_marker = if is_watched { " [watching]" } else { "" };
        ListItem::new(format!("{}{}{}{}", prefix, icon, channel.name, watched_marker)).style(style)
    }).collect();

    let guild_name = app.browse_selected_guild()
        .map(|g| g.name.as_str())
        .unwrap_or("No guild");
    let channel_title = format!(" {} - Channels ({}) ", guild_name, channels.len());
    let channel_list = List::new(channel_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(channel_title)
            .border_style(channel_border_style));

    frame.render_widget(channel_list, chunks[1]);

    // Show hint if no guilds
    if app.guilds.is_empty() {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from("No guilds available."),
            Line::from(""),
            Line::from("The bot may not be connected"),
            Line::from("or has not joined any guilds."),
        ])
        .block(Block::default())
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        let inner = chunks[0].inner(Margin { vertical: 2, horizontal: 1 });
        frame.render_widget(hint, inner);
    }

    // Show hint if no channels in selected guild
    if channels.is_empty() && !app.guilds.is_empty() {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from("No text channels"),
            Line::from("in this guild."),
        ])
        .block(Block::default())
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

        let inner = chunks[1].inner(Margin { vertical: 2, horizontal: 1 });
        frame.render_widget(hint, inner);
    }
}
