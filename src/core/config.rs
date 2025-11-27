use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

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
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            discord_token: env::var("DISCORD_MUPPET_FRIEND")
                .map_err(|_| anyhow::anyhow!("DISCORD_MUPPET_FRIEND environment variable not set"))?,
            openai_api_key: env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY environment variable not set"))?,
            database_path: env::var("DATABASE_PATH").unwrap_or_else(|_| "persona.db".to_string()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            discord_public_key: env::var("DISCORD_PUBLIC_KEY").ok(),
            discord_guild_id: env::var("DISCORD_GUILD_ID").ok(),
            openai_model: env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.1".to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_config_from_env_missing_required() {
        env::remove_var("DISCORD_MUPPET_FRIEND");
        env::remove_var("OPENAI_API_KEY");
        
        let result = Config::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_with_defaults() {
        env::set_var("DISCORD_MUPPET_FRIEND", "test_discord_token");
        env::set_var("OPENAI_API_KEY", "test_openai_key");
        env::remove_var("DATABASE_PATH");
        env::remove_var("LOG_LEVEL");
        
        let config = Config::from_env().unwrap();
        assert_eq!(config.discord_token, "test_discord_token");
        assert_eq!(config.openai_api_key, "test_openai_key");
        assert_eq!(config.database_path, "persona.db");
        assert_eq!(config.log_level, "info");
        
        env::remove_var("DISCORD_MUPPET_FRIEND");
        env::remove_var("OPENAI_API_KEY");
    }
}