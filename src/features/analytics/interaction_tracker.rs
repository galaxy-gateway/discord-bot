//! # Feature: DM Interaction Tracking
//!
//! Tracks DM sessions, engagement metrics, and feature usage with event-driven architecture.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.6.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial release with async event-driven tracking

use crate::database::Database;
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use log::{debug, error, warn};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Types of API calls tracked
#[derive(Debug, Clone)]
pub enum ApiType {
    Chat,
    Whisper,
    DallE,
}

/// Types of features used in DMs
#[derive(Debug, Clone)]
pub enum FeatureType {
    AudioTranscription,
    SlashCommand,
}

/// Reasons for session ending
#[derive(Debug, Clone)]
pub enum SessionEndReason {
    InactivityTimeout,
    UserLeft,
    BotRestart,
}

impl SessionEndReason {
    pub fn as_str(&self) -> &str {
        match self {
            SessionEndReason::InactivityTimeout => "timeout",
            SessionEndReason::UserLeft => "user_left",
            SessionEndReason::BotRestart => "bot_restart",
        }
    }
}

/// DM interaction tracking events
#[derive(Debug, Clone)]
pub enum TrackingEvent {
    /// Session started
    SessionStart {
        session_id: String,
        user_id: String,
        channel_id: String,
    },
    /// Session ended
    SessionEnd {
        session_id: String,
        reason: SessionEndReason,
    },
    /// User message received
    MessageReceived {
        session_id: String,
        user_id: String,
        channel_id: String,
        message_id: String,
        character_count: usize,
        has_attachments: bool,
    },
    /// Bot message sent
    MessageSent {
        session_id: String,
        user_id: String,
        channel_id: String,
        message_id: String,
        character_count: usize,
        response_time_ms: u64,
    },
    /// API call made
    ApiCall {
        session_id: String,
        user_id: String,
        api_type: ApiType,
        tokens: Option<u32>,
        cost: f64,
    },
    /// Feature used in DM
    FeatureUsed {
        session_id: String,
        user_id: String,
        feature: FeatureType,
        feature_detail: String,
    },
}

/// Active session state tracked in memory
#[derive(Debug, Clone)]
struct SessionState {
    session_id: String,
    user_id: String,
    channel_id: String,
    _started_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    message_count: i32,
    user_message_count: i32,
    bot_message_count: i32,
    total_user_chars: i32,
    total_bot_chars: i32,
    response_times: Vec<u64>,
}

impl SessionState {
    fn new(session_id: String, user_id: String, channel_id: String) -> Self {
        let now = Utc::now();
        SessionState {
            session_id,
            user_id,
            channel_id,
            _started_at: now,
            last_activity: now,
            message_count: 0,
            user_message_count: 0,
            bot_message_count: 0,
            total_user_chars: 0,
            total_bot_chars: 0,
            response_times: Vec::new(),
        }
    }

    fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }

    fn add_user_message(&mut self, chars: usize) {
        self.message_count += 1;
        self.user_message_count += 1;
        self.total_user_chars += chars as i32;
        self.update_activity();
    }

    fn add_bot_message(&mut self, chars: usize, response_time_ms: u64) {
        self.message_count += 1;
        self.bot_message_count += 1;
        self.total_bot_chars += chars as i32;
        self.response_times.push(response_time_ms);
        self.update_activity();
    }

    fn avg_response_time(&self) -> i32 {
        if self.response_times.is_empty() {
            return 0;
        }
        let sum: u64 = self.response_times.iter().sum();
        (sum / self.response_times.len() as u64) as i32
    }

    fn is_timed_out(&self, timeout_minutes: i64) -> bool {
        let timeout_duration = Duration::minutes(timeout_minutes);
        Utc::now() - self.last_activity > timeout_duration
    }
}

/// Handles async tracking of DM interactions without blocking responses
#[derive(Clone)]
pub struct InteractionTracker {
    sender: mpsc::UnboundedSender<TrackingEvent>,
    active_sessions: Arc<DashMap<String, SessionState>>,
}

impl InteractionTracker {
    /// Create a new InteractionTracker with background processing task
    pub fn new(database: Database) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let active_sessions = Arc::new(DashMap::new());

        // Spawn background event processor
        tokio::spawn(Self::event_processor(
            database.clone(),
            receiver,
            active_sessions.clone(),
        ));

        // Spawn session timeout cleanup task
        tokio::spawn(Self::cleanup_task(
            database,
            active_sessions.clone(),
            sender.clone(),
        ));

