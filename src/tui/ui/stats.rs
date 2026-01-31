//! # Stats UI
//!
//! Usage statistics and cost breakdown display.

use crate::tui::App;
use crate::tui::ui::{titled_block, format_bytes, format_currency};
use ratatui::prelude::*;
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph, Sparkline};

/// Render the stats screen
pub fn render_stats(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Time period selector
            Constraint::Min(0),      // Main content
        ])
        .split(area);

    // Time period selector
    render_time_selector(frame, app, chunks[0]);

    // Main content
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(chunks[1]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),  // Cost summary
            Constraint::Min(0),      // Cost by service
        ])
        .split(main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),  // Daily chart
            Constraint::Min(0),      // Top users
        ])
        .split(main_chunks[1]);

    render_cost_summary(frame, app, left_chunks[0]);
    render_cost_by_service(frame, app, left_chunks[1]);
    render_daily_chart(frame, app, right_chunks[0]);
    render_top_users(frame, app, right_chunks[1]);
}

fn render_time_selector(frame: &mut Frame, app: &App, area: Rect) {
    let period = app.stats_cache.time_period;
    let text = format!(
        "Time Period: {} (press 't' to cycle)",
        period.label()
    );

    let refreshing = if app.stats_cache.refreshing {
        " [Refreshing...]"
    } else {
        ""
    };

    let paragraph = Paragraph::new(format!("{}{}", text, refreshing))
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, area);
}

fn render_cost_summary(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats_cache.usage;
    let mut lines = vec![];

    lines.push(Line::from(vec![
        Span::raw("Period Cost:   "),
        Span::styled(format_currency(stats.today_cost), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
    ]));

    lines.push(Line::from(vec![
        Span::raw("All-time Cost: "),
        Span::styled(format_currency(stats.total_cost), Style::default().fg(Color::Yellow)),
    ]));

    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::raw("Total Tokens:  "),
        Span::styled(format!("{:>12}", stats.total_tokens), Style::default().fg(Color::Cyan)),
    ]));

    lines.push(Line::from(vec![
        Span::raw("Total Calls:   "),
        Span::styled(format!("{:>12}", stats.total_calls), Style::default().fg(Color::Cyan)),
    ]));

    let avg_tokens = if stats.total_calls > 0 {
        stats.total_tokens / stats.total_calls
    } else {
        0
    };
    lines.push(Line::from(vec![
        Span::raw("Avg Tokens:    "),
        Span::styled(format!("{:>12}", avg_tokens), Style::default().fg(Color::DarkGray)),
    ]));

    let block = titled_block("Cost Summary");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_cost_by_service(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats_cache.usage;

    let items: Vec<ListItem> = stats.cost_by_service.iter().map(|(service, cost)| {
        let bar_width = ((cost / stats.total_cost.max(0.0001)) * 20.0) as usize;
        let bar: String = "█".repeat(bar_width);

        ListItem::new(Line::from(vec![
            Span::styled(format!("{:<12}", service), Style::default().fg(Color::White)),
            Span::styled(format!("{:>10} ", format_currency(*cost)), Style::default().fg(Color::Green)),
            Span::styled(bar, Style::default().fg(Color::Blue)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(titled_block("Cost by Service"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, area);
}

fn render_daily_chart(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats_cache.usage;

    if stats.daily_breakdown.is_empty() {
        let paragraph = Paragraph::new("No daily data available")
            .block(titled_block("Daily Breakdown"))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(paragraph, area);
        return;
    }

    // Convert to sparkline data (multiply by 10000 for visibility)
    let data: Vec<u64> = stats.daily_breakdown.iter()
        .map(|(_, cost)| (cost * 10000.0) as u64)
        .collect();

    let sparkline = Sparkline::default()
        .block(titled_block("Daily Breakdown (7 days)"))
        .data(&data)
        .style(Style::default().fg(Color::Green));

    frame.render_widget(sparkline, area);

    // Show dates below
    let dates: String = stats.daily_breakdown.iter()
        .map(|(date, _)| {
            // Extract just the day
            date.split('-').last().unwrap_or("?").to_string()
        })
        .collect::<Vec<_>>()
        .join(" ");

    let inner = area.inner(Margin { vertical: 0, horizontal: 1 });
    let date_area = Rect::new(inner.x, inner.y + inner.height - 2, inner.width, 1);
    let date_text = Paragraph::new(dates)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    frame.render_widget(date_text, date_area);
}

fn render_top_users(frame: &mut Frame, app: &App, area: Rect) {
    let stats = &app.stats_cache.usage;

    let items: Vec<ListItem> = stats.top_users.iter().enumerate().map(|(i, (user, cost))| {
        let rank_style = match i {
            0 => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            1 => Style::default().fg(Color::White),
            2 => Style::default().fg(Color::Rgb(205, 127, 50)), // Bronze
            _ => Style::default().fg(Color::DarkGray),
        };

        ListItem::new(Line::from(vec![
            Span::styled(format!("{}. ", i + 1), rank_style),
            Span::styled(format!("{:<20}", user), Style::default().fg(Color::White)),
            Span::styled(format_currency(*cost), Style::default().fg(Color::Green)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(titled_block("Top Users by Cost"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(list, area);
}
