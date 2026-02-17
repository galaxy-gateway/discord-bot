//! # Plugin Configuration Schema
//!
//! YAML-based plugin configuration with full schema validation.
//! Supports both monolithic `plugins.yaml` and per-plugin directory (`plugins/`) loading.
//!
//! - **Version**: 4.0.0
//! - **Since**: 0.9.0
//!
//! ## Changelog
//! - 4.0.0: Added PluginType presets (shell/api/docker/virtual), RawPlugin with resolve(),
//!   per-file directory loading (load_dir/load_auto), script sugar, command name inference
//! - 3.4.0: Added chunk_summary_prompt to OutputConfig for casual per-chunk summaries
//! - 3.3.0: Added recommended_max_videos to PlaylistConfig for flexible playlist limits
//! - 3.2.0: Added cumulative_summaries option for progressive summarization during chunked transcription
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

    /// Whether to generate cumulative "story so far" summaries during transcription
    /// When enabled, posts a progressive summary showing all content transcribed so far
    #[serde(default)]
    pub cumulative_summaries: bool,

    /// Generate cumulative summary every N chunks (default: 1 = every chunk)
    /// Set higher to reduce API calls for very long videos
    #[serde(default = "default_one")]
    pub cumulative_summary_interval: u32,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            chunk_duration_secs: 600,            // 10 minutes
            chunk_timeout_secs: 300,             // 5 minutes per chunk
            download_timeout_secs: 300,          // 5 minutes for download
            min_duration_for_chunking_secs: 600, // 10 minutes
            file_command: None,
            file_args: Vec::new(),
            download_command: None,
            download_args: Vec::new(),
            cumulative_summaries: false,    // off by default
            cumulative_summary_interval: 1, // every chunk when enabled
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

    /// Simpler prompt for per-chunk summaries (casual, no formal structure)
    pub chunk_summary_prompt: Option<String>,

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

    /// Maximum videos allowed per playlist request (0 = no hard limit)
    #[serde(default = "default_max_videos")]
    pub max_videos_per_request: u32,

    /// Default max videos if not specified by user
    #[serde(default = "default_default_videos")]
    pub default_max_videos: u32,

    /// Soft recommendation for max videos (shown in UI)
    #[serde(default = "default_max_videos")]
    pub recommended_max_videos: u32,

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
            max_videos_per_request: 0, // 0 = no hard limit
            default_max_videos: 25,
            recommended_max_videos: 50,
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

// ─── Plugin Type Presets ───────────────────────────────────────────────

/// Plugin type preset that provides sensible defaults
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    Shell,
    Api,
    Docker,
    Virtual,
}

/// Default values derived from plugin type
struct TypeDefaults {
    execution_command: &'static str,
    timeout_seconds: u64,
    max_output_bytes: usize,
    create_thread: bool,
    max_inline_length: usize,
}

impl PluginType {
    fn defaults(self) -> TypeDefaults {
        match self {
            PluginType::Shell => TypeDefaults {
                execution_command: "sh",
                timeout_seconds: 30,
                max_output_bytes: 4096,
                create_thread: false,
                max_inline_length: 2000,
            },
            PluginType::Api => TypeDefaults {
                execution_command: "sh",
                timeout_seconds: 15,
                max_output_bytes: 4096,
                create_thread: false,
                max_inline_length: 2000,
            },
            PluginType::Docker => TypeDefaults {
                execution_command: "docker",
                timeout_seconds: 600,
                max_output_bytes: 10_485_760,
                create_thread: true,
                max_inline_length: 1500,
            },
            PluginType::Virtual => TypeDefaults {
                execution_command: "",
                timeout_seconds: 5,
                max_output_bytes: 0,
                create_thread: false,
                max_inline_length: 2000,
            },
        }
    }
}

// ─── Raw Plugin (pre-resolution) ──────────────────────────────────────

/// Raw plugin as read from individual YAML file (before type-default resolution)
#[derive(Debug, Clone, Deserialize)]
pub struct RawPlugin {
    pub name: String,
    pub description: String,
    pub version: String,

    /// Plugin type preset (shell, api, docker, virtual)
    #[serde(rename = "type")]
    pub plugin_type: Option<PluginType>,

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub command: Option<RawCommandDefinition>,

    #[serde(default)]
    pub execution: Option<RawExecutionConfig>,

