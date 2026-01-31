//! # Settings UI
//!
//! Feature toggles, persona management, and guild settings.

use crate::tui::App;
use crate::tui::app::InputMode;
use crate::tui::ui::titled_block;
use crate::features::FEATURES;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};

/// Settings tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Features,
    Personas,
    Guild,
}

impl SettingsTab {
    pub fn all() -> &'static [SettingsTab] {
        &[SettingsTab::Features, SettingsTab::Personas, SettingsTab::Guild]
    }

    pub fn title(&self) -> &'static str {
        match self {
            SettingsTab::Features => "Features",
            SettingsTab::Personas => "Personas",
            SettingsTab::Guild => "Guild Settings",
        }
    }
}

/// Render the settings screen
pub fn render_settings(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tab bar
            Constraint::Min(0),     // Content
        ])
        .split(area);

    // Settings tabs
    let titles: Vec<Line> = SettingsTab::all()
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" Settings "))
        .select(0) // TODO: Track selected tab
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow));

    frame.render_widget(tabs, chunks[0]);

    // Render features tab (default)
    render_features(frame, app, chunks[1]);
}

fn render_features(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(area);

    // Feature list
    let items: Vec<ListItem> = FEATURES.iter().map(|feature| {
        let toggleable = if feature.toggleable {
            Span::styled("[t] ", Style::default().fg(Color::Green))
        } else {
            Span::styled("    ", Style::default().fg(Color::DarkGray))
        };

        let status = Span::styled(
            " ON ",
            Style::default().bg(Color::Green).fg(Color::Black)
        );

        let name_style = if feature.toggleable {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        ListItem::new(Line::from(vec![
            toggleable,
            Span::styled(format!("{:<25}", feature.name), name_style),
            status,
            Span::raw(" "),
            Span::styled(format!("v{}", feature.version), Style::default().fg(Color::DarkGray)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(titled_block("Features"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        .highlight_symbol("> ");

    frame.render_widget(list, chunks[0]);

    // Feature details
    if let Some(feature) = FEATURES.get(app.selected_index) {
        let mut lines = vec![];

        lines.push(Line::from(vec![
            Span::styled(feature.name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::raw("Version: "),
            Span::styled(feature.version, Style::default().fg(Color::Cyan)),
        ]));

        lines.push(Line::from(vec![
            Span::raw("Since:   "),
            Span::styled(feature.since, Style::default().fg(Color::DarkGray)),
        ]));

        lines.push(Line::from(vec![
            Span::raw("Toggle:  "),
            if feature.toggleable {
                Span::styled("Yes", Style::default().fg(Color::Green))
            } else {
                Span::styled("No", Style::default().fg(Color::DarkGray))
            },
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Description:", Style::default().add_modifier(Modifier::BOLD))));
        lines.push(Line::from(feature.description));

        if feature.toggleable {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Press 't' to toggle on/off",
                Style::default().fg(Color::Green)
            )));
        }

        let paragraph = Paragraph::new(lines)
            .block(titled_block("Feature Details"))
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(paragraph, chunks[1]);
    } else {
        let paragraph = Paragraph::new("Select a feature to view details")
            .block(titled_block("Feature Details"))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(paragraph, chunks[1]);
    }
}

fn render_personas(frame: &mut Frame, app: &App, area: Rect) {
    let personas = vec![
        ("obi", "Obi-Wan Kenobi", "Wise mentor from Star Wars"),
        ("muppet", "Muppet Friend", "Enthusiastic and fun"),
        ("chef", "Cooking Expert", "Passionate about food"),
        ("teacher", "Patient Educator", "Clear explanations"),
        ("analyst", "Analyst", "Step-by-step analysis"),
        ("visionary", "Big Thinker", "Future-focused ideas"),
        ("noir", "Detective", "Hard-boiled mystery"),
        ("zen", "Sage", "Contemplative wisdom"),
        ("bard", "Storyteller", "Charismatic tales"),
        ("coach", "Coach", "Motivational support"),
        ("scientist", "Researcher", "Curious exploration"),
        ("gamer", "Gamer", "Friendly gaming talk"),
    ];

    let items: Vec<ListItem> = personas.iter().map(|(id, name, desc)| {
        ListItem::new(Line::from(vec![
            Span::styled(format!("{:<12}", id), Style::default().fg(Color::Yellow)),
            Span::styled(format!("{:<20}", name), Style::default().fg(Color::White)),
            Span::styled(*desc, Style::default().fg(Color::DarkGray)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(titled_block("Available Personas"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        .highlight_symbol("> ");

    frame.render_widget(list, area);
}

fn render_guild_settings(frame: &mut Frame, app: &App, area: Rect) {
    let settings = vec![
        ("default_verbosity", "Response length", "concise/normal/detailed"),
        ("default_persona", "Default persona", "obi/muppet/chef/..."),
        ("conflict_mediation", "Conflict detection", "enabled/disabled"),
        ("conflict_sensitivity", "Detection threshold", "low/medium/high/ultra"),
        ("mediation_cooldown", "Cooldown (minutes)", "1-60"),
        ("max_context_messages", "Context size", "10-60"),
        ("audio_transcription", "Audio processing", "enabled/disabled"),
        ("audio_transcription_mode", "When to transcribe", "always/mention_only"),
        ("mention_responses", "Respond to @mentions", "enabled/disabled"),
        ("response_embeds", "Use embed boxes", "enabled/disabled"),
    ];

    let items: Vec<ListItem> = settings.iter().map(|(key, name, values)| {
        ListItem::new(Line::from(vec![
            Span::styled(format!("{:<25}", name), Style::default().fg(Color::White)),
            Span::styled(format!("[{}]", values), Style::default().fg(Color::DarkGray)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(titled_block("Guild Settings"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))
        .highlight_symbol("> ");

    frame.render_widget(list, area);
}
