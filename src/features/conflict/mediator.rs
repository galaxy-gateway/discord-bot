//! # Feature: Conflict Mediation
//!
//! Obi-Wan themed interventions for heated conversations. Includes rate limiting
//! per channel to prevent over-intervention (configurable cooldown and hourly limits).
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true
//!
//! ## Changelog
//! - 1.0.0: Initial release with themed responses and channel-based rate limiting

use dashmap::DashMap;
use rand::Rng;
use std::time::{Duration, Instant};

/// Obi-Wan's philosophical responses for conflict mediation
const MEDIATION_RESPONSES: &[&str] = &[
    "From a certain point of view, you're both correct. Perspective is everything, my friends.",
    "Perhaps we might pause and consider that both viewpoints have merit worth examining.",
    "I've found that the wisest path often lies in understanding the other's position first.",
    "It appears this matter requires a touch of patience and diplomacy.",
    "Sometimes the greatest strength is in acknowledging the other's point, however different from our own.",
    "There may be more common ground here than appears at first glance.",
    "Ah, I sense tension in the Force. Perhaps a moment of reflection would serve us all well.",
    "In my experience, the truth is often found somewhere between two opposing views.",
    "The ability to disagree without hostility is the mark of true wisdom.",
    "I'm reminded of a lesson from my master: listening is often more powerful than speaking.",
];

/// Specific responses for different conflict types
const RAPID_EXCHANGE_RESPONSES: &[&str] = &[
    "From what I observe, this conversation has become rather... animated. Perhaps a brief pause would help?",
    "I sense we're moving quickly here. Sometimes haste leads us away from understanding.",
];

const HOSTILE_LANGUAGE_RESPONSES: &[&str] = &[
    "Now, now. I've found that words chosen in anger rarely reflect our true intentions.",
    "Perhaps we could choose our words more carefully? Even in disagreement, respect serves us well.",
];

const ESCALATING_TENSION_RESPONSES: &[&str] = &[
    "I notice tensions rising. This is the time for calm heads and open minds, my friends.",
    "Before this escalates further, might I suggest we all take a step back?",
];

/// Manages conflict mediation interventions with rate limiting
#[derive(Clone)]
pub struct ConflictMediator {
    /// Track last intervention time per channel
    channel_interventions: DashMap<String, Instant>,
    /// Maximum interventions per channel per hour
    max_interventions_per_hour: usize,
    /// Cooldown between mediations in the same channel
    mediation_cooldown: Duration,
    /// Track intervention count per hour per channel
    hourly_counts: DashMap<String, Vec<Instant>>,
}

impl ConflictMediator {
    pub fn new(max_interventions_per_hour: usize, cooldown_minutes: u64) -> Self {
        ConflictMediator {
            channel_interventions: DashMap::new(),
            max_interventions_per_hour,
            mediation_cooldown: Duration::from_secs(cooldown_minutes * 60),
            hourly_counts: DashMap::new(),
        }
    }

    /// Check if mediation is allowed in this channel right now
    pub fn can_intervene(&self, channel_id: &str) -> bool {
        // Check cooldown
        if let Some(last_time) = self.channel_interventions.get(channel_id) {
            if last_time.elapsed() < self.mediation_cooldown {
                return false;
            }
        }

        // Check hourly limit
        let now = Instant::now();
        let one_hour_ago = now - Duration::from_secs(3600);

        let mut count_ref = self.hourly_counts.entry(channel_id.to_string()).or_default();

        // Clean up old entries
        count_ref.retain(|&time| time > one_hour_ago);

        // Check if we're under the hourly limit
        count_ref.len() < self.max_interventions_per_hour
    }

    /// Record an intervention in this channel
    pub fn record_intervention(&self, channel_id: &str) {
        let now = Instant::now();
        self.channel_interventions.insert(channel_id.to_string(), now);

        let mut count_ref = self.hourly_counts.entry(channel_id.to_string()).or_default();
        count_ref.push(now);
    }