    #[serde(default)]
    pub security: Option<SecurityConfig>,

    #[serde(default)]
    pub output: Option<RawOutputConfig>,

    #[serde(default)]
    pub playlist: Option<PlaylistConfig>,
}

/// Command definition with optional name (defaults to plugin name)
#[derive(Debug, Clone, Deserialize)]
pub struct RawCommandDefinition {
    pub name: Option<String>,
    pub description: String,
    #[serde(default)]
    pub options: Vec<CommandOption>,
}

/// Execution config with optional fields and script sugar
#[derive(Debug, Clone, Deserialize)]
pub struct RawExecutionConfig {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    /// Sugar: `script: |` becomes `command: "sh"`, `args: ["-c", script]`
    pub script: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub working_directory: Option<String>,
    pub max_output_bytes: Option<usize>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub chunking: Option<ChunkingConfig>,
}

/// Output config with optional type-defaulted fields
#[derive(Debug, Clone, Deserialize)]
pub struct RawOutputConfig {
    pub create_thread: Option<bool>,
    pub thread_name_template: Option<String>,
    pub auto_archive_minutes: Option<u64>,
    pub post_as_file: Option<bool>,
    pub file_name_template: Option<String>,
    pub max_inline_length: Option<usize>,
    pub summary_prompt: Option<String>,
    pub chunk_summary_prompt: Option<String>,
    pub error_template: Option<String>,
    pub source_param: Option<String>,
}

impl RawPlugin {
    /// Resolve a RawPlugin into a fully-populated Plugin by merging type defaults
    pub fn resolve(self) -> Plugin {
        let td = self.plugin_type.map(|t| t.defaults());

        // Command: name defaults to plugin name
        let command = match self.command {
            Some(raw_cmd) => CommandDefinition {
                name: raw_cmd.name.unwrap_or_else(|| self.name.clone()),
                description: raw_cmd.description,
                options: raw_cmd.options,
            },
            None => CommandDefinition {
                name: self.name.clone(),
                description: self.description.clone(),
                options: vec![],
            },
        };

        // Execution: handle script sugar, then merge type defaults
        let execution = match self.execution {
            Some(raw_exec) => {
                let (cmd, args) = if let Some(script) = raw_exec.script {
                    // script sugar: sh -c <script>
                    ("sh".to_string(), vec!["-c".to_string(), script])
                } else {
                    let cmd = raw_exec.command.unwrap_or_else(|| {
                        td.as_ref()
                            .map(|d| d.execution_command.to_string())
                            .unwrap_or_default()
                    });
                    (cmd, raw_exec.args.unwrap_or_default())
                };

                ExecutionConfig {
                    command: cmd,
                    args,
                    timeout_seconds: raw_exec.timeout_seconds.unwrap_or_else(|| {
                        td.as_ref().map(|d| d.timeout_seconds).unwrap_or(300)
                    }),
                    working_directory: raw_exec.working_directory,
                    max_output_bytes: raw_exec.max_output_bytes.unwrap_or_else(|| {
                        td.as_ref()
                            .map(|d| d.max_output_bytes)
                            .unwrap_or(10_485_760)
                    }),
                    env: raw_exec.env.unwrap_or_default(),
                    chunking: raw_exec.chunking,
                }
            }
            None => ExecutionConfig {
                command: td
                    .as_ref()
                    .map(|d| d.execution_command.to_string())
                    .unwrap_or_default(),
                args: vec![],
                timeout_seconds: td.as_ref().map(|d| d.timeout_seconds).unwrap_or(300),
                working_directory: None,
                max_output_bytes: td
                    .as_ref()
                    .map(|d| d.max_output_bytes)
                    .unwrap_or(10_485_760),
                env: HashMap::new(),
                chunking: None,
            },
        };

        // Output: merge type defaults for create_thread and max_inline_length
        let output = match self.output {
            Some(raw_out) => OutputConfig {
                create_thread: raw_out.create_thread.unwrap_or_else(|| {
                    td.as_ref().map(|d| d.create_thread).unwrap_or(false)
                }),
                max_inline_length: raw_out.max_inline_length.unwrap_or_else(|| {
                    td.as_ref().map(|d| d.max_inline_length).unwrap_or(1500)
                }),
                thread_name_template: raw_out.thread_name_template,
                auto_archive_minutes: raw_out.auto_archive_minutes.unwrap_or(60),
                post_as_file: raw_out.post_as_file.unwrap_or(false),
                file_name_template: raw_out.file_name_template,
                summary_prompt: raw_out.summary_prompt,
                chunk_summary_prompt: raw_out.chunk_summary_prompt,
                error_template: raw_out.error_template,
                source_param: raw_out.source_param,
            },
            None => {
                let mut out = OutputConfig::default();
                if let Some(ref d) = td {
                    out.create_thread = d.create_thread;
                    out.max_inline_length = d.max_inline_length;
                }
                out
            }
        };

        Plugin {
            name: self.name,
            description: self.description,
            enabled: self.enabled,
            version: self.version,
            command,
            execution,
            security: self.security.unwrap_or_default(),
            output,
            playlist: self.playlist,
        }
    }
}

