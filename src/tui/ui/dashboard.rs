//! # Dashboard UI
//!
//! Main dashboard showing connection status, system info, and activity feed.

use crate::tui::App;
use crate::tui::ui::{titled_block, format_bytes, format_currency};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Gauge};

/// Render the dashboard screen
pub fn render_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // Connection status
            Constraint::Length(10),  // System info
            Constraint::Min(0),      // Guild list
        ])
        .split(chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // API usage summary
            Constraint::Min(0),     // Activity feed
        ])
        .split(chunks[1]);

    // Connection status box
    render_connection_status(frame, app, left_chunks[0]);

    // System info box
    render_system_info(frame, app, left_chunks[1]);

    // Guild list
    render_guild_list(frame, app, left_chunks[2]);

    // API usage summary
    render_usage_summary(frame, app, right_chunks[0]);

    // Activity feed
    render_activity_feed(frame, app, right_chunks[1]);
}

fn render_connection_status(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![];

    // IPC status
    let ipc_status = if app.connected {
        Span::styled("Connected", Style::default().fg(Color::Green))
    } else {
        Span::styled("Disconnected", Style::default().fg(Color::Red))
    };
    lines.push(Line::from(vec![
        Span::raw("IPC:       "),
        ipc_status,
    ]));

    // Discord status
    let discord_status = if app.bot_connected {
        Span::styled("Connected", Style::default().fg(Color::Green))
    } else {
        Span::styled("Disconnected", Style::default().fg(Color::Red))
    };
    lines.push(Line::from(vec![
        Span::raw("Discord:   "),
        discord_status,
    ]));

    // Bot name
    if let Some(name) = &app.bot_username {
        lines.push(Line::from(vec![
            Span::raw("Bot:       "),
            Span::styled(name.clone(), Style::default().fg(Color::Cyan)),
        ]));
    }

    // Guild count
    lines.push(Line::from(vec![
        Span::raw("Guilds:    "),
        Span::styled(format!("{}", app.guilds.len()), Style::default().fg(Color::Yellow)),
    ]));

    // Active sessions
    lines.push(Line::from(vec![
        Span::raw("Sessions:  "),
        Span::styled(format!("{}", app.active_sessions), Style::default().fg(Color::Yellow)),
    ]));

    let block = titled_block("Connection Status");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_system_info(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats_cache;
    let mut lines = vec![];

    // Uptime
    lines.push(Line::from(vec![
        Span::raw("Uptime:    "),
        Span::styled(stats.format_uptime(), Style::default().fg(Color::Cyan)),
    ]));

    // CPU
    lines.push(Line::from(vec![
        Span::raw("CPU:       "),
        Span::styled(format!("{:.1}%", stats.system.cpu_percent), Style::default().fg(Color::Yellow)),
    ]));

    // Memory
    let mem_percent = stats.memory_percent();
    let mem_color = if mem_percent > 80.0 {
        Color::Red
    } else if mem_percent > 60.0 {
        Color::Yellow
    } else {
        Color::Green
    };
    lines.push(Line::from(vec![
        Span::raw("Memory:    "),
        Span::styled(
            format!("{} / {} ({:.1}%)",
                format_bytes(stats.system.memory_bytes),
                format_bytes(stats.system.memory_total),
                mem_percent
            ),
            Style::default().fg(mem_color)
        ),
    ]));

    // Database size
    lines.push(Line::from(vec![
        Span::raw("DB Size:   "),
        Span::styled(format_bytes(stats.system.db_size), Style::default().fg(Color::Cyan)),
    ]));

    // Last heartbeat
    if let Some(ts) = app.last_heartbeat {
        let ago = chrono::Utc::now().timestamp() - ts;
        lines.push(Line::from(vec![
            Span::raw("Heartbeat: "),
            Span::styled(format!("{}s ago", ago), Style::default().fg(Color::DarkGray)),
        ]));
    }

    let block = titled_block("System Info");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_guild_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app.guilds.iter().map(|g| {
        let channels = g.channels.len();
        let members = g.member_count.map(|c| format!(" ({} members)", c)).unwrap_or_default();
        ListItem::new(format!("{} - {} channels{}", g.name, channels, members))
    }).collect();

    let list = List::new(items)
        .block(titled_block("Guilds"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        .highlight_symbol("> ");

    frame.render_widget(list, area);
}

fn render_usage_summary(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats_cache.usage;
    let mut lines = vec![];

    lines.push(Line::from(vec![
        Span::raw("Today:     "),
        Span::styled(format_currency(stats.today_cost), Style::default().fg(Color::Green)),
    ]));

    lines.push(Line::from(vec![
        Span::raw("Total:     "),
        Span::styled(format_currency(stats.total_cost), Style::default().fg(Color::Yellow)),
    ]));

    lines.push(Line::from(vec![
        Span::raw("Tokens:    "),
        Span::styled(format!("{}", stats.total_tokens), Style::default().fg(Color::Cyan)),
    ]));

    lines.push(Line::from(vec![
        Span::raw("API Calls: "),
        Span::styled(format!("{}", stats.total_calls), Style::default().fg(Color::Cyan)),
    ]));

    let block = titled_block("API Usage");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_activity_feed(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app.activity_log.iter().rev().take(20).map(|entry| {
        ListItem::new(entry.clone())
    }).collect();

    let list = List::new(items)
        .block(titled_block("Activity Feed"))
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(list, area);
}