    /// Get a mediation response based on conflict type
    pub fn get_mediation_response(&self, conflict_type: &str, _confidence: f32) -> String {
        let mut rng = rand::rng();

        // High confidence conflicts get more direct responses
        let response_pool = if conflict_type.contains("hostile_language") {
            HOSTILE_LANGUAGE_RESPONSES
        } else if conflict_type.contains("rapid_exchange") {
            RAPID_EXCHANGE_RESPONSES
        } else if conflict_type.contains("escalating_tension") {
            ESCALATING_TENSION_RESPONSES
        } else {
            MEDIATION_RESPONSES
        };

        // Pick random response from appropriate pool
        let index = rng.random_range(0..response_pool.len());
        response_pool[index].to_string()
    }

    /// Get statistics about mediation activity
    pub fn get_channel_stats(&self, channel_id: &str) -> MediationStats {
        let one_hour_ago = Instant::now() - Duration::from_secs(3600);

        let interventions_last_hour = self
            .hourly_counts
            .get(channel_id)
            .map(|times| times.iter().filter(|&&t| t > one_hour_ago).count())
            .unwrap_or(0);

        let minutes_since_last = self
            .channel_interventions
            .get(channel_id)
            .map(|time| time.elapsed().as_secs() / 60)
            .unwrap_or(u64::MAX);

        let can_intervene_now = self.can_intervene(channel_id);

        MediationStats {
            interventions_last_hour,
            minutes_since_last_intervention: minutes_since_last,
            can_intervene: can_intervene_now,
            cooldown_minutes_remaining: if can_intervene_now {
                0
            } else {
                let elapsed = self
                    .channel_interventions
                    .get(channel_id)
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::from_secs(0));

                if elapsed < self.mediation_cooldown {
                    (self.mediation_cooldown - elapsed).as_secs() / 60
                } else {
                    0
                }
            },
        }
    }

    /// Reset mediation history for a channel (for testing or admin commands)
    pub fn reset_channel(&self, channel_id: &str) {
        self.channel_interventions.remove(channel_id);
        self.hourly_counts.remove(channel_id);
    }
}

impl Default for ConflictMediator {
    fn default() -> Self {
        Self::new(3, 5) // 3 interventions per hour, 5 minute cooldown
    }
}

/// Statistics about mediation activity in a channel
#[derive(Debug, Clone)]
pub struct MediationStats {
    pub interventions_last_hour: usize,
    pub minutes_since_last_intervention: u64,
    pub can_intervene: bool,
    pub cooldown_minutes_remaining: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_mediation_cooldown() {
        let mediator = ConflictMediator::new(3, 0); // 0 minute cooldown for testing
        let channel = "test_channel";

        assert!(mediator.can_intervene(channel), "Should allow first intervention");

        mediator.record_intervention(channel);

        // With 0 cooldown, should immediately allow again
        assert!(mediator.can_intervene(channel), "Should allow with 0 cooldown");
    }

    #[test]
    fn test_hourly_limit() {
        let mediator = ConflictMediator::new(2, 0); // Max 2 per hour
        let channel = "test_channel";

        mediator.record_intervention(channel);
        mediator.record_intervention(channel);

        assert!(!mediator.can_intervene(channel), "Should block after hitting limit");
    }

    #[test]
    fn test_response_selection() {
        let mediator = ConflictMediator::new(3, 5);

        let response = mediator.get_mediation_response("hostile_language", 0.8);
        assert!(!response.is_empty(), "Should return a response");

        let response2 = mediator.get_mediation_response("rapid_exchange", 0.6);
        assert!(!response2.is_empty(), "Should return a response");
    }

    #[test]
    fn test_channel_stats() {
        let mediator = ConflictMediator::new(3, 5);
        let channel = "test_channel";

        let stats = mediator.get_channel_stats(channel);
        assert_eq!(stats.interventions_last_hour, 0);
        assert!(stats.can_intervene);

        mediator.record_intervention(channel);

        let stats = mediator.get_channel_stats(channel);
        assert_eq!(stats.interventions_last_hour, 1);
    }
}