// ─── Directory Loading ────────────────────────────────────────────────

impl PluginConfig {
    /// Load all plugin YAML files from a directory
    ///
    /// Each file is parsed as a `RawPlugin`, resolved with type defaults,
    /// and collected into a `PluginConfig`. Files are sorted alphabetically
    /// for deterministic load order.
    pub fn load_dir(dir: &str) -> Result<Self> {
        let dir_path = std::path::Path::new(dir);
        if !dir_path.is_dir() {
            return Err(anyhow::anyhow!("{} is not a directory", dir));
        }

        let mut entries: Vec<_> = std::fs::read_dir(dir_path)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "yaml" || ext == "yml")
                    .unwrap_or(false)
            })
            .collect();

        entries.sort_by_key(|e| e.file_name());

        let mut plugins = Vec::new();
        for entry in entries {
            let path = entry.path();
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let contents = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", filename, e))?;
            let raw: RawPlugin = serde_yaml::from_str(&contents)
                .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", filename, e))?;
            plugins.push(raw.resolve());
        }

        let config = PluginConfig { plugins };
        config.validate()?;
        Ok(config)
    }

    /// Auto-detect plugin source: directory or single file
    ///
    /// If path is a directory, loads all YAML files from it.
    /// If path is a file, loads it as a monolithic plugin config.
    pub fn load_auto(path: &str) -> Result<Self> {
        let p = std::path::Path::new(path);
        if p.is_dir() {
            Self::load_dir(path)
        } else if p.is_file() || p.extension().is_some() {
            Self::load(path)
        } else {
            Err(anyhow::anyhow!("Plugin config path not found: {}", path))
        }
    }
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

    // ─── RawPlugin tests ─────────────────────────────────────────────

    #[test]
    fn test_raw_plugin_shell_defaults() {
        let yaml = r#"
name: test
description: A test plugin
version: "1.0.0"
type: shell

command:
  description: Run a test

execution:
  script: echo hello
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        assert_eq!(plugin.name, "test");
        assert_eq!(plugin.command.name, "test"); // inferred from name
        assert_eq!(plugin.execution.command, "sh");
        assert_eq!(plugin.execution.args, vec!["-c", "echo hello"]);
        assert_eq!(plugin.execution.timeout_seconds, 30);
        assert_eq!(plugin.execution.max_output_bytes, 4096);
        assert!(!plugin.output.create_thread);
        assert_eq!(plugin.output.max_inline_length, 2000);
    }

    #[test]
    fn test_raw_plugin_virtual_defaults() {
        let yaml = r#"
name: cancel_thing
description: Cancel an operation
version: "1.0.0"
type: virtual

command:
  description: Cancel it
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        assert!(plugin.is_virtual()); // empty command
        assert_eq!(plugin.execution.timeout_seconds, 5);
        assert_eq!(plugin.execution.max_output_bytes, 0);
        assert!(!plugin.output.create_thread);
    }

    #[test]
    fn test_raw_plugin_docker_defaults() {
        let yaml = r#"
name: transcribe
description: Transcribe videos
version: "1.0.0"
type: docker

command:
  description: Transcribe a video

execution:
  args: ["run", "--rm", "my-image"]
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        assert_eq!(plugin.execution.command, "docker");
        assert_eq!(plugin.execution.timeout_seconds, 600);
        assert_eq!(plugin.execution.max_output_bytes, 10_485_760);
        assert!(plugin.output.create_thread);
        assert_eq!(plugin.output.max_inline_length, 1500);
    }

    #[test]
    fn test_raw_plugin_explicit_overrides() {
        let yaml = r#"
name: custom
description: Custom plugin
version: "1.0.0"
type: shell

command:
  name: my_custom_cmd
  description: A custom command

execution:
  command: python3
  args: ["-c", "print('hi')"]
  timeout_seconds: 120
  max_output_bytes: 8192

output:
  create_thread: true
  max_inline_length: 500
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        // Explicit values override type defaults
        assert_eq!(plugin.command.name, "my_custom_cmd");
        assert_eq!(plugin.execution.command, "python3");
        assert_eq!(plugin.execution.timeout_seconds, 120);
        assert_eq!(plugin.execution.max_output_bytes, 8192);
        assert!(plugin.output.create_thread);
        assert_eq!(plugin.output.max_inline_length, 500);
    }

    #[test]
    fn test_raw_plugin_script_sugar() {
        let yaml = r#"
name: greet
description: Say hello
version: "1.0.0"
type: shell

command:
  description: Greet the user

execution:
  script: echo "Hello, world!"
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        assert_eq!(plugin.execution.command, "sh");
        assert_eq!(plugin.execution.args.len(), 2);
        assert_eq!(plugin.execution.args[0], "-c");
        assert!(plugin.execution.args[1].contains("Hello, world!"));
    }

    #[test]
    fn test_raw_plugin_command_name_inference() {
        let yaml = r#"
name: my_plugin
description: My plugin
version: "1.0.0"
type: shell

command:
  description: Does things
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        assert_eq!(plugin.command.name, "my_plugin");
    }

    #[test]
    fn test_load_dir() {
        // Create a temp directory with plugin files
        let dir = std::env::temp_dir().join("persona_test_plugins_load_dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("alpha.yaml"),
            r#"
name: alpha
description: First plugin
version: "1.0.0"
type: shell
command:
  description: Alpha command
execution:
  script: echo alpha
"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("beta.yaml"),
            r#"
name: beta
description: Second plugin
version: "1.0.0"
type: api
command:
  description: Beta command
execution:
  script: echo beta
"#,
        )
        .unwrap();

        let config = PluginConfig::load_dir(dir.to_str().unwrap()).unwrap();
        assert_eq!(config.plugins.len(), 2);
        assert_eq!(config.plugins[0].name, "alpha"); // sorted alphabetically
        assert_eq!(config.plugins[1].name, "beta");
        assert_eq!(config.plugins[0].execution.timeout_seconds, 30); // shell default
        assert_eq!(config.plugins[1].execution.timeout_seconds, 15); // api default

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_auto_dir() {
        let dir = std::env::temp_dir().join("persona_test_plugins_auto_dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("test.yaml"),
            r#"
name: test
description: Test
version: "1.0.0"
type: shell
command:
  description: Test
execution:
  script: echo test
"#,
        )
        .unwrap();

        let config = PluginConfig::load_auto(dir.to_str().unwrap()).unwrap();
        assert_eq!(config.plugins.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backward_compatible_full_schema() {
        // A fully specified plugin (old format) should also parse as RawPlugin
        let yaml = r#"
name: legacy
description: Legacy plugin
enabled: true
version: "1.0.0"
command:
  name: legacy
  description: Legacy command
  options:
    - name: input
      description: Input value
      type: string
      required: true
execution:
  command: echo
  args: ["hello"]
  timeout_seconds: 60
  max_output_bytes: 8192
security:
  cooldown_seconds: 10
  guild_only: true
output:
  create_thread: false
  max_inline_length: 2000
"#;
        let raw: RawPlugin = serde_yaml::from_str(yaml).unwrap();
        let plugin = raw.resolve();

        assert_eq!(plugin.name, "legacy");
        assert_eq!(plugin.command.name, "legacy");
        assert_eq!(plugin.execution.command, "echo");
        assert_eq!(plugin.execution.timeout_seconds, 60);
        assert_eq!(plugin.execution.max_output_bytes, 8192);
        assert_eq!(plugin.security.cooldown_seconds, 10);
        assert!(plugin.security.guild_only);
        assert!(!plugin.output.create_thread);
        assert_eq!(plugin.output.max_inline_length, 2000);
    }
}
