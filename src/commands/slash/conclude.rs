//! # Conclude Command
//!
//! End the current council or debate session in a thread.
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.33.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use serenity::builder::CreateApplicationCommand;

pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_conclude_command()]
}

fn create_conclude_command() -> CreateApplicationCommand {
    let mut command = CreateApplicationCommand::default();
    command
        .name("conclude")
        .description("End the current council or debate session in this thread");
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_conclude_command() {
        let commands = create_commands();
        assert_eq!(commands.len(), 1);

        let conclude = &commands[0];
        let name = conclude.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "conclude");
    }
}
