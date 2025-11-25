//! # Feature: Configuration System
//!
//! Multi-bot configuration system supporting both legacy single-bot environment variables
//! and new YAML-based multi-bot configuration files.
//!
//! - **Version**: 2.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 2.0.0: Multi-bot support with YAML configuration and BotConfig/MultiConfig structs
//! - 1.0.0: Initial single-bot environment variable configuration

use anyhow::{Context, Result};
use log::info;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

// ============================================================================
// Legacy Single-Bot Configuration (Backward Compatibility)
// ============================================================================

/// Legacy configuration for single-bot mode (backward compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub openai_api_key: String,
    pub database_path: String,
    pub log_level: String,
    pub discord_public_key: Option<String>,
    pub discord_guild_id: Option<String>,
    pub openai_model: String,
    pub conflict_mediation_enabled: bool,
    pub conflict_sensitivity: String,
    pub mediation_cooldown_minutes: u64,
}

impl Config {
    /// Load configuration from environment variables (legacy single-bot mode)
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            discord_token: env::var("DISCORD_SPACE_CADET")
                .map_err(|_| anyhow::anyhow!("DISCORD_SPACE_CADET environment variable not set"))?,
            openai_api_key: env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY environment variable not set"))?,
            database_path: env::var("DATABASE_PATH").unwrap_or_else(|_| "persona.db".to_string()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            discord_public_key: env::var("DISCORD_PUBLIC_KEY").ok(),
            discord_guild_id: env::var("DISCORD_GUILD_ID").ok(),
            openai_model: env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            conflict_mediation_enabled: env::var("CONFLICT_MEDIATION_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                .to_lowercase() == "true",
            conflict_sensitivity: env::var("CONFLICT_SENSITIVITY")
                .unwrap_or_else(|_| "medium".to_string()),
            mediation_cooldown_minutes: env::var("MEDIATION_COOLDOWN_MINUTES")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
        })
    }
}

// ============================================================================
// Multi-Bot Configuration System
// ============================================================================

/// Per-bot configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Discord application ID (used as bot_id in database)
    /// This is fetched from Discord API on startup, not configured manually
    #[serde(skip_deserializing, default)]
    pub application_id: Option<String>,

    /// Friendly name for logging and identification
    pub name: String,

    /// Discord bot token (required)
    pub discord_token: String,

    /// Discord public key for interaction verification (HTTP mode only)
    #[serde(default)]
    pub discord_public_key: Option<String>,

    /// Default persona for this bot (overrides global default)
    #[serde(default)]
    pub default_persona: Option<String>,

    /// Dev mode guild ID for this bot (for faster command registration)
    #[serde(default)]
    pub discord_guild_id: Option<String>,

    /// Per-bot OpenAI model override
    #[serde(default)]
    pub openai_model: Option<String>,

    /// Per-bot conflict mediation enabled override
    #[serde(default)]
    pub conflict_mediation_enabled: Option<bool>,

    /// Per-bot conflict sensitivity override
    #[serde(default)]
    pub conflict_sensitivity: Option<String>,

    /// Per-bot mediation cooldown override (minutes)
    #[serde(default)]
    pub mediation_cooldown_minutes: Option<u64>,
}

impl BotConfig {
    /// Get the bot_id (application_id or fallback to name)
    pub fn bot_id(&self) -> &str {
        self.application_id.as_deref().unwrap_or(&self.name)
    }

    /// Get the effective OpenAI model (per-bot or global default)
    pub fn effective_model(&self, global_default: &str) -> String {
        self.openai_model.clone().unwrap_or_else(|| global_default.to_string())
    }

    /// Get effective conflict mediation enabled setting
    pub fn effective_conflict_enabled(&self, global_default: bool) -> bool {
        self.conflict_mediation_enabled.unwrap_or(global_default)
    }

    /// Get effective conflict sensitivity setting
    pub fn effective_conflict_sensitivity(&self, global_default: &str) -> String {
        self.conflict_sensitivity.clone().unwrap_or_else(|| global_default.to_string())
    }

