//! # Help UI
//!
//! Keybindings and usage help.

use crate::tui::App;
use crate::tui::ui::titled_block;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

/// Render the help screen
pub fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    // Navigation help
    render_navigation_help(frame, chunks[0]);

    // Screen-specific help
    render_screen_help(frame, chunks[1]);
}

fn render_navigation_help(frame: &mut Frame, area: Rect) {
    let keybindings = vec![
        ("General", vec![
            ("q", "Quit application"),
            ("Ctrl+c", "Force quit"),
            ("?", "Show this help"),
            ("1-6", "Switch screens"),
            ("Esc", "Go back / Cancel"),
        ]),
        ("Navigation", vec![
            ("j / Down", "Move down"),
            ("k / Up", "Move up"),
            ("Enter / Space", "Select item"),
            ("g / Home", "Go to top"),
            ("G / End", "Go to bottom"),
            ("PgUp/PgDn", "Page scroll"),
        ]),
        ("Actions", vec![
            ("i", "Start text input"),
            ("t", "Toggle item"),
            ("r", "Refresh data"),
            ("d", "Delete item"),
        ]),
        ("Text Input", vec![
            ("Enter", "Submit input"),
            ("Esc", "Cancel input"),
            ("Backspace", "Delete character"),
        ]),
    ];

    let mut lines = vec![];

    for (section, bindings) in keybindings {
        lines.push(Line::from(vec![
            Span::styled(section, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        for (key, desc) in bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<15}", key), Style::default().fg(Color::Cyan)),
                Span::raw(desc),
            ]));
        }

        lines.push(Line::from(""));
    }

    let paragraph = Paragraph::new(lines)
        .block(titled_block("Keybindings"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, area);
}

fn render_screen_help(frame: &mut Frame, area: Rect) {
    let screens = vec![
        ("Dashboard [1]", vec![
            "View connection status",
            "Monitor system resources",
            "See API usage summary",
            "Watch activity feed",
            "",
            "Press 'r' to refresh stats",
        ]),
        ("Channel Watcher [2]", vec![
            "Watch Discord channels in real-time",
            "",
            "i: Add channel by ID",
            "j/k: Navigate list or scroll",
            "Enter: View selected channel",
            "d: Remove channel from list",
            "Esc: Go back to channel list",
        ]),
        ("Stats [3]", vec![
            "View usage statistics",
            "See cost breakdown by service",
            "Track daily spending",
            "CPU/Memory sparklines",
            "",
            "Press 't' to cycle time period",
            "Press 'r' to refresh data",
        ]),
        ("Users [4]", vec![
            "User analytics by cost",
            "View DM session history",
            "API usage per user",
            "",
            "Press Enter for user details",
            "Press 'r' to refresh",
        ]),
        ("Settings [5]", vec![
            "Toggle bot features on/off",
            "Manage personas",
            "Configure guild settings",
            "",
            "Press 't' to toggle feature",
            "Use Tab to switch sections",
        ]),
        ("Errors [6]", vec![
            "View recent error logs",
            "Error counts by type",
            "Stack trace viewer",
            "",
            "Press Enter for details",
            "Press 'r' to refresh",
        ]),
    ];

    let mut lines = vec![];

    for (screen, help) in screens {
        lines.push(Line::from(vec![
            Span::styled(screen, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));

        for line in help {
            if line.is_empty() {
                lines.push(Line::from(""));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(line.to_string()),
                ]));
            }
        }

        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Obi TUI v1.0.0", Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Part of the Obi Discord Bot", Style::default().fg(Color::DarkGray)),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(titled_block("Screen Guide"))
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, area);
}
