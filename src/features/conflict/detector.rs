//! # Feature: Conflict Detection
//!
//! Detects heated discussions using keyword analysis, caps detection, and
//! punctuation patterns. Provides confidence scoring for conflict intensity.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.0.0: Initial release with 50+ hostile keywords and pattern detection

use regex::Regex;

/// Hostile keywords that indicate potential conflict
/// These are matched case-insensitively using substring matching
const HOSTILE_KEYWORDS: &[&str] = &[
    // Intelligence insults
    "stupid",
    "idiotic",
    "idiot",
    "moron",
    "dumb",
    "dumbass",
    "braindead",
    "fool",
    "foolish",
    "ignorant",
    "clueless",
    "delusional",
    // Common profanity - F-word variations
    "fuck",
    "fucking",
    "fucked",
    "fucker",
    "fk",
    "fck",
    "fuk",
    "f*ck",
    "shut the fuck up",
    "what the fuck",
    "the fuck",
    // Common profanity - S-word variations
    "shit",
    "shitty",
    "bullshit",
    "bs",
    "piece of shit",
    "full of shit",
    // Common profanity - other
    "asshole",
    "a**hole",
    "bitch",
    "btch",
    "damn",
    "damned",
    "crap",
    "crappy",
    "hell",
    "go to hell",
    // Dismissive commands
    "shut up",
    "stfu",
    "gtfo",
    "shut it",
    "shut your mouth",
    "piss off",
    "screw you",
    "screw off",
    "get lost",
    "buzz off",
    // Extreme hostility
    "kys",
    "kill yourself",
    "kill your",
    // General insults
    "trash",
    "garbage",
    "pathetic",
    "loser",
    "clown",
    "worthless",
    "useless",
    "incompetent",
    "disgrace",
    "embarrassment",
    "scum",
    "joke",
    "waste of time",
    "waste of space",
    // Quality/value criticisms
    "terrible",
    "awful",
    "worst",
    "disgusting",
    // Direct hostility
    "hate you",
    "hate your",
    "despise",
    // Correctness attacks
    "wrong",
    "you're wrong",
    "completely wrong",
    "so wrong",
    // Dismissive responses
    "nobody asked",
    "didn't ask",
    "who asked",
    "who cares",
    "don't care",
    // Slurs (ableist)
    "retard",
    "retarded",
    "r*tard",
    // Context-dependent insults
    "toxic",
    "cancer",
    "cringe",
    "cringey",
    "embarrassing",
    "noob",
    "scrub",
];

/// Detector for identifying heated arguments and conflicts in conversations
#[derive(Clone)]
pub struct ConflictDetector {
    caps_pattern: Regex,
    excessive_punctuation: Regex,
}

impl ConflictDetector {
    pub fn new() -> Self {
        ConflictDetector {
            caps_pattern: Regex::new(r"[A-Z]{5,}").unwrap(),
            excessive_punctuation: Regex::new(r"[!?]{3,}").unwrap(),
        }
    }

    /// Detect if recent messages indicate a heated argument
    ///
    /// Returns (is_conflict, confidence_score, conflict_type)
    pub fn detect_heated_argument(
        &self,
        messages: &[(String, String, String)], // (user_id, content, timestamp)
        time_window_seconds: i64,
    ) -> (bool, f32, String) {
        if messages.is_empty() {
            return (false, 0.0, String::new());
        }

        let mut total_score = 0.0;
        let mut detection_reasons = Vec::new();

        // Check for rapid back-and-forth between users
        if let Some(rapid_score) = self.detect_rapid_exchange(messages, time_window_seconds) {
            total_score += rapid_score;
            if rapid_score > 0.3 {
                detection_reasons.push("rapid_exchange");
            }
        }

        // Analyze content of recent messages for hostile language
        let content_score = self.analyze_message_content(messages);
        total_score += content_score;
        if content_score > 0.4 {
            detection_reasons.push("hostile_language");
        }

        // Check for escalation pattern
        let escalation_score = self.detect_escalation_pattern(messages);
        total_score += escalation_score;
        if escalation_score > 0.3 {
            detection_reasons.push("escalating_tension");
        }

        // Cap score at 1.0 (don't normalize by dividing, let scores stack)
        let confidence = total_score.min(1.0);
        // Lowered threshold from 0.5 to 0.3 to catch single hostile keywords
        let is_conflict = confidence > 0.3;

        let conflict_type = if detection_reasons.is_empty() {
            String::new()
        } else {
            detection_reasons.join(", ")
        };

        (is_conflict, confidence, conflict_type)
    }

