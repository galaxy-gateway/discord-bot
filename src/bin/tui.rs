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

use persona::ipc::{IpcClient, connect_with_retry};
use persona::tui::{App, Screen, Event, EventHandler};
use persona::tui::event::{map_key_event, KeyAction};
use persona::tui::app::{InputMode, InputPurpose};

/// TUI refresh rate
const TICK_RATE: Duration = Duration::from_millis(250);

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

    // Request initial data if connected
    if let Some(client) = &ipc_client {
        let _ = client.request_status().await;
        let _ = client.request_guilds().await;
        let _ = client.request_usage_stats(Some(7)).await; // Default to week
        let _ = client.request_system_metrics().await;
    }

    // Create event handler
    let (mut events, event_tx) = EventHandler::new(TICK_RATE);

    // Note: IPC event forwarding is handled in run_app main loop
    // since we can't split the client between threads
    let _ = &event_tx; // Suppress unused warning - may be used for future IPC forwarding

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

            // Request data for the new screen
            if let Some(client) = ipc_client {
                match screen {
                    Screen::Users => {
                        if app.users_state.needs_refresh() {
                            app.users_state.start_refresh();
                            let _ = client.request_user_list(50).await;
                        }
                    }
                    Screen::Errors => {
                        if app.errors_state.needs_refresh() {
                            app.errors_state.start_refresh();
                            let _ = client.request_recent_errors(100).await;
                        }
                    }
                    Screen::Stats => {
                        // Request historical metrics for charts
                        let _ = client.request_historical_metrics("cpu".to_string(), 24).await;
                        let _ = client.request_historical_metrics("memory".to_string(), 24).await;
                    }
                    Screen::Channels => {
                        // If a channel is selected and needs history, request it
                        if let Some(channel_id) = app.channel_state.selected() {
                            if app.channel_state.needs_history(channel_id) {
                                app.channel_state.start_fetching_history();
                                let _ = client.get_channel_history(channel_id, 50).await;
                                let _ = client.request_channel_info(channel_id).await;
                            }
                        }
                    }
                    Screen::Settings => {
                        // Request feature states for the settings screen
                        let _ = client.request_feature_states(None).await;
                    }
                    _ => {}
                }
            }
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
                Screen::Users => {
                    if app.users_state.viewing_details {
                        // Scroll DM sessions
                    } else {
                        app.users_state.select_previous();
                    }
                }
                Screen::Errors => {
                    app.errors_state.select_previous();
                }
                Screen::Settings => {
                    let index = app.settings_current_index_mut();
                    if *index > 0 {
                        *index -= 1;
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
                    let max = app.settings_list_len();
                    let index = app.settings_current_index_mut();
                    if *index < max.saturating_sub(1) {
                        *index += 1;
                    }
                }
                Screen::Dashboard => {
                    app.select_next(app.guilds.len());
                }
                Screen::Users => {
                    if app.users_state.viewing_details {
                        // Scroll DM sessions
                    } else {
                        app.users_state.select_next();
                    }
                }
                Screen::Errors => {
                    app.errors_state.select_next();
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
                        // Request channel info and history
                        if let Some(client) = ipc_client {
                            if app.channel_state.needs_history(channel_id) {
                                app.channel_state.start_fetching_history();
                                let _ = client.get_channel_history(channel_id, 50).await;
                            }
                            let _ = client.request_channel_info(channel_id).await;
                        }
                    }
                }
                Screen::Users => {
                    if app.users_state.viewing_details {
                        // Already in details, do nothing
                    } else {
                        // Enter details view and request user details
                        if let Some(user) = app.users_state.selected_user() {
                            let user_id = user.user_id.clone();
                            app.users_state.enter_details();
                            if let Some(client) = ipc_client {
                                let _ = client.request_user_details(user_id).await;
                            }
                        }
                    }
                }
                Screen::Errors => {
                    if app.errors_state.viewing_details {
                        // Already in details
                    } else {
                        app.errors_state.enter_details();
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
                Screen::Users => {
                    if app.users_state.viewing_details {
                        app.users_state.exit_details();
                    } else {
                        app.switch_screen(Screen::Dashboard);
                    }
                }
                Screen::Errors => {
                    if app.errors_state.viewing_details {
                        app.errors_state.exit_details();
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
        KeyAction::StartMessageInput => {
            // Only allow message input when a channel is selected
            if app.current_screen == Screen::Channels && app.channel_state.selected().is_some() {
                app.start_message_input();
            }
        }
        KeyAction::SubmitInput => {
            let input = app.take_input();
            let purpose = app.input_purpose;
            app.stop_editing();

            if !input.is_empty() {
                match app.current_screen {
                    Screen::Channels => {
                        match purpose {
                            InputPurpose::SendMessage => {
                                // Send message to selected channel
                                if let Some(channel_id) = app.channel_state.selected() {
                                    if let Some(client) = ipc_client {
                                        let _ = client.send_message(channel_id, input.clone()).await;
                                    }
                                    app.status_message = Some("Message sent".to_string());
                                }
                            }
                            InputPurpose::AddChannel => {
                                // Parse input as channel ID to watch
                                if let Ok(channel_id) = input.parse::<u64>() {
                                    app.channel_state.watch(channel_id);
                                    if let Some(client) = ipc_client {
                                        let _ = client.watch_channel(channel_id).await;
                                        // Also request channel info
                                        let _ = client.request_channel_info(channel_id).await;
                                    }
                                    app.status_message = Some(format!("Now watching channel {}", channel_id));
                                } else {
                                    app.error_message = Some("Invalid channel ID - must be a number".to_string());
                                }
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
                match app.current_screen {
                    Screen::Users => {
                        app.users_state.start_refresh();
                        let _ = client.request_user_list(50).await;
                        app.status_message = Some("Refreshing users...".to_string());
                    }
                    Screen::Errors => {
                        app.errors_state.start_refresh();
                        let _ = client.request_recent_errors(100).await;
                        app.status_message = Some("Refreshing errors...".to_string());
                    }
                    _ => {
                        let _ = client.request_status().await;
                        let _ = client.request_usage_stats(app.stats_cache.time_period.days()).await;
                        let _ = client.request_system_metrics().await;
                        let _ = client.request_historical_metrics("cpu".to_string(), 24).await;
                        let _ = client.request_historical_metrics("memory".to_string(), 24).await;
                        app.stats_cache.start_refresh();
                        app.status_message = Some("Refreshing...".to_string());
                    }
                }
            }
        }
        KeyAction::Toggle => {
            match app.current_screen {
                Screen::Settings => {
                    if let Some(feature) = persona::features::FEATURES.get(app.settings_feature_index) {
                        if feature.toggleable {
                            // Toggle the local state and get new value
                            let new_state = app.toggle_feature_state(feature.id);

                            if let Some(client) = ipc_client {
                                // Send to server to persist
                                let _ = client.set_feature(
                                    feature.id.to_string(),
                                    new_state,
                                    None, // Global toggle
                                ).await;
                            }

                            let status = if new_state { "enabled" } else { "disabled" };
                            app.status_message = Some(format!("{} {}", feature.name, status));
                        } else {
                            app.error_message = Some("Feature cannot be toggled".to_string());
                        }
                    }
                }
                Screen::Stats => {
                    app.stats_cache.cycle_time_period();
                    // Request stats with new time period
                    if let Some(client) = ipc_client {
                        let _ = client.request_usage_stats(app.stats_cache.time_period.days()).await;
                    }
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
        KeyAction::TabLeft => {
            if app.current_screen == Screen::Settings {
                app.settings_tab_left();
            }
        }
        KeyAction::TabRight => {
            if app.current_screen == Screen::Settings {
                app.settings_tab_right();
            }
        }
        KeyAction::None => {}
    }

    Ok(())
}
