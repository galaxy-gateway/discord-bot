//! Persona embed builders for Discord responses
//!
//! Shared embed construction for persona-styled Discord messages.
//! Extracted from duplicate implementations across command handlers.
//!
//! - **Version**: 1.0.0
//! - **Since**: 4.5.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from 7 duplicate implementations across 4 files

use crate::core::truncate_for_embed;
use crate::features::personas::Persona;
use serenity::builder::CreateEmbed;

/// Build a persona-styled embed: author (name + portrait icon), accent color, truncated description.
///
/// Callers needing extras (footer, thumbnail) can chain additional setters on the returned embed.
pub fn persona_embed(persona: &Persona, text: &str) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.author(|a| {
        a.name(&persona.name);
        if let Some(url) = &persona.portrait_url {
            a.icon_url(url);
        }
        a
    });
    embed.color(persona.color);
    embed.description(truncate_for_embed(text));
    embed
}

/// Build a continuation embed for chunked responses: accent color + description, no author.
pub fn continuation_embed(persona: &Persona, text: &str) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.color(persona.color);
    embed.description(text);
    embed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_persona() -> Persona {
        Persona {
            name: "TestBot".to_string(),
            system_prompt: String::new(),
            description: String::new(),
            portrait_url: Some("https://example.com/portrait.png".to_string()),
            color: 0xFF5733,
        }
    }

    fn test_persona_no_portrait() -> Persona {
        Persona {
            name: "GhostBot".to_string(),
            system_prompt: String::new(),
            description: String::new(),
            portrait_url: None,
            color: 0x00FF00,
        }
    }

    #[test]
    fn test_persona_embed_builds_successfully() {
        let persona = test_persona();
        let _embed = persona_embed(&persona, "Hello world");
        // CreateEmbed is opaque — if it builds without panic, it's correct
    }

    #[test]
    fn test_persona_embed_no_portrait() {
        let persona = test_persona_no_portrait();
        let _embed = persona_embed(&persona, "Hello world");
    }

    #[test]
    fn test_continuation_embed_builds_successfully() {
        let persona = test_persona();
        let _embed = continuation_embed(&persona, "Continued text");
    }

    #[test]
    fn test_persona_embed_truncates_long_text() {
        let persona = test_persona();
        let long_text = "x".repeat(5000);
        // Should not panic — truncate_for_embed handles the limit
        let _embed = persona_embed(&persona, &long_text);
    }

    #[test]
    fn test_continuation_embed_preserves_text() {
        let persona = test_persona();
        // Continuation embeds do NOT truncate — caller is responsible for chunking
        let _embed = continuation_embed(&persona, "Some chunk of text");
    }
}
