//! # Dynamic Command Registration
//!
//! Generate a single `/plugins` Discord slash command with subcommands from plugin configurations.
//!
//! - **Version**: 2.0.0
//! - **Since**: 0.9.0
//!
//! ## Changelog
//! - 2.0.0: Breaking change - consolidate all plugins under single /plugins command with subcommands
//! - 1.0.0: Initial release with per-plugin top-level commands

use crate::features::plugins::config::Plugin;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

/// Generate a single `/plugins` slash command with subcommands from plugin configurations
pub fn create_plugins_command(plugins: &[Plugin]) -> CreateApplicationCommand {
    let mut cmd = CreateApplicationCommand::default();

    cmd.name("plugins")
        .description("Run plugin commands (transcribe, weather, dns, etc.)");

    for plugin in plugins.iter().filter(|p| p.enabled) {
        cmd.create_option(|subcommand| {
            subcommand
                .name(&plugin.command.name)
                .description(&plugin.command.description)
                .kind(CommandOptionType::SubCommand);

            for opt in &plugin.command.options {
                subcommand.create_sub_option(|o| {
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

            subcommand
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
    fn test_create_plugins_command_returns_single_command() {
        let plugins = vec![
            create_test_plugin("enabled_plugin", true),
            create_test_plugin("disabled_plugin", false),
        ];

        let cmd = create_plugins_command(&plugins);

        // Should be a single command named "plugins"
        let name = cmd.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "plugins");

        // Should have only 1 subcommand (the enabled one)
        let options = cmd.0.get("options").unwrap().as_array().unwrap();
        assert_eq!(options.len(), 1);

        // Verify the subcommand name
        let subcommand = &options[0];
        assert_eq!(
            subcommand.get("name").unwrap().as_str().unwrap(),
            "enabled_plugin"
        );
        // Verify it's a SubCommand type (type 1)
        assert_eq!(subcommand.get("type").unwrap().as_u64().unwrap(), 1);
    }

    #[test]
    fn test_create_plugins_command_no_plugins() {
        let cmd = create_plugins_command(&[]);

        let name = cmd.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "plugins");

        // No subcommands when no plugins
        let options = cmd.0.get("options");
        assert!(
            options.is_none() || options.unwrap().as_array().unwrap().is_empty()
        );
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

        let cmd = create_plugins_command(&[plugin]);

        // Should have 1 subcommand
        let options = cmd.0.get("options").unwrap().as_array().unwrap();
        assert_eq!(options.len(), 1);

        // The subcommand should have 1 option with choices
        let subcommand_options = options[0].get("options").unwrap().as_array().unwrap();
        assert_eq!(subcommand_options.len(), 1);
        let choices = subcommand_options[0]
            .get("choices")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(choices.len(), 2);
    }

    #[test]
    fn test_subcommand_has_correct_parameters() {
        let plugins = vec![create_test_plugin("my_plugin", true)];
        let cmd = create_plugins_command(&plugins);

        let options = cmd.0.get("options").unwrap().as_array().unwrap();
        let subcommand = &options[0];

        // Verify subcommand options contain the plugin's parameter
        let sub_options = subcommand.get("options").unwrap().as_array().unwrap();
        assert_eq!(sub_options.len(), 1);
        assert_eq!(
            sub_options[0].get("name").unwrap().as_str().unwrap(),
            "input"
        );
        assert_eq!(
            sub_options[0].get("required").unwrap().as_bool().unwrap(),
            true
        );
    }
}
