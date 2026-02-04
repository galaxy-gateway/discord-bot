//! # Channel Watcher UI
//!
//! Real-time channel message viewing and posting.
//!
//! - **Version**: 1.5.0
//! - **Since**: 3.18.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.5.0: Add word wrapping for long messages in message display
//! - 1.4.0: Collapsible guild sections with hierarchical channel display
//! - 1.3.0: Add scrollable channel list with selection kept in view
//! - 1.2.0: Show DB channels with message counts in browse mode, merged Discord+DB view
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
    use crate::tui::app::ChannelListSelection;

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
                // Show context-appropriate help based on current selection
                match app.resolve_channel_selection() {
                    ChannelListSelection::GuildHeader(_) => {
                        "j/k: Navigate | Enter: Toggle | h: Collapse | l: Expand | d: Remove | i: Add"
                    }
                    ChannelListSelection::Channel(_) => {
                        "j/k: Navigate | Enter: View | h: Collapse guild | d: Remove | i: Add"
                    }
                    ChannelListSelection::None => {
                        "Press 'b' to browse guilds or 'i' to enter a channel ID"
                    }
                }
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
    let viewing_channel = app.channel_state.selected();
    let groups = app.channels_by_guild();
    let visible_items = app.channel_list_visible_items();
    let total_items = visible_items.len();

    // Calculate visible height (account for borders)
    let visible_height = area.height.saturating_sub(2) as usize;
    let scroll = app.channel_list_scroll();

    // Build list items from visible items (respecting scroll)
    let items: Vec<ListItem> = visible_items.iter().enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, &(is_header, guild_id, channel_id))| {
            let is_cursor = i == app.selected_index && viewing_channel.is_none();

            if is_header {
                // Render guild header
                let is_collapsed = app.is_guild_collapsed(guild_id);
                let collapse_icon = if is_collapsed { "▶" } else { "▼" };

                // Find guild name and channel count
                let (guild_name, channel_count) = groups.iter()
                    .find(|(gid, _, _)| *gid == guild_id)
                    .map(|(_, name, channels)| (name.clone(), channels.len()))
                    .unwrap_or_else(|| ("Unknown".to_string(), 0));

                let style = if is_cursor {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                };

                let prefix = if is_cursor { ">" } else { " " };
                let display = format!("{}[{}] {} ({} ch)", prefix, collapse_icon, guild_name, channel_count);
                ListItem::new(display).style(style)
            } else {
                // Render channel
                let is_viewing = viewing_channel == Some(channel_id);

                // Get message count from current buffer
                let buffer_count = app.channel_state.message_count(channel_id);
                // Get DB history count if available
                let db_count = app.db_channel_history.get(&channel_id).map(|s| s.message_count as usize);
                let display_count = db_count.map(|d| d.max(buffer_count)).unwrap_or(buffer_count);

                // Get channel name - prefer db_channel_history first (authoritative)
                let channel_name = app.db_channel_history.get(&channel_id)
                    .and_then(|s| s.channel_name.clone())
                    .or_else(|| {
                        app.channel_state.get_metadata(channel_id).map(|m| m.name.clone())
                    })
                    .or_else(|| {
                        app.guilds.iter()
                            .find_map(|g| g.channels.iter().find(|c| c.id == channel_id).map(|c| c.name.clone()))
                    })
                    .unwrap_or_else(|| format!("{}", channel_id));

                let style = if is_viewing {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else if is_cursor {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let prefix = if is_viewing || is_cursor { " >" } else { "  " };
                let status = if is_viewing { " [viewing]" } else { "" };
                let display = format!("{}  #{} ({}){}", prefix, channel_name, display_count, status);
                ListItem::new(display).style(style)
            }
        }).collect();

    // Count total watched channels
    let total_channels: usize = groups.iter().map(|(_, _, channels)| channels.len()).sum();

    // Show scroll position in title if scrolled or list is larger than visible
    let title = if scroll > 0 || total_items > visible_height {
        format!("Channels [{}-{}/{}]", scroll + 1, (scroll + visible_height).min(total_items), total_channels)
    } else {
        format!("Channels ({})", total_channels)
    };

    let list = List::new(items)
        .block(titled_block(&title))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, area);

    // Show hint if no channels
    if total_channels == 0 {
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
        let content_width = area.width.saturating_sub(2) as usize; // account for borders
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

                // Calculate prefix length for wrapping: "[HH:MM:SS] author: "
                let prefix = format!("[{}] {}: ", timestamp, msg.author_name);
                let prefix_len = prefix.chars().count();

                // Wrap content to fit available width
                let wrap_width = content_width.saturating_sub(prefix_len).max(20);
                let wrapped_lines = wrap_text(&msg.content, wrap_width);

                if wrapped_lines.len() <= 1 {
                    // Single line - render normally
                    let content = wrapped_lines.into_iter().next().unwrap_or_default();
                    let line = Line::from(vec![
                        Span::styled(format!("[{}] ", timestamp), Style::default().fg(Color::DarkGray)),
                        Span::styled(&msg.author_name, author_style),
                        Span::raw(": "),
                        Span::raw(content),
                    ]);
                    ListItem::new(line)
                } else {
                    // Multiple lines - first line has header, continuation lines are indented
                    let mut lines = Vec::with_capacity(wrapped_lines.len());

                    // First line with timestamp and author
                    lines.push(Line::from(vec![
                        Span::styled(format!("[{}] ", timestamp), Style::default().fg(Color::DarkGray)),
                        Span::styled(&msg.author_name, author_style),
                        Span::raw(": "),
                        Span::raw(wrapped_lines[0].clone()),
                    ]));

                    // Continuation lines with indent
                    let indent = " ".repeat(prefix_len);
                    for wrapped_line in wrapped_lines.iter().skip(1) {
                        lines.push(Line::from(format!("{}{}", indent, wrapped_line)));
                    }

                    ListItem::new(lines)
                }
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
/// Shows merged view of Discord cache channels and DB channels with history
fn render_browse_mode(frame: &mut Frame, app: &App, area: Rect) {
    use crate::ipc::ChannelType;
    use std::collections::HashSet;

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

    // Count channels with history per guild
    let mut history_count_by_guild: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for summary in app.db_channel_history.values() {
        if let Some(gid) = summary.guild_id {
            *history_count_by_guild.entry(gid).or_default() += 1;
        }
    }

    let guild_items: Vec<ListItem> = app.guilds.iter().enumerate().map(|(i, guild)| {
        let is_selected = i == app.browse_guild_index;
        let channel_count = guild.channels.iter()
            .filter(|c| matches!(c.channel_type, ChannelType::Text | ChannelType::News | ChannelType::Thread | ChannelType::Forum))
            .count();

        let history_count = history_count_by_guild.get(&guild.id).copied().unwrap_or(0);

        let style = if is_selected && !app.browse_channel_pane_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_selected { "> " } else { "  " };
        // Show channel count and history count if different
        let display = if history_count > 0 {
            format!("{}{} ({} ch, {} w/hist)", prefix, guild.name, channel_count, history_count)
        } else {
            format!("{}{} ({} ch)", prefix, guild.name, channel_count)
        };
        ListItem::new(display).style(style)
    }).collect();

    let guild_title = format!(" Guilds ({}) ", app.guilds.len());
    let guild_list = List::new(guild_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(guild_title)
            .border_style(guild_border_style));

    frame.render_widget(guild_list, chunks[0]);

    // Right pane: Channels - merge Discord cache + DB history
    let channel_border_style = if app.browse_channel_pane_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };

    // Get Discord cache channels
    let discord_channels = app.browse_available_channels();
    let watched = app.channel_state.watched_channels();

    // Get the selected guild ID
    let selected_guild_id = app.browse_selected_guild().map(|g| g.id);

    // Collect channel IDs we've already shown from Discord cache
    let discord_channel_ids: HashSet<u64> = discord_channels.iter().map(|c| c.id).collect();

    // Build merged channel list with DB history info
    #[allow(dead_code)]
    struct MergedChannel {
        id: u64,
        name: String,
        channel_type_icon: &'static str,
        message_count: Option<u64>,
        is_watched: bool,
    }

    let mut merged_channels: Vec<MergedChannel> = discord_channels.iter().map(|channel| {
        let msg_count = app.db_channel_history.get(&channel.id).map(|s| s.message_count);
        let icon = match channel.channel_type {
            ChannelType::Text => "#",
            ChannelType::News => "!",
            ChannelType::Thread => "@",
            ChannelType::Forum => "F",
            _ => "#",
        };
        MergedChannel {
            id: channel.id,
            name: channel.name.clone(),
            channel_type_icon: icon,
            message_count: msg_count,
            is_watched: watched.contains(&channel.id),
        }
    }).collect();

    // Add DB-only channels (channels with history but not in Discord cache for this guild)
    if let Some(guild_id) = selected_guild_id {
        for (channel_id, summary) in &app.db_channel_history {
            if summary.guild_id == Some(guild_id) && !discord_channel_ids.contains(channel_id) {
                // Channel exists in DB but not in Discord cache (maybe deleted or hidden)
                let name = summary.channel_name.clone().unwrap_or_else(|| format!("{}", channel_id));
                merged_channels.push(MergedChannel {
                    id: *channel_id,
                    name,
                    channel_type_icon: "#",
                    message_count: Some(summary.message_count),
                    is_watched: watched.contains(channel_id),
                });
            }
        }
    }

    // Sort: channels with history first (by message count desc), then others alphabetically
    merged_channels.sort_by(|a, b| {
        match (a.message_count, b.message_count) {
            (Some(ac), Some(bc)) => bc.cmp(&ac), // Higher count first
            (Some(_), None) => std::cmp::Ordering::Less, // Has history comes first
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name), // Alphabetical
        }
    });

    let channel_items: Vec<ListItem> = merged_channels.iter().enumerate().map(|(i, channel)| {
        let is_selected = i == app.browse_channel_index && app.browse_channel_pane_active;

        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if channel.is_watched {
            Style::default().fg(Color::Green)
        } else if channel.message_count.is_some() {
            Style::default().fg(Color::Cyan) // Has history
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_selected { "> " } else { "  " };
        let watched_marker = if channel.is_watched { " [watching]" } else { "" };

        // Show message count for channels with history
        let count_str = match channel.message_count {
            Some(count) if count > 0 => format!(" [{} msgs]", count),
            Some(_) => String::new(), // 0 messages
            None => " [new]".to_string(),
        };

        ListItem::new(format!("{}{}{}{}{}",
            prefix,
            channel.channel_type_icon,
            channel.name,
            count_str,
            watched_marker
        )).style(style)
    }).collect();

    let guild_name = app.browse_selected_guild()
        .map(|g| g.name.as_str())
        .unwrap_or("No guild");
    let channel_title = format!(" {} - Channels ({}) ", guild_name, merged_channels.len());
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
    if merged_channels.is_empty() && !app.guilds.is_empty() {
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

/// Wrap text to fit within a given width, breaking on word boundaries
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();

        if current_width == 0 {
            // First word on the line
            if word_len > max_width {
                // Word is longer than max width, force break it
                let mut remaining = word;
                while !remaining.is_empty() {
                    let take: String = remaining.chars().take(max_width).collect();
                    let taken_len = take.chars().count();
                    lines.push(take);
                    remaining = &remaining[remaining.char_indices().nth(taken_len).map(|(i, _)| i).unwrap_or(remaining.len())..];
                }
            } else {
                current_line = word.to_string();
                current_width = word_len;
            }
        } else if current_width + 1 + word_len <= max_width {
            // Word fits on current line with space
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_len;
        } else {
            // Word doesn't fit, start new line
            lines.push(std::mem::take(&mut current_line));
            current_width = 0;

            if word_len > max_width {
                // Word is longer than max width, force break it
                let mut remaining = word;
                while !remaining.is_empty() {
                    let take: String = remaining.chars().take(max_width).collect();
                    let taken_len = take.chars().count();
                    if remaining.chars().count() > max_width {
                        lines.push(take);
                        remaining = &remaining[remaining.char_indices().nth(taken_len).map(|(i, _)| i).unwrap_or(remaining.len())..];
                    } else {
                        current_line = take;
                        current_width = taken_len;
                        break;
                    }
                }
            } else {
                current_line = word.to_string();
                current_width = word_len;
            }
        }
    }

    // Don't forget the last line
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // If text was empty or only whitespace, return single empty string
    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
