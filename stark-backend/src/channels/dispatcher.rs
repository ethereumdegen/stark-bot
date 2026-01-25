use crate::ai::{AiClient, Message, MessageRole};
use crate::channels::types::{DispatchResult, NormalizedMessage};
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{MemoryType, SessionScope};
use crate::models::session_message::MessageRole as DbMessageRole;
use chrono::Utc;
use regex::Regex;
use std::sync::Arc;

/// Dispatcher routes messages to the AI and returns responses
pub struct MessageDispatcher {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    // Regex patterns for memory markers
    daily_log_pattern: Regex,
    remember_pattern: Regex,
    remember_important_pattern: Regex,
}

impl MessageDispatcher {
    pub fn new(db: Arc<Database>, broadcaster: Arc<EventBroadcaster>) -> Self {
        Self {
            db,
            broadcaster,
            daily_log_pattern: Regex::new(r"\[DAILY_LOG:\s*(.+?)\]").unwrap(),
            remember_pattern: Regex::new(r"\[REMEMBER:\s*(.+?)\]").unwrap(),
            remember_important_pattern: Regex::new(r"\[REMEMBER_IMPORTANT:\s*(.+?)\]").unwrap(),
        }
    }

    /// Dispatch a normalized message to the AI and return the response
    pub async fn dispatch(&self, message: NormalizedMessage) -> DispatchResult {
        // Emit message received event
        self.broadcaster.broadcast(GatewayEvent::channel_message(
            message.channel_id,
            &message.channel_type,
            &message.user_name,
            &message.text,
        ));

        // Check for reset commands
        let text_lower = message.text.trim().to_lowercase();
        if text_lower == "/new" || text_lower == "/reset" {
            return self.handle_reset_command(&message).await;
        }

        // Get or create identity for the user
        let identity = match self.db.get_or_create_identity(
            &message.channel_type,
            &message.user_id,
            Some(&message.user_name),
        ) {
            Ok(id) => id,
            Err(e) => {
                log::error!("Failed to get/create identity: {}", e);
                return DispatchResult::error(format!("Identity error: {}", e));
            }
        };

        // Determine session scope (group if chat_id != user_id, otherwise dm)
        let scope = if message.chat_id != message.user_id {
            SessionScope::Group
        } else {
            SessionScope::Dm
        };

        // Get or create chat session
        let session = match self.db.get_or_create_chat_session(
            &message.channel_type,
            message.channel_id,
            &message.chat_id,
            scope,
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to get/create session: {}", e);
                return DispatchResult::error(format!("Session error: {}", e));
            }
        };

        // Store user message in session
        if let Err(e) = self.db.add_session_message(
            session.id,
            DbMessageRole::User,
            &message.text,
            Some(&message.user_id),
            Some(&message.user_name),
            message.message_id.as_deref(),
            None,
        ) {
            log::error!("Failed to store user message: {}", e);
        }

        // Get active agent settings from database
        let settings = match self.db.get_active_agent_settings() {
            Ok(Some(settings)) => settings,
            Ok(None) => {
                let error = "No AI provider configured. Please configure agent settings.".to_string();
                log::error!("{}", error);
                return DispatchResult::error(error);
            }
            Err(e) => {
                let error = format!("Database error: {}", e);
                log::error!("{}", error);
                return DispatchResult::error(error);
            }
        };

        log::info!(
            "Using {} provider with model {} for message dispatch",
            settings.provider,
            settings.model
        );

        // Create AI client from settings
        let client = match AiClient::from_settings(&settings) {
            Ok(c) => c,
            Err(e) => {
                let error = format!("Failed to create AI client: {}", e);
                log::error!("{}", error);
                return DispatchResult::error(error);
            }
        };

        // Build context from memories and session history
        let system_prompt = self.build_system_prompt(&message, &identity.identity_id);

        // Get recent session messages for conversation context
        let history = self.db.get_recent_session_messages(session.id, 20).unwrap_or_default();

