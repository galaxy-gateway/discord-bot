//! # Obi TUI
//!
//! Terminal user interface for controlling and monitoring the Obi Discord bot.
//!
//! Usage: `cargo run --features tui --bin obi-tui`

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::{error, info};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

use persona::ipc::{IpcClient, BotEvent, TuiCommand, connect_with_retry};
use persona::tui::{App, Screen, Event, EventHandler};
use persona::tui::event::{map_key_event, KeyAction};
use persona::tui::app::InputMode;

/// TUI refresh rate
const TICK_RATE: Duration = Duration::from_millis(250);

/// Stats refresh interval
const STATS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn")
    ).init();

    info!("Starting Obi TUI...");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new();

    // Try to connect to bot
    app.add_activity("Connecting to bot...".to_string());
    let ipc_result: Result<IpcClient> = connect_with_retry(3, Duration::from_secs(2)).await;

    let mut ipc_client: Option<IpcClient> = match ipc_result {
        Ok(client) => {
            app.set_connected(true);
            app.add_activity("Connected to IPC server".to_string());
            Some(client)
        }
        Err(e) => {
            app.add_activity(format!("Failed to connect: {}", e));
            app.error_message = Some(format!("Not connected: {}", e));
            None
        }
    };

    // Request initial status if connected
    if let Some(client) = &ipc_client {
        let _ = client.request_status().await;
        let _ = client.request_guilds().await;
    }

    // Create event handler
    let (mut events, event_tx) = EventHandler::new(TICK_RATE);

    // Spawn IPC event forwarder if connected
    if let Some(ref mut client) = ipc_client {
        let tx = event_tx.clone();
        // We need to move event receiving to the main loop since we can't split the client
    }

    // Main loop
    let result = run_app(&mut terminal, &mut app, &mut events, &mut ipc_client).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        error!("Application error: {}", e);
        return Err(e);
    }

    info!("Obi TUI shutdown complete");
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events: &mut EventHandler,
    ipc_client: &mut Option<IpcClient>,
) -> Result<()> {
    loop {
        // Draw UI
        terminal.draw(|frame| {
            persona::tui::ui::render(frame, app);
        })?;

        // Check for IPC events
        if let Some(client) = ipc_client {
            while let Some(event) = client.try_recv() {
                app.handle_bot_event(event);
            }

            // Check connection status
            if !client.is_connected().await {
                app.set_connected(false);
                app.add_activity("IPC connection lost".to_string());
            }
        }

        // Handle events
        if let Some(event) = events.next().await {
            match event {
                Event::Key(key) => {
                    let action = map_key_event(key, app.input_mode == InputMode::Editing);
                    handle_action(app, action, ipc_client).await?;
                }
                Event::Ipc(bot_event) => {
                    app.handle_bot_event(bot_event);
                }
                Event::Tick => {
                    // Clear transient messages after a while
                    // (In a real app, we'd track message age)
                }
                Event::Resize(_, _) => {
                    // Terminal will redraw automatically
                }
                Event::Disconnected => {
                    app.set_connected(false);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

async fn handle_action(
    app: &mut App,
    action: KeyAction,
    ipc_client: &mut Option<IpcClient>,
) -> Result<()> {
    match action {
        KeyAction::Quit => {
            app.should_quit = true;
        }
        KeyAction::SwitchScreen(screen) => {
            app.switch_screen(screen);
            app.clear_error();
            app.clear_status();
        }
        KeyAction::Up => {
            match app.current_screen {
                Screen::Channels => {
                    if app.channel_state.selected().is_some() {
                        app.channel_state.scroll_up(1);
                    } else {
                        app.select_previous();
                    }
                }
                _ => app.select_previous(),
            }
        }
        KeyAction::Down => {
            match app.current_screen {
                Screen::Channels => {
                    if let Some(messages) = app.channel_state.get_selected_messages() {
                        app.channel_state.scroll_down(1, messages.len());
                    } else {
                        let max = app.channel_state.watched_channels().len();
                        app.select_next(max);
                    }
                }
                Screen::Settings => {
                    app.select_next(persona::features::FEATURES.len());
                }
                Screen::Dashboard => {
                    app.select_next(app.guilds.len());
                }
                _ => {}
            }
        }
        KeyAction::Select => {
            match app.current_screen {
                Screen::Channels => {
                    let watched = app.channel_state.watched_channels();
                    if let Some(&channel_id) = watched.get(app.selected_index) {
                        app.channel_state.select(channel_id);
                    }
                }
                _ => {}
            }
        }
        KeyAction::Back => {
            match app.current_screen {
                Screen::Channels => {
                    if app.channel_state.selected().is_some() {
                        app.channel_state.clear_selection();
                    } else {
                        app.switch_screen(Screen::Dashboard);
                    }
                }
                Screen::Help => {
                    app.switch_screen(Screen::Dashboard);
                }
                _ => {}
            }
        }
        KeyAction::StartInput => {
            app.start_editing();
        }
        KeyAction::SubmitInput => {
            let input = app.take_input();
            app.stop_editing();

            if !input.is_empty() {
                match app.current_screen {
                    Screen::Channels => {
                        if let Some(channel_id) = app.channel_state.selected() {
                            // Send message
                            if let Some(client) = ipc_client {
                                match client.send_message(channel_id, input.clone()).await {
                                    Ok(_) => {
                                        app.status_message = Some("Message sent".to_string());
                                    }
                                    Err(e) => {
                                        app.error_message = Some(format!("Failed: {}", e));
                                    }
                                }
                            }
                        } else {
                            // Try to parse as channel ID to watch
                            if let Ok(channel_id) = input.parse::<u64>() {
                                app.channel_state.watch(channel_id);
                                if let Some(client) = ipc_client {
                                    let _ = client.watch_channel(channel_id).await;
                                }
                                app.status_message = Some(format!("Watching channel {}", channel_id));
                            } else {
                                app.error_message = Some("Invalid channel ID".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        KeyAction::CancelInput => {
            app.input_clear();
            app.stop_editing();
        }
        KeyAction::Char(c) => {
            app.input_char(c);
        }
        KeyAction::Backspace => {
            app.input_backspace();
        }
        KeyAction::Refresh => {
            if let Some(client) = ipc_client {
                let _ = client.request_status().await;
                app.status_message = Some("Refreshing...".to_string());
            }
        }
        KeyAction::Toggle => {
            match app.current_screen {
                Screen::Settings => {
                    if let Some(feature) = persona::features::FEATURES.get(app.selected_index) {
                        if feature.toggleable {
                            if let Some(client) = ipc_client {
                                // Toggle feature (we'd need to track current state)
                                let _ = client.set_feature(
                                    feature.id.to_string(),
                                    true, // TODO: Toggle actual state
                                    None,
                                ).await;
                                app.status_message = Some(format!("Toggled {}", feature.name));
                            }
                        } else {
                            app.error_message = Some("Feature cannot be toggled".to_string());
                        }
                    }
                }
                Screen::Stats => {
                    app.stats_cache.cycle_time_period();
                }
                _ => {}
            }
        }
        KeyAction::Delete => {
            match app.current_screen {
                Screen::Channels => {
                    let watched = app.channel_state.watched_channels();
                    if let Some(&channel_id) = watched.get(app.selected_index) {
                        app.channel_state.unwatch(channel_id);
                        if let Some(client) = ipc_client {
                            let _ = client.unwatch_channel(channel_id).await;
                        }
                        app.status_message = Some(format!("Unwatched channel {}", channel_id));
                    }
                }
                _ => {}
            }
        }
        KeyAction::PageUp => {
            if app.current_screen == Screen::Channels {
                app.channel_state.scroll_up(10);
            }
        }
        KeyAction::PageDown => {
            if app.current_screen == Screen::Channels {
                if let Some(messages) = app.channel_state.get_selected_messages() {
                    app.channel_state.scroll_down(10, messages.len());
                }
            }
        }
        KeyAction::Home => {
            if app.current_screen == Screen::Channels {
                app.channel_state.scroll_to_top();
            } else {
                app.selected_index = 0;
            }
        }
        KeyAction::End => {
            if app.current_screen == Screen::Channels {
                app.channel_state.scroll_to_bottom();
            }
        }
        KeyAction::None => {}
    }

    Ok(())
}
