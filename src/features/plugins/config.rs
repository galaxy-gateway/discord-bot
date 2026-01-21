//! # Plugin Configuration Schema
//!
//! YAML-based plugin configuration with full schema validation.
//!
//! - **Version**: 3.1.0
//! - **Since**: 0.9.0
//!
//! ## Changelog
//! - 3.1.0: Added download_command/download_args for configurable audio download
//! - 3.0.0: Added chunking configuration for long video streaming transcription
//! - 2.0.0: Added playlist configuration for multi-video transcription
//! - 1.1.0: Added source_param for structured output posting
//! - 1.0.0: Initial release

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration containing all plugins
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginConfig {
    pub plugins: Vec<Plugin>,
}

impl PluginConfig {
    /// Load plugin configuration from a YAML file
    pub fn load(path: &str) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: PluginConfig = serde_yaml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Create an empty configuration
    pub fn empty() -> Self {
        Self { plugins: vec![] }
    }

    /// Validate all plugins in the configuration
    pub fn validate(&self) -> Result<()> {
        for plugin in &self.plugins {
            // Validate command name format (lowercase, underscores allowed)
            if !plugin
                .command
                .name
                .chars()
                .all(|c| c.is_lowercase() || c == '_')
            {
                return Err(anyhow::anyhow!(
                    "Command name must be lowercase: {}",
                    plugin.command.name
                ));
            }

            // Validate command name length (Discord limit)
            if plugin.command.name.len() > 32 {
                return Err(anyhow::anyhow!(
                    "Command name too long (max 32 chars): {}",
                    plugin.command.name
                ));
            }

            // Validate description length (Discord limit)
            if plugin.command.description.len() > 100 {
                return Err(anyhow::anyhow!(
                    "Command description too long (max 100 chars): {}",
                    plugin.name
                ));
            }

            // Validate required fields (allow empty for virtual plugins)
            // Virtual plugins are handled internally (e.g., transcribe_cancel)
            // and don't need a CLI command

            // Validate regex patterns compile
            for opt in &plugin.command.options {
                if let Some(ref validation) = opt.validation {
                    if let Some(ref pattern) = validation.pattern {
                        regex::Regex::new(pattern).map_err(|e| {
                            anyhow::anyhow!(
                                "Invalid regex pattern for option '{}' in plugin '{}': {}",
                                opt.name,
                                plugin.name,
                                e
                            )
                        })?;
                    }
                }

                // Validate option name format
                if !opt.name.chars().all(|c| c.is_lowercase() || c == '_') {
                    return Err(anyhow::anyhow!(
                        "Option name must be lowercase: {} in plugin {}",
                        opt.name,
                        plugin.name
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A single plugin definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plugin {
    /// Unique plugin identifier
    pub name: String,

    /// Human-readable description
    pub description: String,

    /// Whether the plugin is active
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Semantic version
    pub version: String,

    /// Slash command definition
    pub command: CommandDefinition,

    /// CLI execution configuration
    pub execution: ExecutionConfig,

    /// Security constraints
    #[serde(default)]
    pub security: SecurityConfig,

    /// Output handling configuration
    #[serde(default)]
    pub output: OutputConfig,

    /// Playlist-specific configuration (optional)
    #[serde(default)]
    pub playlist: Option<PlaylistConfig>,
}

impl Plugin {
    /// Check if this plugin is a "virtual" plugin (handled internally, no CLI execution)
    pub fn is_virtual(&self) -> bool {
        self.execution.command.is_empty()
    }
}

/// Slash command definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandDefinition {
    /// Command name (lowercase, no spaces)
    pub name: String,

    /// Command description shown in Discord
    pub description: String,

    /// Command parameters
    #[serde(default)]
    pub options: Vec<CommandOption>,
}

/// A single command option/parameter
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandOption {
    /// Option name
    pub name: String,

    /// Option description
    pub description: String,

    /// Option type: string, integer, boolean, user, channel, role
    #[serde(rename = "type", default = "default_string")]
    pub option_type: String,

    /// Whether the option is required
    #[serde(default)]
    pub required: bool,

    /// Default value if not provided
    #[serde(default)]
    pub default: Option<String>,

    /// Validation rules
    #[serde(default)]
    pub validation: Option<ValidationRule>,

    /// Predefined choices
    #[serde(default)]
    pub choices: Vec<Choice>,
}

/// Validation rules for an option
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ValidationRule {
    /// Regex pattern for validation
    pub pattern: Option<String>,

    /// Minimum string length
    pub min_length: Option<usize>,

    /// Maximum string length
    pub max_length: Option<usize>,

    /// Minimum numeric value
    pub min_value: Option<i64>,

    /// Maximum numeric value
    pub max_value: Option<i64>,
}

/// A predefined choice for an option
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Choice {
    /// Display name
    pub name: String,

    /// Actual value
    pub value: String,
}

/// CLI execution configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionConfig {
    /// Base command to execute
    pub command: String,

    /// Command arguments with ${param} placeholders
    #[serde(default)]
    pub args: Vec<String>,

    /// Maximum execution time in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,

    /// Working directory for command
    pub working_directory: Option<String>,

    /// Maximum output size in bytes
    #[serde(default = "default_max_output")]
    pub max_output_bytes: usize,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Chunking configuration for long content (optional)
    #[serde(default)]
    pub chunking: Option<ChunkingConfig>,
}

/// Configuration for chunked execution of long content
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChunkingConfig {
    /// Whether chunking is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Duration of each chunk in seconds (default: 600 = 10 minutes)
    #[serde(default = "default_chunk_duration")]
    pub chunk_duration_secs: u64,

    /// Timeout for each chunk transcription in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_chunk_timeout")]
    pub chunk_timeout_secs: u64,

    /// Timeout for audio download in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_download_timeout")]
    pub download_timeout_secs: u64,

    /// Minimum video duration to trigger chunking (default: 600 = 10 minutes)
    /// Videos shorter than this will use the standard single-execution mode
    #[serde(default = "default_chunk_duration")]
    pub min_duration_for_chunking_secs: u64,

    /// Command template for transcribing a local audio file
    /// Use ${file} for the input file path, ${output_dir} for output directory
    pub file_command: Option<String>,

    /// Arguments for the file command
    #[serde(default)]
    pub file_args: Vec<String>,

    /// Command for downloading audio (default: yt-dlp directly)
    /// Use ${url} for the video URL, ${output_dir} for temp directory
    pub download_command: Option<String>,

    /// Arguments for the download command
    #[serde(default)]
    pub download_args: Vec<String>,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            chunk_duration_secs: 600,        // 10 minutes
            chunk_timeout_secs: 300,         // 5 minutes per chunk
            download_timeout_secs: 300,      // 5 minutes for download
            min_duration_for_chunking_secs: 600, // 10 minutes
            file_command: None,
            file_args: Vec::new(),
            download_command: None,
            download_args: Vec::new(),
        }
    }
}

/// Security constraints
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SecurityConfig {
    /// Role IDs that can use this plugin
    #[serde(default)]
    pub allowed_roles: Vec<String>,

    /// User IDs that can use this plugin
    #[serde(default)]
    pub allowed_users: Vec<String>,

    /// User IDs blocked from this plugin
    #[serde(default)]
    pub blocked_users: Vec<String>,

    /// Per-user cooldown in seconds
    #[serde(default)]
    pub cooldown_seconds: u64,

    /// Restrict to guild channels only (no DMs)
    #[serde(default)]
    pub guild_only: bool,
}

/// Output handling configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OutputConfig {
    /// Create a thread for output
    #[serde(default)]
    pub create_thread: bool,

    /// Thread name template with ${param} placeholders
    pub thread_name_template: Option<String>,

    /// Thread auto-archive duration in minutes (60, 1440, 4320, 10080)
    #[serde(default = "default_archive")]
    pub auto_archive_minutes: u64,

    /// Post large output as file attachment
    #[serde(default)]
    pub post_as_file: bool,

    /// Filename template with ${param}/${timestamp} placeholders
    pub file_name_template: Option<String>,

    /// Maximum characters before using file (when post_as_file is true)
    #[serde(default = "default_max_inline")]
    pub max_inline_length: usize,

    /// OpenAI prompt for summarization
    pub summary_prompt: Option<String>,

    /// Custom error message template with ${error} placeholder
    pub error_template: Option<String>,

    /// Parameter name containing the source URL to post first in thread
    /// When set, uses structured output: URL -> Summary -> File
    pub source_param: Option<String>,
}

/// Playlist-specific configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaylistConfig {
    /// Whether playlist support is enabled for this plugin
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum videos allowed per playlist request
    #[serde(default = "default_max_videos")]
    pub max_videos_per_request: u32,

    /// Default max videos if not specified by user
    #[serde(default = "default_default_videos")]
    pub default_max_videos: u32,

    /// Maximum concurrent playlists per user
    #[serde(default = "default_one")]
    pub concurrent_playlists_per_user: u32,

    /// Cooldown between playlist starts in seconds
    #[serde(default)]
    pub cooldown_between_playlists: u64,

    /// Minimum interval between video processing in seconds
    #[serde(default = "default_interval")]
    pub min_video_interval_seconds: u64,
}

impl Default for PlaylistConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_videos_per_request: 50,
            default_max_videos: 25,
            concurrent_playlists_per_user: 1,
            cooldown_between_playlists: 300,
            min_video_interval_seconds: 5,
        }
    }
}

