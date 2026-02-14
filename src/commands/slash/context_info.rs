//! # Context Info Command
//!
//! Shows users their current context window stats.
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.4.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use serenity::builder::CreateApplicationCommand;

pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_context_command()]
}

fn create_context_command() -> CreateApplicationCommand {
    let mut command = CreateApplicationCommand::default();
    command
        .name("context")
        .description("Show your current context window — message count, token estimate, and active persona");
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_context_command() {
        let commands = create_commands();
        assert_eq!(commands.len(), 1);

        let context = &commands[0];
        let name = context.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "context");
    }
}