    /// Get effective mediation cooldown (minutes)
    pub fn effective_mediation_cooldown(&self, global_default: u64) -> u64 {
        self.mediation_cooldown_minutes.unwrap_or(global_default)
    }
}

/// Multi-bot configuration with shared and per-bot settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiConfig {
    /// List of bot configurations
    pub bots: Vec<BotConfig>,

    /// Shared OpenAI API key (used by all bots)
    pub openai_api_key: String,

    /// Shared database path (all bots share the database, isolated by bot_id)
    #[serde(default = "default_database_path")]
    pub database_path: String,

    /// Logging level
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Default OpenAI model (can be overridden per-bot)
    #[serde(default = "default_openai_model")]
    pub openai_model: String,

    /// Default conflict mediation enabled (can be overridden per-bot)
    #[serde(default = "default_conflict_enabled")]
    pub conflict_mediation_enabled: bool,

    /// Default conflict sensitivity (can be overridden per-bot)
    #[serde(default = "default_conflict_sensitivity")]
    pub conflict_sensitivity: String,

    /// Default mediation cooldown in minutes (can be overridden per-bot)
    #[serde(default = "default_mediation_cooldown")]
    pub mediation_cooldown_minutes: u64,
}

// Default value functions for serde
fn default_database_path() -> String {
    "persona.db".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_openai_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_conflict_enabled() -> bool {
    true
}

fn default_conflict_sensitivity() -> String {
    "medium".to_string()
}

fn default_mediation_cooldown() -> u64 {
    5
}

impl MultiConfig {
    /// Load configuration from a YAML file with environment variable interpolation
    ///
    /// Supports `${VAR_NAME}` syntax for environment variable substitution.
    /// Example: `discord_token: "${DISCORD_BOT_TOKEN}"`
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        // Interpolate environment variables
        let interpolated = interpolate_env_vars(&content)?;

        // Parse YAML
        let config: MultiConfig = serde_yaml::from_str(&interpolated)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        // Validate configuration
        config.validate()?;

        info!(
            "Loaded multi-bot config from {} with {} bot(s)",
            path.display(),
            config.bots.len()
        );

        Ok(config)
    }

    /// Create MultiConfig from legacy environment variables (single bot mode)
    ///
    /// This provides backward compatibility with the existing DISCORD_SPACE_CADET
    /// environment variable approach.
    pub fn from_env_single_bot() -> Result<Self> {
        let legacy_config = Config::from_env()?;

        let bot = BotConfig {
            application_id: None, // Will be fetched from Discord API
            name: "default".to_string(),
            discord_token: legacy_config.discord_token,
            discord_public_key: legacy_config.discord_public_key,
            default_persona: None, // Use global default
            discord_guild_id: legacy_config.discord_guild_id,
            openai_model: None,    // Use global default
            conflict_mediation_enabled: None,
            conflict_sensitivity: None,
            mediation_cooldown_minutes: None,
        };

        Ok(MultiConfig {
            bots: vec![bot],
            openai_api_key: legacy_config.openai_api_key,
            database_path: legacy_config.database_path,
            log_level: legacy_config.log_level,
            openai_model: legacy_config.openai_model,
            conflict_mediation_enabled: legacy_config.conflict_mediation_enabled,
            conflict_sensitivity: legacy_config.conflict_sensitivity,
            mediation_cooldown_minutes: legacy_config.mediation_cooldown_minutes,
        })
    }

    /// Auto-detect and load configuration
    ///
    /// Priority order:
    /// 1. If CONFIG_FILE env var is set, load from that file
    /// 2. If config.yaml exists in current directory, load from it
    /// 3. Fall back to legacy environment variables
    pub fn auto_load() -> Result<Self> {
        // Check for explicit config file path
        if let Ok(config_path) = env::var("CONFIG_FILE") {
            info!("Loading config from CONFIG_FILE: {}", config_path);
            return Self::from_file(&config_path);
        }

        // Check for config.yaml in current directory
        let default_config_path = "config.yaml";
        if Path::new(default_config_path).exists() {
            info!("Loading config from {}", default_config_path);
            return Self::from_file(default_config_path);
        }

        // Fall back to legacy environment variables
        info!("No config file found, using legacy environment variables");
        Self::from_env_single_bot()
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.bots.is_empty() {
            anyhow::bail!("At least one bot configuration is required");
        }

        if self.openai_api_key.is_empty() {
            anyhow::bail!("openai_api_key is required");
        }

        for (i, bot) in self.bots.iter().enumerate() {
            if bot.name.is_empty() {
                anyhow::bail!("Bot {} has empty name", i);
            }
            if bot.discord_token.is_empty() {
                anyhow::bail!("Bot '{}' has empty discord_token", bot.name);
            }

            // Validate conflict sensitivity if provided
            if let Some(ref sensitivity) = bot.conflict_sensitivity {
                if !["low", "medium", "high"].contains(&sensitivity.as_str()) {
                    anyhow::bail!(
                        "Bot '{}' has invalid conflict_sensitivity '{}'. Use: low, medium, high",
                        bot.name,
                        sensitivity
                    );
                }
            }
        }

        // Validate global conflict sensitivity
        if !["low", "medium", "high"].contains(&self.conflict_sensitivity.as_str()) {
            anyhow::bail!(
                "Invalid global conflict_sensitivity '{}'. Use: low, medium, high",
                self.conflict_sensitivity
            );
        }

        Ok(())
    }

    /// Get a bot configuration by name
    pub fn get_bot(&self, name: &str) -> Option<&BotConfig> {
        self.bots.iter().find(|b| b.name == name)
    }

    /// Get a mutable bot configuration by name
    pub fn get_bot_mut(&mut self, name: &str) -> Option<&mut BotConfig> {
        self.bots.iter_mut().find(|b| b.name == name)
    }

    /// Convert to a legacy Config for a specific bot (for backward compatibility)
    pub fn to_legacy_config(&self, bot: &BotConfig) -> Config {
        Config {
            discord_token: bot.discord_token.clone(),
            openai_api_key: self.openai_api_key.clone(),
            database_path: self.database_path.clone(),
            log_level: self.log_level.clone(),
            discord_public_key: bot.discord_public_key.clone(),
            discord_guild_id: bot.discord_guild_id.clone(),
            openai_model: bot.effective_model(&self.openai_model),
            conflict_mediation_enabled: bot.effective_conflict_enabled(self.conflict_mediation_enabled),
            conflict_sensitivity: bot.effective_conflict_sensitivity(&self.conflict_sensitivity),
            mediation_cooldown_minutes: bot.effective_mediation_cooldown(self.mediation_cooldown_minutes),
        }
    }
}

