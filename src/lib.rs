// Core layer - shared types and configuration
pub mod core;

// Features layer - all feature modules
pub mod features;

// IPC layer - communication between bot and TUI
pub mod ipc;

// TUI layer - terminal user interface (optional feature)
#[cfg(feature = "tui")]
pub mod tui;

// UI components (to be moved to presentation/)
pub mod message_components;

// Infrastructure (to be reorganized)
pub mod database;

// Application layer
pub mod command_handler;
pub mod commands;

// Re-export core config for backwards compatibility
pub use core::Config;

// Re-export feature items for backwards compatibility
pub use features::{
    // Introspection
    get_component_snippet,
    // Analytics
    metrics_collection_loop,
    // Audio
    AudioTranscriber,
    // Conflict
    ConflictDetector,
    ConflictMediator,
    CurrentMetrics,
    DebateConfig,
    // Debate
    DebateOrchestrator,
    GeneratedImage,
    // Image generation
    ImageGenerator,
    ImageSize,
    ImageStyle,
    InteractionTracker,
    JobManager,
    OutputHandler,
    // Personas
    Persona,
    PersonaManager,
    // Plugins
    Plugin,
    PluginConfig,
    PluginExecutor,
    PluginManager,
    // Rate limiting
    RateLimiter,
    // Reminders
    ReminderScheduler,
    // Startup
    StartupNotifier,
    TranscriptionResult,
    UsageTracker,
};

// Re-export IPC items
pub use ipc::{BotEvent, IpcClient, IpcServer, TuiCommand};