    /// Detect rapid message exchanges between users
    fn detect_rapid_exchange(
        &self,
        messages: &[(String, String, String)],
        time_window_seconds: i64,
    ) -> Option<f32> {
        if messages.len() < 2 {
            return None;
        }

        // Count messages by user
        let mut user_messages: std::collections::HashMap<String, Vec<&(String, String, String)>> =
            std::collections::HashMap::new();

        for msg in messages {
            user_messages.entry(msg.0.clone()).or_default().push(msg);
        }

        // Check if 2 users are dominating conversation
        if user_messages.len() == 2 {
            let users: Vec<_> = user_messages.keys().collect();
            let user_a_count = user_messages.get(users[0]).map(|v| v.len()).unwrap_or(0);
            let user_b_count = user_messages.get(users[1]).map(|v| v.len()).unwrap_or(0);

            // Both users have sent multiple messages
            if user_a_count >= 2 && user_b_count >= 2 {
                // Check if messages are rapid (within time window)
                let first_timestamp = messages.first().and_then(|m| m.2.parse::<i64>().ok());
                let last_timestamp = messages.last().and_then(|m| m.2.parse::<i64>().ok());

                if let (Some(first), Some(last)) = (first_timestamp, last_timestamp) {
                    let duration = last - first;
                    if duration <= time_window_seconds {
                        // More rapid = higher score
                        let rapidity_score = 1.0 - (duration as f32 / time_window_seconds as f32);
                        return Some(rapidity_score * 0.6); // Max 0.6 for rapid exchange
                    }
                }
            }
        }

        None
    }

    /// Analyze message content for hostile indicators
    fn analyze_message_content(&self, messages: &[(String, String, String)]) -> f32 {
        let mut total_score = 0.0;
        let message_count = messages.len() as f32;

        for (_user_id, content, _timestamp) in messages {
            total_score += self.get_conflict_score(content);
        }

        // Average score across all messages
        total_score / message_count
    }

    /// Get conflict score for a single message
    pub fn get_conflict_score(&self, content: &str) -> f32 {
        let mut score = 0.0;

        // Check for hostile keywords (0.4 per keyword, max 0.8)
        let lowercase_content = content.to_lowercase();
        let keyword_count = HOSTILE_KEYWORDS
            .iter()
            .filter(|&&keyword| lowercase_content.contains(keyword))
            .count();

        score += (keyword_count as f32 * 0.4).min(0.8);

        // Check for ALL CAPS (0.3 if >20% caps)
        let caps_percentage = self.calculate_caps_percentage(content);
        if caps_percentage > 0.2 {
            score += 0.3;
        }

        // Check for excessive punctuation (0.2)
        if self.excessive_punctuation.is_match(content) {
            score += 0.2;
        }

        // Check for shouting (multiple caps words)
        if self.caps_pattern.find_iter(content).count() >= 2 {
            score += 0.2;
        }

        score.min(1.0)
    }

    /// Calculate percentage of uppercase characters
    fn calculate_caps_percentage(&self, text: &str) -> f32 {
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        if letters.is_empty() {
            return 0.0;
        }

        let caps_count = letters.iter().filter(|c| c.is_uppercase()).count();
        caps_count as f32 / letters.len() as f32
    }

    /// Detect if messages show escalating tension
    fn detect_escalation_pattern(&self, messages: &[(String, String, String)]) -> f32 {
        if messages.len() < 3 {
            return 0.0;
        }

        let mut scores = Vec::new();
        for (_user_id, content, _timestamp) in messages {
            scores.push(self.get_conflict_score(content));
        }

        // Check if scores are generally increasing
        let mut increasing_count = 0;
        for i in 1..scores.len() {
            if scores[i] > scores[i - 1] {
                increasing_count += 1;
            }
        }

        let escalation_ratio = increasing_count as f32 / (scores.len() - 1) as f32;

        // If >50% of messages show increasing hostility
        if escalation_ratio > 0.5 {
            escalation_ratio * 0.7 // Max 0.7 for escalation
        } else {
            0.0
        }
    }

    /// Check if two specific users are in conflict
    pub fn are_users_in_conflict(
        &self,
        user_a: &str,
        user_b: &str,
        messages: &[(String, String, String)],
    ) -> bool {
        // Filter messages to only those from the two users
        let filtered: Vec<_> = messages
            .iter()
            .filter(|(user_id, _, _)| user_id == user_a || user_id == user_b)
            .map(|(u, c, t)| (u.clone(), c.clone(), t.clone()))
            .collect();

        if filtered.len() < 4 {
            return false;
        }

        let (is_conflict, confidence, _) = self.detect_heated_argument(&filtered, 120);
        is_conflict && confidence > 0.6
    }
}