        // Build messages for the AI
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: system_prompt,
        }];

        // Add conversation history (skip the last one since it's the current message)
        for msg in history.iter().take(history.len().saturating_sub(1)) {
            let role = match msg.role {
                DbMessageRole::User => MessageRole::User,
                DbMessageRole::Assistant => MessageRole::Assistant,
                DbMessageRole::System => MessageRole::System,
            };
            messages.push(Message {
                role,
                content: msg.content.clone(),
            });
        }

        // Add current user message
        messages.push(Message {
            role: MessageRole::User,
            content: message.text.clone(),
        });

        // Generate response
        match client.generate_text(messages).await {
            Ok(response) => {
                // Parse and create memories from the response
                self.process_memory_markers(
                    &response,
                    &identity.identity_id,
                    session.id,
                    &message.channel_type,
                    message.message_id.as_deref(),
                );

                // Clean response by removing memory markers before storing/returning
                let clean_response = self.clean_response(&response);

                // Store AI response in session
                if let Err(e) = self.db.add_session_message(
                    session.id,
                    DbMessageRole::Assistant,
                    &clean_response,
                    None,
                    None,
                    None,
                    None,
                ) {
                    log::error!("Failed to store AI response: {}", e);
                }

                // Emit response event
                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &clean_response,
                ));

                log::info!(
                    "Generated response for {} on channel {} using {}",
                    message.user_name,
                    message.channel_id,
                    settings.provider
                );

                DispatchResult::success(clean_response)
            }
            Err(e) => {
                let error = format!("AI generation error ({}): {}", settings.provider, e);
                log::error!("{}", error);
                DispatchResult::error(error)
            }
        }
    }

    /// Build the system prompt with context from memories
    fn build_system_prompt(&self, message: &NormalizedMessage, identity_id: &str) -> String {
        let mut prompt = format!(
            "You are StarkBot, a helpful AI assistant. You are responding to a message from {} on {}.\n\n",
            message.user_name, message.channel_type
        );

        // Add daily logs context
        if let Ok(daily_logs) = self.db.get_todays_daily_logs(Some(identity_id)) {
            if !daily_logs.is_empty() {
                prompt.push_str("## Today's Notes\n");
                for log in daily_logs {
                    prompt.push_str(&format!("- {}\n", log.content));
                }
                prompt.push('\n');
            }
        }

        // Add relevant long-term memories
        if let Ok(memories) = self.db.get_long_term_memories(Some(identity_id), Some(5), 10) {
            if !memories.is_empty() {
                prompt.push_str("## Things to Remember About This User\n");
                for mem in memories {
                    prompt.push_str(&format!("- {}\n", mem.content));
                }
                prompt.push('\n');
            }
        }

        // Add instructions for memory markers
        prompt.push_str(
            "## Memory Instructions\n\
            You can save information for future conversations using these markers:\n\
            - [DAILY_LOG: note] - Save a note for today's log (temporary, resets daily)\n\
            - [REMEMBER: fact] - Save an important fact about the user (persists long-term)\n\
            - [REMEMBER_IMPORTANT: critical fact] - Save a critical fact (high importance)\n\n\
            Use these sparingly and only for genuinely useful information.\n\n\
            Keep responses concise and helpful."
        );

        prompt
    }

    /// Process memory markers in the AI response
    fn process_memory_markers(
        &self,
        response: &str,
        identity_id: &str,
        session_id: i64,
        channel_type: &str,
        message_id: Option<&str>,
    ) {
        let today = Utc::now().date_naive();

        // Process daily logs
        for cap in self.daily_log_pattern.captures_iter(response) {
            if let Some(content) = cap.get(1) {
                let content_str = content.as_str().trim();
                if !content_str.is_empty() {
                    if let Err(e) = self.db.create_memory(
                        MemoryType::DailyLog,
                        content_str,
                        None,
                        None,
                        5,
                        Some(identity_id),
                        Some(session_id),
                        Some(channel_type),
                        message_id,
                        Some(today),
                        None,
                    ) {
                        log::error!("Failed to create daily log: {}", e);
                    } else {
                        log::info!("Created daily log: {}", content_str);
                    }
                }
            }
        }

        // Process regular remember markers (importance 7)
        for cap in self.remember_pattern.captures_iter(response) {
            if let Some(content) = cap.get(1) {
                let content_str = content.as_str().trim();
                if !content_str.is_empty() {
                    if let Err(e) = self.db.create_memory(
                        MemoryType::LongTerm,
                        content_str,
                        None,
                        None,
                        7,
                        Some(identity_id),
                        Some(session_id),
                        Some(channel_type),
                        message_id,
                        None,
                        None,
                    ) {
                        log::error!("Failed to create long-term memory: {}", e);
                    } else {
                        log::info!("Created long-term memory: {}", content_str);
                    }
                }
            }
        }

        // Process important remember markers (importance 9)
        for cap in self.remember_important_pattern.captures_iter(response) {
            if let Some(content) = cap.get(1) {
                let content_str = content.as_str().trim();
                if !content_str.is_empty() {
                    if let Err(e) = self.db.create_memory(
                        MemoryType::LongTerm,
                        content_str,
                        None,
                        None,
                        9,
                        Some(identity_id),
                        Some(session_id),
                        Some(channel_type),
                        message_id,
                        None,
                        None,
                    ) {
                        log::error!("Failed to create important memory: {}", e);
                    } else {
                        log::info!("Created important memory: {}", content_str);
                    }
                }
            }
        }
    }

    /// Remove memory markers from the response before returning to user
    fn clean_response(&self, response: &str) -> String {
        let mut clean = response.to_string();
        clean = self.daily_log_pattern.replace_all(&clean, "").to_string();
        clean = self.remember_pattern.replace_all(&clean, "").to_string();
        clean = self.remember_important_pattern.replace_all(&clean, "").to_string();
        // Clean up any double spaces or trailing whitespace
        clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        clean.trim().to_string()
    }

    /// Handle /new or /reset commands
    async fn handle_reset_command(&self, message: &NormalizedMessage) -> DispatchResult {
        // Determine session scope
        let scope = if message.chat_id != message.user_id {
            SessionScope::Group
        } else {
            SessionScope::Dm
        };

        // Get the current session
        match self.db.get_or_create_chat_session(
            &message.channel_type,
            message.channel_id,
            &message.chat_id,
            scope,
            None,
        ) {
            Ok(session) => {
                // Reset the session
                match self.db.reset_chat_session(session.id) {
                    Ok(_) => {
                        let response = "Session reset. Let's start fresh!".to_string();
                        self.broadcaster.broadcast(GatewayEvent::agent_response(
                            message.channel_id,
                            &message.user_name,
                            &response,
                        ));
                        DispatchResult::success(response)
                    }
                    Err(e) => {
                        log::error!("Failed to reset session: {}", e);
                        DispatchResult::error(format!("Failed to reset session: {}", e))
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to get session for reset: {}", e);
                DispatchResult::error(format!("Session error: {}", e))
            }
        }
    }
}
