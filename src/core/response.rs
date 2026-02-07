//! Response chunking and Discord message utilities
//!
//! - **Version**: 1.0.0
//! - **Since**: 3.38.0
//!
//! ## Changelog
//! - 1.0.0: Extracted from 8 duplicate implementations in command_handler.rs

/// Discord embed description limit
pub const EMBED_LIMIT: usize = 4096;
/// Discord message content limit
pub const MESSAGE_LIMIT: usize = 2000;

/// Chunk text into pieces that fit Discord limits (UTF-8 safe, line-aware)
///
/// This function splits text respecting:
/// - UTF-8 character boundaries (never splits mid-character)
/// - Line boundaries when possible (prefers splitting at newlines)
/// - Falls back to byte-aware character splitting for very long lines
pub fn chunk_text(text: &str, max_size: usize) -> Vec<String> {
    if text.len() <= max_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_with_newline = format!("{line}\n");
        if current.len() + line_with_newline.len() > max_size {
            if !current.is_empty() {
                chunks.push(current.trim_end().to_string());
                current = String::new();
            }
            // Handle lines longer than max_size (byte-aware)
            if line_with_newline.len() > max_size {
                chunks.extend(chunk_long_line(line, max_size));
            } else {
                current = line_with_newline;
            }
        } else {
            current.push_str(&line_with_newline);
        }
    }
    if !current.is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}

/// Split a single long line into chunks respecting UTF-8 boundaries
fn chunk_long_line(line: &str, max_size: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for ch in line.chars() {
        let ch_len = ch.len_utf8();
        if current.len() + ch_len > max_size
            && !current.is_empty() {
                result.push(current);
                current = String::new();
            }
        current.push(ch);
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// Chunk text for embed descriptions (4096 character limit)
pub fn chunk_for_embed(text: &str) -> Vec<String> {
    chunk_text(text, EMBED_LIMIT)
}

/// Chunk text for message content (2000 character limit)
pub fn chunk_for_message(text: &str) -> Vec<String> {
    chunk_text(text, MESSAGE_LIMIT)
}

/// Truncate text to fit embed limit, adding ellipsis if needed
pub fn truncate_for_embed(text: &str) -> String {
    if text.len() <= EMBED_LIMIT {
        text.to_string()
    } else {
        // Find a safe UTF-8 boundary
        let mut end = EMBED_LIMIT - 3; // Room for "..."
        while !text.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &text[..end])
    }
}

/// Truncate text to fit message limit, adding ellipsis if needed
pub fn truncate_for_message(text: &str) -> String {
    if text.len() <= MESSAGE_LIMIT {
        text.to_string()
    } else {
        // Find a safe UTF-8 boundary
        let mut end = MESSAGE_LIMIT - 3; // Room for "..."
        while !text.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &text[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_no_chunk() {
        let result = chunk_text("hello", 100);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn test_chunk_respects_lines() {
        let text = "line1\nline2\nline3";
        let result = chunk_text(text, 12);
        assert!(result.len() >= 2);
        // Each chunk should end with complete lines
        for chunk in &result {
            assert!(!chunk.ends_with('\n'));
        }
    }

    #[test]
    fn test_chunk_handles_long_lines() {
        let long_line = "a".repeat(100);
        let result = chunk_text(&long_line, 30);
        assert!(result.len() >= 3);
        for chunk in &result {
            assert!(chunk.len() <= 30);
        }
    }

    #[test]
    fn test_embed_limit() {
        let result = chunk_for_embed(&"a".repeat(5000));
        assert!(result.len() >= 2);
        assert!(result[0].len() <= EMBED_LIMIT);
    }

    #[test]
    fn test_message_limit() {
        let result = chunk_for_message(&"a".repeat(3000));
        assert!(result.len() >= 2);
        assert!(result[0].len() <= MESSAGE_LIMIT);
    }

    #[test]
    fn test_truncate_for_embed_short() {
        let text = "short text";
        assert_eq!(truncate_for_embed(text), text);
    }

    #[test]
    fn test_truncate_for_embed_long() {
        let text = "a".repeat(5000);
        let result = truncate_for_embed(&text);
        assert!(result.len() <= EMBED_LIMIT);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_utf8_safety() {
        // Test with multi-byte characters
        let text = "Hello 世界! ".repeat(500); // Chinese characters
        let chunks = chunk_for_message(&text);
        for chunk in chunks {
            // Verify each chunk is valid UTF-8 (would panic otherwise)
            assert!(chunk.len() <= MESSAGE_LIMIT);
            // Verify no partial characters
            assert!(chunk.chars().count() > 0);
        }
    }

    #[test]
    fn test_empty_text() {
        let result = chunk_text("", 100);
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn test_exactly_at_limit() {
        let text = "a".repeat(100);
        let result = chunk_text(&text, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 100);
    }
}
