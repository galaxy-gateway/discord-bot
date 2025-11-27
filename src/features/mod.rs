//! # Features Module
//!
//! Central registry for all bot features with version tracking and runtime toggles,
//! plus all feature module declarations.
//!
//! - **Version**: 2.0.0
//! - **Since**: 0.2.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 2.0.0: Reorganized as parent module with feature subdirectories
//! - 1.0.0: Initial feature registry implementation

// Feature submodules
pub mod analytics;
pub mod audio;
pub mod conflict;
pub mod image_gen;
pub mod introspection;
pub mod personas;
pub mod rate_limiting;
pub mod reminders;
pub mod startup;

// Re-export commonly used items from submodules
pub use analytics::{
    metrics_collection_loop, InteractionTracker, UsageTracker, CurrentMetrics,
    format_bytes, format_bytes_signed, format_duration, format_history,
    get_db_file_size, DiskInfo, HistoricalSummary,
};
pub use audio::{AudioTranscriber, TranscriptionResult};
pub use conflict::{ConflictDetector, ConflictMediator};
pub use image_gen::{ImageGenerator, ImageSize, ImageStyle, GeneratedImage};
pub use introspection::get_component_snippet;
pub use personas::{Persona, PersonaManager};
pub use rate_limiting::RateLimiter;
pub use reminders::ReminderScheduler;
pub use startup::StartupNotifier;

// ============================================================================
// Feature Registry
// ============================================================================

/// Describes a versioned bot feature
#[derive(Debug, Clone)]
pub struct Feature {
    /// Feature identifier (snake_case)
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// Current semantic version
    pub version: &'static str,
    /// Bot version when feature was added
    pub since: &'static str,
    /// Can be toggled at runtime by admins
    pub toggleable: bool,
    /// Brief description
    pub description: &'static str,
}

/// All registered features
pub const FEATURES: &[Feature] = &[
    Feature {
        id: "personas",
        name: "Persona System",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: false,
        description: "Multi-personality AI responses with 5 distinct personas",
    },
    Feature {
        id: "reminders",
        name: "Reminders",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: true,
        description: "Scheduled reminder system with persona-aware delivery",
    },
    Feature {
        id: "conflict_detection",
        name: "Conflict Detection",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: true,
        description: "Detects heated discussions using keyword and pattern analysis",
    },
    Feature {
        id: "conflict_mediation",
        name: "Conflict Mediation",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: true,
        description: "Obi-Wan themed interventions for heated conversations",
    },
    Feature {
        id: "image_generation",
        name: "Image Generation",
        version: "1.0.0",
        since: "0.2.0",
        toggleable: true,
        description: "DALL-E 3 powered image creation with size and style options",
    },
    Feature {
        id: "audio_transcription",
        name: "Audio Transcription",
        version: "1.3.0",
        since: "0.1.0",
        toggleable: true,
        description: "Whisper-powered transcription with configurable output modes",
    },
    Feature {
        id: "introspection",
        name: "Self-Introspection",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: false,
        description: "Bot can explain its own internals and architecture",
    },
    Feature {
        id: "rate_limiting",
        name: "Rate Limiting",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: false,
        description: "Prevents spam with configurable request limits per user",
    },
    Feature {
        id: "verbosity_control",
        name: "Verbosity Control",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: false,
        description: "Per-channel response length settings (concise/normal/detailed)",
    },
    Feature {
        id: "guild_settings",
        name: "Guild Settings",
        version: "1.0.0",
        since: "0.1.0",
        toggleable: false,
        description: "Server-wide configuration and defaults",
    },
    Feature {
        id: "system_info",
        name: "System Information",
        version: "1.0.0",
        since: "0.3.0",
        toggleable: false,
        description: "System diagnostics and historical resource metrics tracking",
    },
    Feature {
        id: "startup_notification",
        name: "Startup Notification",
        version: "1.1.0",
        since: "0.4.0",
        toggleable: true,
        description: "Rich notifications when bot comes online, configured via /set_guild_setting",
    },
    Feature {
        id: "usage_tracking",
        name: "Usage Tracking",
        version: "1.0.0",
        since: "0.5.0",
        toggleable: false,
        description: "OpenAI API usage and cost tracking with /usage command",
    },
    Feature {
        id: "dm_interaction_tracking",
        name: "DM Interaction Tracking",
        version: "1.0.0",
        since: "0.6.0",
        toggleable: false,
        description: "Comprehensive DM session and engagement metrics with user-facing analytics",
    },
];

/// Get all registered features
pub fn get_features() -> &'static [Feature] {
    FEATURES
}

/// Get a feature by ID
pub fn get_feature(id: &str) -> Option<&'static Feature> {
    FEATURES.iter().find(|f| f.id == id)
}

/// Get all toggleable features
pub fn get_toggleable_features() -> impl Iterator<Item = &'static Feature> {
    FEATURES.iter().filter(|f| f.toggleable)
}

/// Get bot version from Cargo.toml
pub fn get_bot_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Format features as a display string
pub fn format_features_list() -> String {
    let mut output = format!("📦 Bot Features (v{})\n\n", get_bot_version());
    output.push_str("Feature              Version  Status    Toggleable\n");
    output.push_str("─────────────────────────────────────────────────────\n");

    for feature in FEATURES {
        let toggle_str = if feature.toggleable { "Yes" } else { "No" };
        output.push_str(&format!(
            "{:<20} {:<8} ✅ ON     {}\n",
            feature.name, feature.version, toggle_str
        ));
    }

    output.push_str("\nUse /toggle <feature_id> to enable/disable toggleable features.");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_features() {
        let features = get_features();
        assert!(!features.is_empty());
        assert!(features.len() >= 13, "Should have at least 13 features");
    }

    #[test]
    fn test_get_feature_by_id() {
        let personas = get_feature("personas");
        assert!(personas.is_some());
        assert_eq!(personas.unwrap().name, "Persona System");

        let nonexistent = get_feature("nonexistent");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_toggleable_features() {
        let toggleable: Vec<_> = get_toggleable_features().collect();
        assert!(!toggleable.is_empty());

        // Verify all returned features are actually toggleable
        for feature in toggleable {
            assert!(feature.toggleable);
        }
    }

    #[test]
    fn test_feature_ids_unique() {
        let features = get_features();
        let mut ids: Vec<_> = features.iter().map(|f| f.id).collect();
        let original_len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), original_len, "Feature IDs should be unique");
    }

    #[test]
    fn test_format_features_list() {
        let output = format_features_list();
        assert!(output.contains("Bot Features"));
        assert!(output.contains("Persona System"));
        assert!(output.contains("Toggleable"));
    }
}