        InteractionTracker {
            sender,
            active_sessions,
        }
    }

    /// Get or create a session for a DM channel
    pub fn get_or_create_session(&self, user_id: &str, channel_id: &str) -> String {
        let key = format!("{}:{}", user_id, channel_id);

        // Check if active session exists
        if let Some(session) = self.active_sessions.get(&key) {
            if !session.is_timed_out(30) {
                return session.session_id.clone();
            }
        }

        // Create new session
        let session_id = Uuid::new_v4().to_string();
        let session = SessionState::new(session_id.clone(), user_id.to_string(), channel_id.to_string());
        self.active_sessions.insert(key, session);

        // Emit session start event
        self.track_session_start(&session_id, user_id, channel_id);

        session_id
    }

    /// Track session start (non-blocking)
    pub fn track_session_start(&self, session_id: &str, user_id: &str, channel_id: &str) {
        let event = TrackingEvent::SessionStart {
            session_id: session_id.to_string(),
            user_id: user_id.to_string(),
            channel_id: channel_id.to_string(),
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue session start event: {e}");
        }
    }

    /// Track session end (non-blocking)
    pub fn track_session_end(&self, session_id: &str, reason: SessionEndReason) {
        let event = TrackingEvent::SessionEnd {
            session_id: session_id.to_string(),
            reason,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue session end event: {e}");
        }
    }

    /// Track message received (non-blocking)
    pub fn track_message_received(
        &self,
        session_id: &str,
        user_id: &str,
        channel_id: &str,
        message_id: &str,
        character_count: usize,
        has_attachments: bool,
    ) {
        let event = TrackingEvent::MessageReceived {
            session_id: session_id.to_string(),
            user_id: user_id.to_string(),
            channel_id: channel_id.to_string(),
            message_id: message_id.to_string(),
            character_count,
            has_attachments,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue message received event: {e}");
        }
    }

    /// Track message sent (non-blocking)
    pub fn track_message_sent(
        &self,
        session_id: &str,
        user_id: &str,
        channel_id: &str,
        message_id: &str,
        character_count: usize,
        response_time_ms: u64,
    ) {
        let event = TrackingEvent::MessageSent {
            session_id: session_id.to_string(),
            user_id: user_id.to_string(),
            channel_id: channel_id.to_string(),
            message_id: message_id.to_string(),
            character_count,
            response_time_ms,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue message sent event: {e}");
        }
    }

    /// Track API call (non-blocking)
    pub fn track_api_call(
        &self,
        session_id: &str,
        user_id: &str,
        api_type: ApiType,
        tokens: Option<u32>,
        cost: f64,
    ) {
        let event = TrackingEvent::ApiCall {
            session_id: session_id.to_string(),
            user_id: user_id.to_string(),
            api_type,
            tokens,
            cost,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue API call event: {e}");
        }
    }

    /// Track feature usage (non-blocking)
    pub fn track_feature_used(
        &self,
        session_id: &str,
        user_id: &str,
        feature: FeatureType,
        feature_detail: String,
    ) {
        let event = TrackingEvent::FeatureUsed {
            session_id: session_id.to_string(),
            user_id: user_id.to_string(),
            feature,
            feature_detail,
        };

        if let Err(e) = self.sender.send(event) {
            warn!("Failed to queue feature usage event: {e}");
        }
    }

    /// Background task that processes tracking events
    async fn event_processor(
        database: Database,
        mut receiver: mpsc::UnboundedReceiver<TrackingEvent>,
        active_sessions: Arc<DashMap<String, SessionState>>,
    ) {
        debug!("InteractionTracker event processor started");

        while let Some(event) = receiver.recv().await {
            if let Err(e) = Self::process_event(&database, &active_sessions, event).await {
                error!("Failed to process tracking event: {e}");
            }
        }

        debug!("InteractionTracker event processor stopped");
    }

    /// Process a single tracking event
    async fn process_event(
        database: &Database,
        active_sessions: &DashMap<String, SessionState>,
        event: TrackingEvent,
    ) -> anyhow::Result<()> {
        match event {
            TrackingEvent::SessionStart {
                session_id,
                user_id,
                channel_id,
            } => {
                database.create_dm_session(&session_id, &user_id, &channel_id).await?;
                database.log_dm_event(&session_id, "session_start", &user_id, &channel_id, None).await?;
                debug!("Session started: {session_id}");
            }

            TrackingEvent::SessionEnd { session_id, reason } => {
                // Get session state and finalize metrics
                let key_to_remove = active_sessions
                    .iter()
                    .find(|entry| entry.value().session_id == session_id)
                    .map(|entry| entry.key().clone());

                if let Some(key) = key_to_remove {
                    if let Some((_, session)) = active_sessions.remove(&key) {
                        // Update session with final metrics
                        database
                            .update_dm_session_activity(
                                &session_id,
                                session.message_count,
                                session.total_user_chars,
                                session.total_bot_chars,
                                session.avg_response_time(),
                            )
                            .await?;

                        database.end_dm_session(&session_id, reason.as_str()).await?;
                        database.log_dm_event(&session_id, "session_end", &session.user_id, &session.channel_id, Some(reason.as_str())).await?;
                        debug!("Session ended: {session_id} (reason: {:?})", reason);
                    }
                }
            }

            TrackingEvent::MessageReceived {
                session_id,
                user_id,
                channel_id,
                message_id,
                character_count,
                has_attachments,
            } => {
                // Update active session state
                let key = format!("{}:{}", user_id, channel_id);
                if let Some(mut session) = active_sessions.get_mut(&key) {
                    session.add_user_message(character_count);
                }

                // Log event
                let event_data = format!(r#"{{"message_id":"{}","chars":{},"attachments":{}}}"#, message_id, character_count, has_attachments);
                database.log_dm_event(&session_id, "message_received", &user_id, &channel_id, Some(&event_data)).await?;
            }

            TrackingEvent::MessageSent {
                session_id,
                user_id,
                channel_id,
                message_id,
                character_count,
                response_time_ms,
            } => {
                // Update active session state
                let key = format!("{}:{}", user_id, channel_id);
                if let Some(mut session) = active_sessions.get_mut(&key) {
                    session.add_bot_message(character_count, response_time_ms);
                }

                // Log event
                let event_data = format!(r#"{{"message_id":"{}","chars":{},"response_time_ms":{}}}"#, message_id, character_count, response_time_ms);
                database.log_dm_event(&session_id, "message_sent", &user_id, &channel_id, Some(&event_data)).await?;
            }

            TrackingEvent::ApiCall {
                session_id,
                user_id,
                api_type,
                tokens,
                cost,
            } => {
                let api_type_str = match api_type {
                    ApiType::Chat => "chat",
                    ApiType::Whisper => "whisper",
                    ApiType::DallE => "dalle",
                };

                let event_data = format!(r#"{{"api_type":"{}","tokens":{},"cost":{}}}"#, api_type_str, tokens.unwrap_or(0), cost);
                database.log_dm_event(&session_id, "api_call", &user_id, "", Some(&event_data)).await?;

                // Update session metrics
                database.update_dm_session_metrics(&session_id, api_type_str, tokens.unwrap_or(0), cost).await?;
            }

            TrackingEvent::FeatureUsed {
                session_id,
                user_id,
                feature,
                feature_detail,
            } => {
                let feature_str = match feature {
                    FeatureType::AudioTranscription => "audio",
                    FeatureType::SlashCommand => "slash_command",
                };

                let event_data = format!(r#"{{"feature":"{}","detail":"{}"}}"#, feature_str, feature_detail);
                database.log_dm_event(&session_id, "feature_used", &user_id, "", Some(&event_data)).await?;

                // Update session metrics
                database.increment_dm_session_feature(&session_id, feature_str).await?;
            }
        }

        Ok(())
    }

    /// Background cleanup task that times out idle sessions
    async fn cleanup_task(
        _database: Database,
        active_sessions: Arc<DashMap<String, SessionState>>,
        sender: mpsc::UnboundedSender<TrackingEvent>,
    ) {
        debug!("InteractionTracker cleanup task started");

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // Run every 5 minutes

            let mut timed_out_sessions = Vec::new();

            // Find timed out sessions
            for entry in active_sessions.iter() {
                if entry.value().is_timed_out(30) {
                    timed_out_sessions.push(entry.value().session_id.clone());
                }
            }

            // End timed out sessions
            for session_id in timed_out_sessions {
                debug!("Timing out session: {session_id}");
                if let Err(e) = sender.send(TrackingEvent::SessionEnd {
                    session_id,
                    reason: SessionEndReason::InactivityTimeout,
                }) {
                    error!("Failed to send session timeout event: {e}");
                }
            }
        }
    }
}