// Default value functions
fn default_true() -> bool {
    true
}

fn default_string() -> String {
    "string".to_string()
}

fn default_timeout() -> u64 {
    300 // 5 minutes
}

fn default_max_output() -> usize {
    10_485_760 // 10MB
}

fn default_archive() -> u64 {
    60 // 1 hour
}

fn default_max_inline() -> usize {
    1500
}

fn default_max_videos() -> u32 {
    50
}

fn default_default_videos() -> u32 {
    25
}

fn default_one() -> u32 {
    1
}

fn default_interval() -> u64 {
    5
}

fn default_chunk_duration() -> u64 {
    600 // 10 minutes
}

fn default_chunk_timeout() -> u64 {
    300 // 5 minutes
}

fn default_download_timeout() -> u64 {
    300 // 5 minutes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let yaml = r#"
plugins:
  - name: test
    description: Test plugin
    version: "1.0.0"
    command:
      name: test
      description: Test command
    execution:
      command: echo
      args:
        - "hello"
"#;
        let config: PluginConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.plugins.len(), 1);
        assert_eq!(config.plugins[0].name, "test");
        assert!(config.plugins[0].enabled); // default true
    }

    #[test]
    fn test_parse_full_config() {
        let yaml = r#"
plugins:
  - name: transcribe
    description: Transcribe videos
    enabled: true
    version: "1.0.0"
    command:
      name: transcribe
      description: Transcribe a video
      options:
        - name: url
          description: Video URL
          type: string
          required: true
          validation:
            pattern: "^https?://"
            max_length: 200
    execution:
      command: docker
      args:
        - run
        - --rm
        - quietly
        - "${url}"
      timeout_seconds: 600
    security:
      cooldown_seconds: 60
    output:
      create_thread: true
      thread_name_template: "Transcript: ${url}"
      post_as_file: true
      max_inline_length: 1500
"#;
        let config: PluginConfig = serde_yaml::from_str(yaml).unwrap();
        config.validate().unwrap();

        let plugin = &config.plugins[0];
        assert_eq!(plugin.name, "transcribe");
        assert_eq!(plugin.execution.timeout_seconds, 600);
        assert_eq!(plugin.security.cooldown_seconds, 60);
        assert!(plugin.output.create_thread);
    }

    #[test]
    fn test_validate_invalid_command_name() {
        let yaml = r#"
plugins:
  - name: test
    description: Test
    version: "1.0.0"
    command:
      name: TestCommand
      description: Invalid name
    execution:
      command: echo
"#;
        let config: PluginConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_virtual_plugin_allowed() {
        // Virtual plugins (like transcribe_cancel) have empty commands
        // and are handled internally by the bot
        let yaml = r#"
plugins:
  - name: test_cancel
    description: Cancel test
    version: "1.0.0"
    command:
      name: test_cancel
      description: Cancel a test operation
    execution:
      command: ""
"#;
        let config: PluginConfig = serde_yaml::from_str(yaml).unwrap();
        // Should validate successfully - virtual plugins are allowed
        assert!(config.validate().is_ok());
        assert!(config.plugins[0].is_virtual());
    }

    #[test]
    fn test_validate_invalid_regex() {
        let yaml = r#"
plugins:
  - name: test
    description: Test
    version: "1.0.0"
    command:
      name: test
      description: Test
      options:
        - name: input
          description: Input
          validation:
            pattern: "[invalid"
    execution:
      command: echo
"#;
        let config: PluginConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }
}
