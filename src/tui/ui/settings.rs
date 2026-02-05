//! # Settings UI
//!
//! Feature toggles, persona management, and guild settings.
//!
//! - **Version**: 1.1.0
//! - **Since**: 3.20.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.1.0: Added tab switching, selection indicators, and persona prompt preview panel
//! - 1.0.0: Initial settings screen with features, personas, and guild settings tabs

use crate::features::{PersonaManager, FEATURES};
use crate::tui::ui::titled_block;
use crate::tui::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap};

/// Settings tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Features,
    Personas,
    Guild,
}

impl SettingsTab {
    pub fn all() -> &'static [SettingsTab] {
        &[
            SettingsTab::Features,
            SettingsTab::Personas,
            SettingsTab::Guild,
        ]
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
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
        ])
        .split(area);

    // Settings tabs
    let titles: Vec<Line> = SettingsTab::all()
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" Settings "))
        .select(app.settings_tab as usize)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, chunks[0]);

    // Render content based on selected tab
    match app.settings_tab {
        SettingsTab::Features => render_features(frame, app, chunks[1]),
        SettingsTab::Personas => render_personas(frame, app, chunks[1]),
        SettingsTab::Guild => render_guild_settings(frame, app, chunks[1]),
    }
}

fn render_features(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Feature list
    let items: Vec<ListItem> = FEATURES
        .iter()
        .map(|feature| {
            let toggleable = if feature.toggleable {
                Span::styled("[t] ", Style::default().fg(Color::Green))
            } else {
                Span::styled("    ", Style::default().fg(Color::DarkGray))
            };

            // Get actual feature state
            let enabled = if feature.toggleable {
                app.is_feature_enabled(feature.id)
            } else {
                true // Non-toggleable features are always on
            };

            let status = if enabled {
                Span::styled(" ON  ", Style::default().bg(Color::Green).fg(Color::Black))
            } else {
                Span::styled(" OFF ", Style::default().bg(Color::Red).fg(Color::White))
            };

            let name_style = if feature.toggleable {
                if enabled {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            } else {
                Style::default().fg(Color::DarkGray)
            };

            ListItem::new(Line::from(vec![
                toggleable,
                Span::styled(format!("{:<25}", feature.name), name_style),
                status,
                Span::raw(" "),
                Span::styled(
                    format!("v{}", feature.version),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Features"))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        )
        .highlight_symbol("> ");

    let mut list_state = ListState::default().with_selected(Some(app.settings_feature_index));
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    // Feature details
    if let Some(feature) = FEATURES.get(app.settings_feature_index) {
        let enabled = if feature.toggleable {
            app.is_feature_enabled(feature.id)
        } else {
            true
        };

        let mut lines = vec![];

        lines.push(Line::from(vec![Span::styled(
            feature.name,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
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

        // Show current state for toggleable features
        if feature.toggleable {
            lines.push(Line::from(vec![
                Span::raw("Status:  "),
                if enabled {
                    Span::styled(
                        "ENABLED",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(
                        "DISABLED",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )
                },
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Description:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(feature.description));

        if feature.toggleable {
            lines.push(Line::from(""));
            let toggle_hint = if enabled {
                "Press 't' to disable"
            } else {
                "Press 't' to enable"
            };
            lines.push(Line::from(Span::styled(
                toggle_hint,
                Style::default().fg(Color::Green),
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
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    // Get persona data from PersonaManager
    let manager = PersonaManager::new();
    let mut personas: Vec<_> = manager.list_personas();
    // Sort by persona ID for consistent ordering
    personas.sort_by(|a, b| a.0.cmp(b.0));

    // Persona list
    let items: Vec<ListItem> = personas
        .iter()
        .map(|(id, persona)| {
            // Convert color to RGB for display hint
            let color = Color::Rgb(
                ((persona.color >> 16) & 0xFF) as u8,
                ((persona.color >> 8) & 0xFF) as u8,
                (persona.color & 0xFF) as u8,
            );
            ListItem::new(Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::styled(format!("{:<10}", id), Style::default().fg(Color::Yellow)),
                Span::styled(&persona.name, Style::default().fg(Color::White)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Personas"))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        )
        .highlight_symbol("> ");

    let mut list_state = ListState::default().with_selected(Some(app.settings_persona_index));
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    // Persona details with prompt preview
    if let Some((id, persona)) = personas.get(app.settings_persona_index) {
        let color = Color::Rgb(
            ((persona.color >> 16) & 0xFF) as u8,
            ((persona.color >> 8) & 0xFF) as u8,
            (persona.color & 0xFF) as u8,
        );

        let mut lines = vec![];

        // Header with name and color indicator
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(
                &persona.name,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ({})", id), Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));

        // Description
        lines.push(Line::from(Span::styled(
            "Description:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(persona.description.clone()));
        lines.push(Line::from(""));

        // System Prompt Preview
        lines.push(Line::from(Span::styled(
            "System Prompt:",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )));
        lines.push(Line::from(""));

        // Show the prompt (truncated to fit the area, user can scroll)
        let prompt_lines: Vec<&str> = persona.system_prompt.lines().collect();
        let max_prompt_lines = (area.height as usize).saturating_sub(12); // Leave room for header

        for (_i, line) in prompt_lines.iter().take(max_prompt_lines).enumerate() {
            let style = if line.starts_with('#') {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if line.starts_with('-') || line.starts_with('*') {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(*line, style)));
        }

        if prompt_lines.len() > max_prompt_lines {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("... ({} more lines)", prompt_lines.len() - max_prompt_lines),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        let paragraph = Paragraph::new(lines)
            .block(titled_block("Persona Details"))
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, chunks[1]);
    } else {
        let paragraph = Paragraph::new("Select a persona to view details")
            .block(titled_block("Persona Details"))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(paragraph, chunks[1]);
    }
}

fn render_guild_settings(frame: &mut Frame, app: &App, area: Rect) {
    let settings = vec![
        (
            "default_verbosity",
            "Response length",
            "concise/normal/detailed",
        ),
        ("default_persona", "Default persona", "obi/muppet/chef/..."),
        (
            "conflict_mediation",
            "Conflict detection",
            "enabled/disabled",
        ),
        (
            "conflict_sensitivity",
            "Detection threshold",
            "low/medium/high/ultra",
        ),
        ("mediation_cooldown", "Cooldown (minutes)", "1-60"),
        ("max_context_messages", "Context size", "10-60"),
        (
            "audio_transcription",
            "Audio processing",
            "enabled/disabled",
        ),
        (
            "audio_transcription_mode",
            "When to transcribe",
            "always/mention_only",
        ),
        (
            "mention_responses",
            "Respond to @mentions",
            "enabled/disabled",
        ),
        ("response_embeds", "Use embed boxes", "enabled/disabled"),
    ];

    let items: Vec<ListItem> = settings
        .iter()
        .map(|(_key, name, values)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<25}", name), Style::default().fg(Color::White)),
                Span::styled(
                    format!("[{}]", values),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(titled_block("Guild Settings"))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        )
        .highlight_symbol("> ");

    let mut list_state = ListState::default().with_selected(Some(app.settings_guild_index));
    frame.render_stateful_widget(list, area, &mut list_state);
}