impl Default for ConflictDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_score_hostile_keywords() {
        let detector = ConflictDetector::new();

        let hostile_message = "you're stupid and wrong";
        let score = detector.get_conflict_score(hostile_message);
        assert!(score > 0.5, "Hostile keywords should increase score");

        let calm_message = "I respectfully disagree with your perspective";
        let score = detector.get_conflict_score(calm_message);
        assert!(score < 0.2, "Calm message should have low score");
    }

    #[test]
    fn test_caps_detection() {
        let detector = ConflictDetector::new();

        let caps_message = "YOU ARE COMPLETELY WRONG ABOUT THIS";
        let score = detector.get_conflict_score(caps_message);
        assert!(score > 0.3, "ALL CAPS should increase score");
    }

    #[test]
    fn test_excessive_punctuation() {
        let detector = ConflictDetector::new();

        let punctuation_message = "What are you talking about???!!!";
        let score = detector.get_conflict_score(punctuation_message);
        assert!(
            score >= 0.2,
            "Excessive punctuation should increase score, got: {}",
            score
        );
    }

    #[test]
    fn test_real_hostile_messages() {
        let detector = ConflictDetector::new();

        // Test individual messages from the user's example
        let test_messages = vec![
            ("How STUPID are we now?", "stupid + CAPS"),
            (
                "REALLY STUPID!!!!!!!",
                "stupid + CAPS + excessive punctuation + shouting",
            ),
            ("DUMB", "dumb keyword"),
            ("DUMBASS", "dumbass keyword"),
            ("DUMB DUMB", "dumb keyword (counted once)"),
            ("!!!!!!!!", "excessive punctuation only"),
        ];

        println!("\n=== Individual Message Scores ===");
        for (msg, desc) in &test_messages {
            let score = detector.get_conflict_score(msg);
            println!("Message: '{}' ({})", msg, desc);
            println!("  Score: {:.2} | Triggers (>0.3): {}\n", score, score > 0.3);
        }

        // Test as a conversation
        let conversation: Vec<(String, String, String)> = test_messages
            .iter()
            .enumerate()
            .map(|(i, (msg, _))| {
                (
                    format!("user_{}", i % 2),
                    msg.to_string(),
                    format!("{}", 1000 + i),
                )
            })
            .collect();

        let (is_conflict, confidence, conflict_type) =
            detector.detect_heated_argument(&conversation, 120);

        println!("=== Conversation Analysis ===");
        println!("Is Conflict: {}", is_conflict);
        println!("Confidence: {:.2}", confidence);
        println!("Conflict Type: {:?}", conflict_type);
        println!("Threshold: 0.3");
        println!("Should trigger: {}", confidence > 0.3);

        // This should definitely trigger
        assert!(
            is_conflict,
            "Hostile conversation should trigger conflict detection"
        );
        assert!(
            confidence > 0.3,
            "Confidence should exceed 0.3 threshold, got: {}",
            confidence
        );
    }

    #[test]
    fn test_profanity_detection() {
        let detector = ConflictDetector::new();

        // Test newly added profanity keywords
        let profanity_tests = vec![
            ("fuck you", "f-word"),
            ("this is fucking stupid", "f-word + stupid"),
            ("what the fuck", "f-word phrase"),
            ("this is bullshit", "s-word variation"),
            ("you're full of shit", "s-word phrase"),
            ("you're an asshole", "a-word"),
            ("shut the fuck up", "dismissive profanity"),
            ("nobody asked", "dismissive"),
            ("you're worthless", "general insult"),
            ("get lost loser", "dismissive + insult"),
            ("so cringe", "context-dependent"),
            ("you're retarded", "ableist slur"),
        ];

        println!("\n=== Profanity Detection Tests ===");
        for (msg, category) in &profanity_tests {
            let score = detector.get_conflict_score(msg);
            println!("Message: '{}' ({})", msg, category);
            println!("  Score: {:.2} | Triggers (>0.3): {}\n", score, score > 0.3);

            // All of these should trigger (score > 0.3)
            assert!(
                score > 0.3,
                "Message '{}' ({}) should trigger, got score: {}",
                msg,
                category,
                score
            );
        }
    }
}
