// Core layer - shared types and configuration
pub mod core;

// Features layer - all feature modules
pub mod features;

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
    // Analytics
    metrics_collection_loop, InteractionTracker, UsageTracker, CurrentMetrics,
    // Audio
    AudioTranscriber, TranscriptionResult,
    // Conflict
    ConflictDetector, ConflictMediator,
    // Image generation
    ImageGenerator, ImageSize, ImageStyle, GeneratedImage,
    // Introspection
    get_component_snippet,
    // Personas
    Persona, PersonaManager,
    // Plugins
    Plugin, PluginConfig, PluginManager, PluginExecutor, JobManager, OutputHandler,
    // Rate limiting
    RateLimiter,
    // Reminders
    ReminderScheduler,
    // Startup
    StartupNotifier,
};