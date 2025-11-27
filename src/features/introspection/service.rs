//! # Feature: Self-Introspection
//!
//! Bot can explain its own internals and architecture through curated code snippets.
//! Provides explanations for: overview, personas, reminders, conflict, commands, database.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: false
//!
//! ## Changelog
//! - 1.0.0: Initial release with 6 introspection components

/// Get a curated code snippet for the given component
pub fn get_component_snippet(component: &str) -> (&'static str, &'static str) {
    match component {
        "overview" => (
            "Bot Architecture Overview",
            r#"// Main bot structure - I'm built in Rust using the Serenity Discord library

pub struct Handler {
    command_handler: Arc<CommandHandler>,      // Processes all user commands
    component_handler: Arc<MessageComponentHandler>, // Handles buttons, modals
    guild_id: Option<GuildId>,                 // Development mode guild
}

// My brain - the CommandHandler
pub struct CommandHandler {
    persona_manager: PersonaManager,           // My personalities
    database: Database,                        // My memory
    rate_limiter: RateLimiter,                // Prevents spam
    audio_transcriber: AudioTranscriber,       // I can hear audio files
    conflict_detector: ConflictDetector,       // I sense disturbances
    conflict_mediator: ConflictMediator,       // I bring peace
}

// I start up like this:
async fn main() {
    let database = Database::new(&config.database_path).await?;
    let persona_manager = PersonaManager::new();
    let command_handler = CommandHandler::new(database, ...);

    // Background task checks reminders every 60 seconds
    let scheduler = ReminderScheduler::new(database, openai_model);
    tokio::spawn(async move { scheduler.run(http).await; });

    // Connect to Discord and listen for events
    client.start().await?;
}"#,
        ),

        "personas" => (
            "Persona System",
            r#"// My personality system - each persona has a unique system prompt

pub struct Persona {
    pub name: String,
    pub system_prompt: String,  // My core personality instructions
    pub description: String,     // What I'm good at
}

pub struct PersonaManager {
    personas: HashMap<String, Persona>,  // obi, muppet, chef, teacher, analyst
}

impl PersonaManager {
    pub fn new() -> Self {
        // Prompts are embedded at compile time from prompt/*.md files
        let obi_prompt = include_str!("../prompt/obi.md");
        let muppet_prompt = include_str!("../prompt/muppet.md");
        // ... etc
    }

    // Building my full system prompt with modifiers
    pub fn get_system_prompt(&self, persona: &str, modifier: Option<&str>, verbosity: &str) -> String {
        let base_prompt = self.get_persona(persona).system_prompt;

        // Add task-specific instructions
        let modifier_suffix = match modifier {
            Some("explain") => "\n\nFocus on clear, detailed explanations.",
            Some("simple") => "\n\nUse simple language and helpful analogies.",
            Some("steps") => "\n\nBreak this down into numbered steps.",
            _ => "",
        };

        // Add verbosity control
        let verbosity_suffix = match verbosity {
            "concise" => "\n\nKeep responses brief: 2-3 sentences.",
            "detailed" => "\n\nProvide comprehensive, thorough responses.",
            _ => "",
        };

        format!("{}{}{}", base_prompt, modifier_suffix, verbosity_suffix)
    }
}"#,
        ),

        "reminders" => (
            "Reminder System",
            r#"// The reminder scheduler runs as a background task

pub struct ReminderScheduler {
    database: Database,
    persona_manager: PersonaManager,
    openai_model: String,
}

impl ReminderScheduler {
    // This runs forever, checking every 60 seconds
    pub async fn run(&self, http: Arc<Http>) {
        let mut interval = interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            self.process_due_reminders(&http).await;
        }
    }

    async fn deliver_reminder(&self, user_id: &str, reminder_text: &str) {
        // Get the user's preferred persona
        let persona = self.database.get_user_persona(user_id).await;

        // Generate an in-character reminder message using OpenAI
        let message = self.generate_reminder_message(&persona, reminder_text).await;

        // Send with @mention so they get notified
        channel.say(&http, format!("<@{}>\n\n{}", user_id, message)).await;

        // Mark as complete
        self.database.complete_reminder(reminder_id).await;
    }
}

// Time parsing - "1h30m" becomes 5400 seconds
fn parse_duration(&self, time_str: &str) -> Option<i64> {
    // Parse combinations like "1d2h30m" into total seconds
    for c in time_str.chars() {
        match c {
            'm' => total += value * 60,
            'h' => total += value * 3600,
            'd' => total += value * 86400,
            // ...
        }
    }
}"#,
        ),

        "conflict" => (
            "Conflict Detection & Mediation",
            r#"// I can sense when conversations get heated

pub struct ConflictDetector {
    hostile_keywords: Vec<&'static str>,  // Words that indicate conflict
}

impl ConflictDetector {
    // Analyze a message for signs of conflict
    pub fn analyze(&self, content: &str) -> (bool, f32, String) {
        let mut confidence = 0.0;

        // Check for hostile keywords
        for keyword in &self.hostile_keywords {
            if content.to_lowercase().contains(keyword) {
                confidence += 0.15;  // Each hostile word adds to confidence
            }
        }

        // ALL CAPS MESSAGES ARE OFTEN ANGRY
        if content.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) {
            confidence += 0.2;
        }

        // Excessive punctuation!!! suggests frustration!!!
        let punct_count = content.matches(|c| c == '!' || c == '?').count();
        if punct_count > 3 {
            confidence += 0.15;
        }

        (confidence > threshold, confidence, conflict_type)
    }
}

// When I detect conflict, I intervene with wisdom
async fn generate_mediation_response(&self, messages: &[(String, String)]) -> String {
    let prompt = "You are Obi-Wan Kenobi observing a heated conversation. \
                  Offer a brief, calming philosophical perspective. \
                  Acknowledge what's being discussed, encourage understanding.";

    // Generate a context-aware response using OpenAI
    ChatCompletion::builder(model, messages).create().await
}"#,
        ),

        "commands" => (
            "Command Processing",
            r#"// How I handle slash commands

pub async fn handle_slash_command(&self, ctx: &Context, command: &ApplicationCommandInteraction) {
    let request_id = Uuid::new_v4();  // Unique ID for logging

    // Rate limiting - prevent spam
    if !self.rate_limiter.check(&user_id) {
        return respond("You're sending commands too quickly!");
    }

    // Route to appropriate handler
    match command.data.name.as_str() {
        "ping" => self.handle_ping(ctx, command).await,
        "hey" | "explain" | "simple" | "steps" | "recipe" => {
            self.handle_ai_command(ctx, command).await  // AI-powered responses
        }
        "remind" => self.handle_remind(ctx, command).await,
        "introspect" => self.handle_introspect(ctx, command).await,  // Meta!
        _ => respond("Unknown command"),
    }
}

// AI commands use deferred responses (thinking indicator)
async fn handle_ai_command(&self, command: &ApplicationCommandInteraction) {
    // Show "thinking..." to user (we have 15 minutes to respond)
    command.create_interaction_response(DeferredChannelMessageWithSource).await;

    // Build conversation with persona system prompt + history
    let system_prompt = self.persona_manager.get_system_prompt(persona, modifier, verbosity);
    let history = self.database.get_conversation_history(user_id, channel_id, 40).await;

    // Call OpenAI
    let response = ChatCompletion::builder(model, messages).create().await;

    // Edit the deferred response with the actual answer
    command.edit_original_interaction_response(response).await;
}"#,
        ),

        "database" => (
            "Database & Memory",
            r#"// SQLite database - my long-term memory

pub struct Database {
    connection: Arc<Mutex<Connection>>,  // Thread-safe connection
}

// Tables I maintain:
// - user_preferences: Your chosen persona
// - conversation_history: Our chat history (for context)
// - reminders: Scheduled reminders
// - guild_settings: Server-wide configuration
// - channel_settings: Per-channel verbosity
// - conflict_detection: Logged conflicts
// - usage_stats: Analytics

impl Database {
    // Store each message for context
    pub async fn store_message(&self, user_id: &str, channel_id: &str,
                                role: &str, content: &str) {
        conn.execute("INSERT INTO conversation_history
                      (user_id, channel_id, role, content)
                      VALUES (?, ?, ?, ?)", ...);
    }

    // Retrieve conversation history for AI context
    pub async fn get_conversation_history(&self, user_id: &str, channel_id: &str,
                                           limit: i64) -> Vec<(String, String)> {
        conn.prepare("SELECT role, content FROM conversation_history
                      WHERE user_id = ? AND channel_id = ?
                      ORDER BY timestamp DESC LIMIT ?");
        // Returns messages in chronological order for OpenAI
    }

    // Persona cascade: user preference -> guild default -> env var -> "obi"
    pub async fn get_user_persona_with_guild(&self, user_id: &str, guild_id: Option<&str>) {
        // First check user's personal preference
        // Then fall back to guild default
        // Then environment variable
        // Finally default to "obi"
    }
}"#,
        ),

        _ => (
            "Unknown Component",
            "I don't have information about that component.",
        ),
    }
}
