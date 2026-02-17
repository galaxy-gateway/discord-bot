//! Interactive plugin scaffold generator
//!
//! Generates a new plugin YAML file in the plugins/ directory with
//! type-appropriate defaults and validates it parses correctly.
//!
//! Usage: cargo run --features scaffold --bin new-plugin

use dialoguer::{Input, Select};
use persona::features::plugins::config::RawPlugin;
use std::path::Path;

fn main() {
    println!("Plugin Scaffold Generator");
    println!("=========================\n");

    // 1. Plugin name
    let name: String = Input::new()
        .with_prompt("Plugin name (lowercase, underscores allowed, max 32 chars)")
        .validate_with(|input: &String| -> Result<(), String> {
            if input.is_empty() {
                return Err("Name cannot be empty".to_string());
            }
            if input.len() > 32 {
                return Err("Name must be 32 characters or less".to_string());
            }
            if !input.chars().all(|c| c.is_lowercase() || c == '_') {
                return Err("Name must be lowercase letters and underscores only".to_string());
            }
            Ok(())
        })
        .interact_text()
        .expect("Failed to read input");

    // Check if plugin already exists
    let plugin_path = format!("plugins/{}.yaml", name);
    if Path::new(&plugin_path).exists() {
        eprintln!("Error: {} already exists", plugin_path);
        std::process::exit(1);
    }

    // 2. Description
    let description: String = Input::new()
        .with_prompt("Description (max 100 chars)")
        .validate_with(|input: &String| -> Result<(), String> {
            if input.is_empty() {
                return Err("Description cannot be empty".to_string());
            }
            if input.len() > 100 {
                return Err("Description must be 100 characters or less".to_string());
            }
            Ok(())
        })
        .interact_text()
        .expect("Failed to read input");

    // 3. Plugin type
    let types = vec!["shell", "api", "docker", "virtual"];
    let type_descriptions = vec![
        "Shell commands (sh -c), 30s timeout",
        "API calls via shell, 15s timeout",
        "Docker containers, 600s timeout, threaded output",
        "Handled internally by bot, no CLI execution",
    ];

    println!("\nPlugin types:");
    for (i, (t, d)) in types.iter().zip(type_descriptions.iter()).enumerate() {
        println!("  {}: {} - {}", i, t, d);
    }

    let type_index = Select::new()
        .with_prompt("Plugin type")
        .items(&types)
        .default(0)
        .interact()
        .expect("Failed to read input");

    let plugin_type = types[type_index];

    // 4. Generate YAML
    let yaml = generate_yaml(&name, &description, plugin_type);

    // 5. Validate it parses
    match serde_yaml::from_str::<RawPlugin>(&yaml) {
        Ok(raw) => {
            let resolved = raw.resolve();
            println!("\nValidation passed:");
            println!("  Name: {}", resolved.name);
            println!("  Command: /{}", resolved.command.name);
            println!(
                "  Execution: {} (timeout: {}s)",
                if resolved.execution.command.is_empty() {
                    "virtual"
                } else {
                    &resolved.execution.command
                },
                resolved.execution.timeout_seconds
            );
        }
        Err(e) => {
            eprintln!("Generated YAML failed to parse: {}", e);
            eprintln!("YAML content:\n{}", yaml);
            std::process::exit(1);
        }
    }

    // 6. Write file
    if !Path::new("plugins").is_dir() {
        std::fs::create_dir_all("plugins").expect("Failed to create plugins directory");
    }

    std::fs::write(&plugin_path, &yaml).expect("Failed to write plugin file");
    println!("\nCreated: {}", plugin_path);
    println!("Edit the file to customize your plugin, then restart the bot to load it.");
}

fn generate_yaml(name: &str, description: &str, plugin_type: &str) -> String {
    let mut y = String::new();

    // Common header
    y.push_str(&format!("name: {name}\n"));
    y.push_str(&format!("description: {description}\n"));
    y.push_str("version: \"1.0.0\"\n");
    y.push_str(&format!("type: {plugin_type}\n"));

    match plugin_type {
        "shell" => {
            y.push_str("\ncommand:\n");
            y.push_str(&format!("  description: {description}\n"));
            y.push_str("  options:\n");
            y.push_str("    - name: input\n");
            y.push_str("      description: \"Input value\"\n");
            y.push_str("      type: string\n");
            y.push_str("      required: true\n");
            y.push_str("\nexecution:\n");
            y.push_str("  script: |\n");
            y.push_str(&format!("    echo \"## {name}\"\n"));
            y.push_str("    echo \"\"\n");
            y.push_str("    echo \"Input: ${input}\"\n");
            y.push_str("    # TODO: Add your shell command here\n");
            y.push_str("\nsecurity:\n");
            y.push_str("  cooldown_seconds: 5\n");
        }
        "api" => {
            y.push_str("\ncommand:\n");
            y.push_str(&format!("  description: {description}\n"));
            y.push_str("  options:\n");
            y.push_str("    - name: query\n");
            y.push_str("      description: \"Query parameter\"\n");
            y.push_str("      type: string\n");
            y.push_str("      required: true\n");
            y.push_str("\nexecution:\n");
            y.push_str("  script: |\n");
            y.push_str("    RESPONSE=$(curl -s --max-time 10 \"https://api.example.com/${query}\")\n");
            y.push_str(&format!("    echo \"## {name}\"\n"));
            y.push_str("    echo \"\"\n");
            y.push_str("    echo \"$RESPONSE\"\n");
            y.push_str("\nsecurity:\n");
            y.push_str("  cooldown_seconds: 5\n");
        }
        "docker" => {
            y.push_str("\ncommand:\n");
            y.push_str(&format!("  description: {description}\n"));
            y.push_str("  options:\n");
            y.push_str("    - name: input\n");
            y.push_str("      description: \"Input value\"\n");
            y.push_str("      type: string\n");
            y.push_str("      required: true\n");
            y.push_str("\nexecution:\n");
            y.push_str("  args:\n");
            y.push_str("    - run\n");
            y.push_str("    - --rm\n");
            y.push_str("    - \"your-image:latest\"\n");
            y.push_str("    - \"${input}\"\n");
            y.push_str("\noutput:\n");
            y.push_str(&format!("  thread_name_template: \"{name}: ${{input}}\"\n"));
            y.push_str("  post_as_file: true\n");
            y.push_str(&format!(
                "  file_name_template: \"{name}-${{timestamp}}.txt\"\n"
            ));
            y.push_str("\nsecurity:\n");
            y.push_str("  cooldown_seconds: 30\n");
        }
        "virtual" => {
            y.push_str("\ncommand:\n");
            y.push_str(&format!("  description: {description}\n"));
            y.push_str("\nsecurity:\n");
            y.push_str("  cooldown_seconds: 5\n");
        }
        _ => unreachable!(),
    }

    y
}