// ============================================================================
// Environment Variable Interpolation
// ============================================================================

/// Interpolate environment variables in a string
///
/// Supports `${VAR_NAME}` syntax. If a variable is not set, returns an error.
/// Use `${VAR_NAME:-default}` for default values.
fn interpolate_env_vars(content: &str) -> Result<String> {
    // Pattern: ${VAR_NAME} or ${VAR_NAME:-default}
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}")
        .expect("Invalid regex");

    let mut result = content.to_string();
    let mut errors = Vec::new();

    for cap in re.captures_iter(content) {
        let full_match = cap.get(0).unwrap().as_str();
        let var_name = &cap[1];
        let default_value = cap.get(2).map(|m| m.as_str());

        let value = match env::var(var_name) {
            Ok(v) => v,
            Err(_) => {
                if let Some(default) = default_value {
                    default.to_string()
                } else {
                    errors.push(format!("Environment variable '{}' is not set", var_name));
                    continue;
                }
            }
        };

        result = result.replace(full_match, &value);
    }

    if !errors.is_empty() {
        anyhow::bail!("Missing environment variables:\n  - {}", errors.join("\n  - "));
    }

    Ok(result)
}

// ============================================================================
// Discord API - Application ID Fetching
// ============================================================================

/// Fetch the application ID from Discord API using the bot token
///
/// This makes a request to GET /users/@me to get the bot's user info,
/// then extracts the application ID from the response.
pub async fn fetch_application_id(token: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let response = client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await
        .context("Failed to connect to Discord API")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Discord API error ({}): {}", status, body);
    }

    let user_info: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Discord API response")?;

    // The user ID is the same as the application ID for bot users
    let id = user_info["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Discord API response missing 'id' field"))?;

    Ok(id.to_string())
}

