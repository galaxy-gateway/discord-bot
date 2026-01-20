//! # Dynamic Command Registration
//!
//! Generate Discord slash commands from plugin configurations.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.9.0

use crate::features::plugins::config::Plugin;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Generate slash commands from plugin configurations
pub fn create_plugin_commands(plugins: &[Plugin]) -> Vec<CreateApplicationCommand> {
    plugins
        .iter()
        .filter(|p| p.enabled)
        .map(create_command_from_plugin)
        .collect()
}

/// Create a single slash command from a plugin definition
fn create_command_from_plugin(plugin: &Plugin) -> CreateApplicationCommand {
    let mut cmd = CreateApplicationCommand::default();

    cmd.name(&plugin.command.name)
        .description(&plugin.command.description);

    for opt in &plugin.command.options {
        cmd.create_option(|o| {
            o.name(&opt.name)
                .description(&opt.description)
                .kind(parse_option_type(&opt.option_type))
                .required(opt.required);

            // Add choices if defined
            for choice in &opt.choices {
                match opt.option_type.as_str() {
                    "integer" => {
                        if let Ok(val) = choice.value.parse::<i32>() {
                            o.add_int_choice(&choice.name, val);
                        }
                    }
                    "number" => {
                        if let Ok(val) = choice.value.parse::<f64>() {
                            o.add_number_choice(&choice.name, val);
                        }
                    }
                    _ => {
                        o.add_string_choice(&choice.name, &choice.value);
                    }
                }
            }

            o
        });
    }

    cmd
}

/// Parse option type string to Discord CommandOptionType
fn parse_option_type(type_str: &str) -> CommandOptionType {
    match type_str.to_lowercase().as_str() {
        "string" => CommandOptionType::String,
        "integer" => CommandOptionType::Integer,
        "boolean" => CommandOptionType::Boolean,
        "user" => CommandOptionType::User,
        "channel" => CommandOptionType::Channel,
        "role" => CommandOptionType::Role,
        "number" => CommandOptionType::Number,
        "attachment" => CommandOptionType::Attachment,
        _ => CommandOptionType::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::plugins::config::{
        Choice, CommandDefinition, CommandOption, ExecutionConfig, OutputConfig, SecurityConfig,
    };
    use std::collections::HashMap;

    fn create_test_plugin(name: &str, enabled: bool) -> Plugin {
        Plugin {
            name: name.to_string(),
            description: "Test plugin".to_string(),
            enabled,
            version: "1.0.0".to_string(),
            command: CommandDefinition {
                name: name.to_string(),
                description: "Test command".to_string(),
                options: vec![CommandOption {
                    name: "input".to_string(),
                    description: "Input parameter".to_string(),
                    option_type: "string".to_string(),
                    required: true,
                    default: None,
                    validation: None,
                    choices: vec![],
                }],
            },
            execution: ExecutionConfig {
                command: "echo".to_string(),
                args: vec!["${input}".to_string()],
                timeout_seconds: 60,
                working_directory: None,
                max_output_bytes: 1000,
                env: HashMap::new(),
                chunking: None,
            },
            security: SecurityConfig::default(),
            output: OutputConfig::default(),
            playlist: None,
        }
    }

    #[test]
    fn test_create_plugin_commands() {
        let plugins = vec![
            create_test_plugin("enabled_plugin", true),
            create_test_plugin("disabled_plugin", false),
        ];

        let commands = create_plugin_commands(&plugins);

        // Only enabled plugins should generate commands
        assert_eq!(commands.len(), 1);
    }

    #[test]
    fn test_parse_option_types() {
        assert_eq!(parse_option_type("string"), CommandOptionType::String);
        assert_eq!(parse_option_type("STRING"), CommandOptionType::String);
        assert_eq!(parse_option_type("integer"), CommandOptionType::Integer);
        assert_eq!(parse_option_type("boolean"), CommandOptionType::Boolean);
        assert_eq!(parse_option_type("user"), CommandOptionType::User);
        assert_eq!(parse_option_type("channel"), CommandOptionType::Channel);
        assert_eq!(parse_option_type("role"), CommandOptionType::Role);
        assert_eq!(parse_option_type("unknown"), CommandOptionType::String);
    }

    #[test]
    fn test_command_with_choices() {
        let plugin = Plugin {
            name: "test".to_string(),
            description: "Test".to_string(),
            enabled: true,
            version: "1.0.0".to_string(),
            command: CommandDefinition {
                name: "test".to_string(),
                description: "Test command".to_string(),
                options: vec![CommandOption {
                    name: "format".to_string(),
                    description: "Output format".to_string(),
                    option_type: "string".to_string(),
                    required: true,
                    default: None,
                    validation: None,
                    choices: vec![
                        Choice {
                            name: "JSON".to_string(),
                            value: "json".to_string(),
                        },
                        Choice {
                            name: "Text".to_string(),
                            value: "text".to_string(),
                        },
                    ],
                }],
            },
            execution: ExecutionConfig {
                command: "echo".to_string(),
                args: vec![],
                timeout_seconds: 60,
                working_directory: None,
                max_output_bytes: 1000,
                env: HashMap::new(),
                chunking: None,
            },
            security: SecurityConfig::default(),
            output: OutputConfig::default(),
            playlist: None,
        };

        let commands = create_plugin_commands(&[plugin]);
        assert_eq!(commands.len(), 1);
    }
}
