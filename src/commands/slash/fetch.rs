//! # Fetch Command
//!
//! Fetch a webpage and get a persona-flavored summary or Q&A.
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.2.0
//!
//! ## Changelog
//! - 1.0.0: Initial implementation

use serenity::builder::CreateApplicationCommand;
use serenity::model::application::command::CommandOptionType;

pub fn create_commands() -> Vec<CreateApplicationCommand> {
    vec![create_fetch_command()]
}

fn create_fetch_command() -> CreateApplicationCommand {
    let mut command = CreateApplicationCommand::default();
    command
        .name("fetch")
        .description("Fetch a webpage and get a persona-powered summary or answer")
        .create_option(|option| {
            option
                .name("url")
                .description("The URL of the webpage to fetch")
                .kind(CommandOptionType::String)
                .required(true)
                .min_length(1)
                .max_length(2000)
        })
        .create_option(|option| {
            option
                .name("question")
                .description("Ask a specific question about the page content (optional)")
                .kind(CommandOptionType::String)
                .required(false)
                .max_length(2000)
        });
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_fetch_command() {
        let commands = create_commands();
        assert_eq!(commands.len(), 1);

        let fetch = &commands[0];
        let name = fetch.0.get("name").unwrap().as_str().unwrap();
        assert_eq!(name, "fetch");
    }
}
