//! # TUI Event Handling
//!
//! Keyboard input and tick event handling.

use crate::ipc::BotEvent;
use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

/// TUI events
#[derive(Debug)]
pub enum Event {
    /// Keyboard input
    Key(KeyEvent),
    /// Terminal resize
    Resize(u16, u16),
    /// IPC event from bot
    Ipc(BotEvent),
    /// Tick for periodic updates
    Tick,
    /// IPC connection lost
    Disconnected,
}

/// Event handler that combines keyboard, IPC, and tick events
pub struct EventHandler {
    /// Event receiver
    rx: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    /// Create a new event handler
    pub fn new(tick_rate: Duration) -> (Self, mpsc::UnboundedSender<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn keyboard event handler
        let key_tx = tx.clone();
        std::thread::spawn(move || {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            if key_tx.send(Event::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Resize(w, h)) => {
                            if key_tx.send(Event::Resize(w, h)).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                } else {
                    // Send tick on poll timeout
                    if key_tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        (EventHandler { rx }, tx)
    }

    /// Receive the next event
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

/// Key action result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// No action
    None,
    /// Quit the application
    Quit,
    /// Switch to screen
    SwitchScreen(crate::tui::Screen),
    /// Move selection up
    Up,
    /// Move selection down
    Down,
    /// Select / Enter
    Select,
    /// Go back / Cancel
    Back,
    /// Start text input (add channel)
    StartInput,
    /// Start message input (send message)
    StartMessageInput,
    /// Submit text input
    SubmitInput,
    /// Cancel text input
    CancelInput,
    /// Character input
    Char(char),
    /// Backspace
    Backspace,
    /// Refresh data
    Refresh,
    /// Toggle item
    Toggle,
    /// Delete item
    Delete,
    /// Page up
    PageUp,
    /// Page down
    PageDown,
    /// Home
    Home,
    /// End
    End,
    /// Move to previous tab
    TabLeft,
    /// Move to next tab
    TabRight,
    /// Start browse mode for channel selection
    StartBrowse,
}

/// Map a key event to an action
pub fn map_key_event(key: KeyEvent, in_edit_mode: bool) -> KeyAction {
    if in_edit_mode {
        // In edit mode, handle text input
        match key.code {
            KeyCode::Esc => KeyAction::CancelInput,
            KeyCode::Enter => KeyAction::SubmitInput,
            KeyCode::Backspace => KeyAction::Backspace,
            KeyCode::Char(c) => KeyAction::Char(c),
            _ => KeyAction::None,
        }
    } else {
        // Normal mode navigation
        match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => KeyAction::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => KeyAction::Quit,

            // Screen switching
            (KeyCode::Char('1'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Dashboard)
            }
            (KeyCode::Char('2'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Channels)
            }
            (KeyCode::Char('3'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Stats)
            }
            (KeyCode::Char('4'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Users)
            }
            (KeyCode::Char('5'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Settings)
            }
            (KeyCode::Char('6'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Errors)
            }
            (KeyCode::Char('?'), KeyModifiers::NONE) => {
                KeyAction::SwitchScreen(crate::tui::Screen::Help)
            }

            // Navigation
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => KeyAction::Up,
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => KeyAction::Down,
            (KeyCode::Enter, _) | (KeyCode::Char(' '), KeyModifiers::NONE) => KeyAction::Select,
            (KeyCode::Esc, _) => KeyAction::Back,

            // Browse mode (context-aware: enters browse mode or goes back depending on screen)
            (KeyCode::Char('b'), KeyModifiers::NONE) => KeyAction::StartBrowse,

            // Text input
            (KeyCode::Char('i'), KeyModifiers::NONE) => KeyAction::StartInput,
            (KeyCode::Char('/'), KeyModifiers::NONE) => KeyAction::StartInput,
            (KeyCode::Char('m'), KeyModifiers::NONE) => KeyAction::StartMessageInput,

            // Actions
            (KeyCode::Char('r'), KeyModifiers::NONE) => KeyAction::Refresh,
            (KeyCode::Char('t'), KeyModifiers::NONE) => KeyAction::Toggle,
            (KeyCode::Char('d'), KeyModifiers::NONE) => KeyAction::Delete,

            // Page navigation
            (KeyCode::PageUp, _) => KeyAction::PageUp,
            (KeyCode::PageDown, _) => KeyAction::PageDown,
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => KeyAction::Home,
            (KeyCode::End, _) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => KeyAction::End,

            // Tab navigation (for Settings screen tabs)
            (KeyCode::Left, KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                KeyAction::TabLeft
            }
            (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                KeyAction::TabRight
            }
            (KeyCode::Tab, KeyModifiers::NONE) => KeyAction::TabRight,
            (KeyCode::BackTab, _) => KeyAction::TabLeft,

            _ => KeyAction::None,
        }
    }
}