/// Fetch and set application IDs for all bots in the config
pub async fn populate_application_ids(config: &mut MultiConfig) -> Result<()> {
    for bot in &mut config.bots {
        match fetch_application_id(&bot.discord_token).await {
            Ok(app_id) => {
                info!("Bot '{}' has application ID: {}", bot.name, app_id);
                bot.application_id = Some(app_id);
            }
            Err(e) => {
                anyhow::bail!("Failed to fetch application ID for bot '{}': {}", bot.name, e);
            }
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_config_from_env_missing_required() {
        env::remove_var("DISCORD_SPACE_CADET");
        env::remove_var("OPENAI_API_KEY");

        let result = Config::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_with_defaults() {
        env::set_var("DISCORD_SPACE_CADET", "test_discord_token");
        env::set_var("OPENAI_API_KEY", "test_openai_key");
        env::remove_var("DATABASE_PATH");
        env::remove_var("LOG_LEVEL");

        let config = Config::from_env().unwrap();
        assert_eq!(config.discord_token, "test_discord_token");
        assert_eq!(config.openai_api_key, "test_openai_key");
        assert_eq!(config.database_path, "persona.db");
        assert_eq!(config.log_level, "info");

        env::remove_var("DISCORD_SPACE_CADET");
        env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_interpolate_env_vars_simple() {
        env::set_var("TEST_VAR_1", "value1");
        let input = "key: ${TEST_VAR_1}";
        let result = interpolate_env_vars(input).unwrap();
        assert_eq!(result, "key: value1");
        env::remove_var("TEST_VAR_1");
    }

    #[test]
    fn test_interpolate_env_vars_with_default() {
        env::remove_var("NONEXISTENT_VAR");
        let input = "key: ${NONEXISTENT_VAR:-default_value}";
        let result = interpolate_env_vars(input).unwrap();
        assert_eq!(result, "key: default_value");
    }

    #[test]
    fn test_interpolate_env_vars_missing_no_default() {
        env::remove_var("MISSING_VAR_FOR_TEST");
        let input = "key: ${MISSING_VAR_FOR_TEST}";
        let result = interpolate_env_vars(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_multi_config_from_yaml() {
        env::set_var("TEST_DISCORD_TOKEN", "test_token_123");
        env::set_var("TEST_OPENAI_KEY", "sk-test-key");

        let yaml = r#"
bots:
  - name: "test_bot"
    discord_token: "${TEST_DISCORD_TOKEN}"
    default_persona: "obi"

openai_api_key: "${TEST_OPENAI_KEY}"
database_path: "test.db"
log_level: "debug"
openai_model: "gpt-4o"
conflict_mediation_enabled: true
conflict_sensitivity: "medium"
mediation_cooldown_minutes: 10
"#;

        let interpolated = interpolate_env_vars(yaml).unwrap();
        let config: MultiConfig = serde_yaml::from_str(&interpolated).unwrap();

        assert_eq!(config.bots.len(), 1);
        assert_eq!(config.bots[0].name, "test_bot");
        assert_eq!(config.bots[0].discord_token, "test_token_123");
        assert_eq!(config.bots[0].default_persona, Some("obi".to_string()));
        assert_eq!(config.openai_api_key, "sk-test-key");
        assert_eq!(config.database_path, "test.db");
        assert_eq!(config.openai_model, "gpt-4o");

        env::remove_var("TEST_DISCORD_TOKEN");
        env::remove_var("TEST_OPENAI_KEY");
    }

    #[test]
    fn test_bot_config_effective_values() {
        let bot = BotConfig {
            application_id: Some("123456789".to_string()),
            name: "test".to_string(),
            discord_token: "token".to_string(),
            discord_public_key: None,
            default_persona: None,
            discord_guild_id: None,
            openai_model: Some("gpt-4".to_string()),
            conflict_mediation_enabled: Some(false),
            conflict_sensitivity: Some("high".to_string()),
            mediation_cooldown_minutes: Some(15),
        };

        assert_eq!(bot.bot_id(), "123456789");
        assert_eq!(bot.effective_model("gpt-3.5-turbo"), "gpt-4");
        assert!(!bot.effective_conflict_enabled(true));
        assert_eq!(bot.effective_conflict_sensitivity("low"), "high");
        assert_eq!(bot.effective_mediation_cooldown(5), 15);
    }

    #[test]
    fn test_bot_config_fallback_to_global() {
        let bot = BotConfig {
            application_id: None,
            name: "test".to_string(),
            discord_token: "token".to_string(),
            discord_public_key: None,
            default_persona: None,
            discord_guild_id: None,
            openai_model: None,
            conflict_mediation_enabled: None,
            conflict_sensitivity: None,
            mediation_cooldown_minutes: None,
        };

        // Should fall back to name when no application_id
        assert_eq!(bot.bot_id(), "test");
        // Should fall back to global defaults
        assert_eq!(bot.effective_model("gpt-3.5-turbo"), "gpt-3.5-turbo");
        assert!(bot.effective_conflict_enabled(true));
        assert_eq!(bot.effective_conflict_sensitivity("low"), "low");
        assert_eq!(bot.effective_mediation_cooldown(5), 5);
    }

    #[test]
    fn test_multi_config_validation_empty_bots() {
        let config = MultiConfig {
            bots: vec![],
            openai_api_key: "key".to_string(),
            database_path: "db".to_string(),
            log_level: "info".to_string(),
            openai_model: "gpt-4".to_string(),
            conflict_mediation_enabled: true,
            conflict_sensitivity: "medium".to_string(),
            mediation_cooldown_minutes: 5,
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("At least one bot"));
    }

    #[test]
    fn test_multi_config_validation_invalid_sensitivity() {
        let config = MultiConfig {
            bots: vec![BotConfig {
                application_id: None,
                name: "test".to_string(),
                discord_token: "token".to_string(),
                discord_public_key: None,
                default_persona: None,
                discord_guild_id: None,
                openai_model: None,
                conflict_mediation_enabled: None,
                conflict_sensitivity: Some("invalid".to_string()),
                mediation_cooldown_minutes: None,
            }],
            openai_api_key: "key".to_string(),
            database_path: "db".to_string(),
            log_level: "info".to_string(),
            openai_model: "gpt-4".to_string(),
            conflict_mediation_enabled: true,
            conflict_sensitivity: "medium".to_string(),
            mediation_cooldown_minutes: 5,
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid conflict_sensitivity"));
    }

    #[test]
    fn test_to_legacy_config() {
        let bot = BotConfig {
            application_id: Some("123".to_string()),
            name: "test".to_string(),
            discord_token: "bot_token".to_string(),
            discord_public_key: Some("pub_key".to_string()),
            default_persona: Some("chef".to_string()),
            discord_guild_id: Some("guild_123".to_string()),
            openai_model: Some("gpt-4".to_string()),
            conflict_mediation_enabled: Some(false),
            conflict_sensitivity: Some("high".to_string()),
            mediation_cooldown_minutes: Some(10),
        };

        let multi = MultiConfig {
            bots: vec![bot.clone()],
            openai_api_key: "openai_key".to_string(),
            database_path: "test.db".to_string(),
            log_level: "debug".to_string(),
            openai_model: "gpt-3.5-turbo".to_string(),
            conflict_mediation_enabled: true,
            conflict_sensitivity: "low".to_string(),
            mediation_cooldown_minutes: 5,
        };

        let legacy = multi.to_legacy_config(&bot);

        assert_eq!(legacy.discord_token, "bot_token");
        assert_eq!(legacy.openai_api_key, "openai_key");
        assert_eq!(legacy.database_path, "test.db");
        assert_eq!(legacy.discord_public_key, Some("pub_key".to_string()));
        assert_eq!(legacy.discord_guild_id, Some("guild_123".to_string()));
        // Per-bot overrides should be used
        assert_eq!(legacy.openai_model, "gpt-4");
        assert!(!legacy.conflict_mediation_enabled);
        assert_eq!(legacy.conflict_sensitivity, "high");
        assert_eq!(legacy.mediation_cooldown_minutes, 10);
    }
}
