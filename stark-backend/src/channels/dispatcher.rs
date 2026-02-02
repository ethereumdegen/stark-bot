use crate::ai::{
    multi_agent::{types::{AgentSubtype, AgentMode}, Orchestrator, ProcessResult as OrchestratorResult, SubAgentManager},
    AiClient, ArchetypeId, ArchetypeRegistry, AiResponse, Message, MessageRole, ModelArchetype,
    ThinkingLevel, ToolHistoryEntry, ToolResponse,
};
use crate::channels::types::{DispatchResult, NormalizedMessage};
use crate::config::MemoryConfig;
use crate::context::{self, estimate_tokens, ContextManager};
use crate::controllers::api_keys::ApiKeyId;
use std::str::FromStr;
use crate::db::Database;
use crate::execution::ExecutionTracker;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::session_message::MessageRole as DbMessageRole;
use crate::models::{AgentSettings, CompletionStatus, MemoryType, SessionScope, DEFAULT_MAX_TOOL_ITERATIONS};
use crate::tools::{ToolConfig, ToolContext, ToolDefinition, ToolExecution, ToolRegistry};
use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

/// Compiled regex patterns - avoid recompiling on every call
static INLINE_THINKING_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^/(?:t|think|thinking):(\w+)\s+(.+)$").unwrap()
});
static THINKING_DIRECTIVE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^/(?:t|think|thinking)(?::(\w+))?$").unwrap()
});

/// Fallback maximum tool iterations (used when db lookup fails)
/// Actual value is configurable via bot settings
const FALLBACK_MAX_TOOL_ITERATIONS: usize = DEFAULT_MAX_TOOL_ITERATIONS as usize;

/// How often to broadcast "still waiting" events during long AI calls
const AI_PROGRESS_INTERVAL_SECS: u64 = 30;

/// Configuration for a memory marker pattern
struct MemoryMarkerConfig {
    pattern: Regex,
    memory_type: MemoryType,
    importance: i32,
    name: &'static str,
    use_today_date: bool,
}

impl MemoryMarkerConfig {
    fn new(
        pattern: &str,
        memory_type: MemoryType,
        importance: i32,
        name: &'static str,
        use_today_date: bool,
    ) -> Self {
        Self {
            pattern: Regex::new(pattern).unwrap(),
            memory_type,
            importance,
            name,
            use_today_date,
        }
    }
}

/// Create all memory marker configurations
fn create_memory_markers() -> Vec<MemoryMarkerConfig> {
    vec![
        MemoryMarkerConfig::new(
            r"\[DAILY_LOG:\s*(.+?)\]",
            MemoryType::DailyLog,
            5,
            "daily log",
            true,
        ),
        MemoryMarkerConfig::new(
            r"\[REMEMBER:\s*(.+?)\]",
            MemoryType::LongTerm,
            7,
            "long-term memory",
            false,
        ),
        MemoryMarkerConfig::new(
            r"\[REMEMBER_IMPORTANT:\s*(.+?)\]",
            MemoryType::LongTerm,
            9,
            "important memory",
            false,
        ),
        // Phase 2: New memory types
        MemoryMarkerConfig::new(
            r"\[PREFERENCE:\s*(.+?)\]",
            MemoryType::Preference,
            7,
            "user preference",
            false,
        ),
        MemoryMarkerConfig::new(
            r"\[FACT:\s*(.+?)\]",
            MemoryType::Fact,
            7,
            "user fact",
            false,
        ),
        MemoryMarkerConfig::new(
            r"\[TASK:\s*(.+?)\]",
            MemoryType::Task,
            8,
            "task/commitment",
            true, // Use today's date for tasks
        ),
    ]
}

/// Dispatcher routes messages to the AI and returns responses
pub struct MessageDispatcher {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    tool_registry: Arc<ToolRegistry>,
    execution_tracker: Arc<ExecutionTracker>,
    burner_wallet_private_key: Option<String>,
    context_manager: ContextManager,
    archetype_registry: ArchetypeRegistry,
    /// Memory marker configurations
    memory_markers: Vec<MemoryMarkerConfig>,
    /// Memory configuration for cross-session and other features
    memory_config: MemoryConfig,
    /// SubAgent manager for spawning background AI agents
    subagent_manager: Option<Arc<SubAgentManager>>,
    /// Skill registry for managing skills
    skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
    /// Hook manager for lifecycle events
    hook_manager: Option<Arc<crate::hooks::HookManager>>,
    /// Tool validator registry for pre-execution validation
    validator_registry: Option<Arc<crate::tool_validators::ValidatorRegistry>>,
    /// Transaction queue manager for queued web3 transactions
    tx_queue: Option<Arc<crate::tx_queue::TxQueueManager>>,
}

impl MessageDispatcher {
    pub fn new(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
    ) -> Self {
        Self::new_with_wallet(db, broadcaster, tool_registry, execution_tracker, None)
    }

    pub fn new_with_wallet(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        burner_wallet_private_key: Option<String>,
    ) -> Self {
        Self::new_with_wallet_and_skills(
            db,
            broadcaster,
            tool_registry,
            execution_tracker,
            burner_wallet_private_key,
            None,
        )
    }

    pub fn new_with_wallet_and_skills(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        burner_wallet_private_key: Option<String>,
        skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
    ) -> Self {
        let memory_config = MemoryConfig::from_env();
        let context_manager = ContextManager::new(db.clone())
            .with_memory_config(memory_config.clone());

        // Create SubAgentManager for spawning background AI agents
        let subagent_manager = Arc::new(SubAgentManager::new_with_config(
            db.clone(),
            broadcaster.clone(),
            tool_registry.clone(),
            Default::default(),
            burner_wallet_private_key.clone(),
        ));
        log::info!("[DISPATCHER] SubAgentManager initialized");

        Self {
            db,
            broadcaster,
            tool_registry,
            execution_tracker,
            burner_wallet_private_key,
            context_manager,
            archetype_registry: ArchetypeRegistry::new(),
            memory_markers: create_memory_markers(),
            memory_config,
            subagent_manager: Some(subagent_manager),
            skill_registry,
            hook_manager: None,
            validator_registry: None,
            tx_queue: None,
        }
    }

    /// Set the hook manager for lifecycle events
    pub fn with_hook_manager(mut self, hook_manager: Arc<crate::hooks::HookManager>) -> Self {
        self.hook_manager = Some(hook_manager);
        self
    }

    /// Set the tool validator registry for pre-execution validation
    pub fn with_validator_registry(mut self, validator_registry: Arc<crate::tool_validators::ValidatorRegistry>) -> Self {
        self.validator_registry = Some(validator_registry);
        self
    }

    /// Set the transaction queue manager
    pub fn with_tx_queue(mut self, tx_queue: Arc<crate::tx_queue::TxQueueManager>) -> Self {
        self.tx_queue = Some(tx_queue);
        self
    }

    /// Create a dispatcher without tool support (for backwards compatibility)
    pub fn new_without_tools(db: Arc<Database>, broadcaster: Arc<EventBroadcaster>) -> Self {
        // Create a minimal execution tracker for legacy use
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
        let memory_config = MemoryConfig::from_env();
        let context_manager = ContextManager::new(db.clone())
            .with_memory_config(memory_config.clone());
        Self {
            db: db.clone(),
            broadcaster,
            tool_registry: Arc::new(ToolRegistry::new()),
            execution_tracker,
            burner_wallet_private_key: None,
            context_manager,
            archetype_registry: ArchetypeRegistry::new(),
            memory_markers: create_memory_markers(),
            memory_config,
            subagent_manager: None, // No tools = no subagent support
            skill_registry: None,   // No skills without tools
            hook_manager: None,     // No hooks without explicit setup
            validator_registry: None, // No validators without explicit setup
            tx_queue: None,         // No tx queue without explicit setup
        }
    }

    /// Get the SubAgentManager (if available)
    pub fn subagent_manager(&self) -> Option<Arc<SubAgentManager>> {
        self.subagent_manager.clone()
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

        // Check for thinking directives (session-level setting)
        if let Some(thinking_response) = self.handle_thinking_directive(&message).await {
            return thinking_response;
        }

        // Parse inline thinking directive and extract clean message
        let (thinking_level, clean_text) = self.parse_inline_thinking(&message.text);

        // Start execution tracking with user message for descriptive display
        let user_msg = clean_text.as_deref().unwrap_or(&message.text);
        let execution_id = self.execution_tracker.start_execution(
            message.channel_id,
            "execute",
            Some(user_msg),
        );

        // Get or create identity for the user
        let identity = match self.db.get_or_create_identity(
            &message.channel_type,
            &message.user_id,
            Some(&message.user_name),
        ) {
            Ok(id) => id,
            Err(e) => {
                let error_msg = format!("Identity error: {}", e);
                log::error!("Failed to get/create identity: {}", e);
                self.broadcaster.broadcast(GatewayEvent::agent_error(
                    message.channel_id,
                    &error_msg,
                ));
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error_msg);
            }
        };

        // Determine session scope based on session_mode (for cron) or chat context
        let scope = if let Some(ref mode) = message.session_mode {
            // Cron job with explicit session_mode
            match mode.as_str() {
                "isolated" => SessionScope::Cron,
                "main" => {
                    // Main mode uses existing session logic (shares with web chat)
                    if message.chat_id != message.user_id {
                        SessionScope::Group
                    } else {
                        SessionScope::Dm
                    }
                }
                _ => SessionScope::Dm, // fallback
            }
        } else {
            // Original logic for non-cron messages
            if message.chat_id != message.user_id {
                SessionScope::Group
            } else {
                SessionScope::Dm
            }
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
                let error_msg = format!("Session error: {}", e);
                log::error!("Failed to get/create session: {}", e);
                self.broadcaster.broadcast(GatewayEvent::agent_error(
                    message.channel_id,
                    &error_msg,
                ));
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error_msg);
            }
        };

        // Reset session state when a new message comes in on a previously-completed session
        // This allows the session to be reused for new requests
        if let Ok(Some(status)) = self.db.get_session_completion_status(session.id) {
            if status.should_stop() {
                log::info!(
                    "[DISPATCH] Resetting session {} from {:?} to Active for new request",
                    session.id, status
                );
                if let Err(e) = self.db.update_session_completion_status(session.id, CompletionStatus::Active) {
                    log::error!("[DISPATCH] Failed to reset session completion status: {}", e);
                }
                // Also reset total_iterations in AgentContext if it exists
                if let Ok(Some(mut context)) = self.db.get_agent_context(session.id) {
                    context.total_iterations = 0;
                    context.mode_iterations = 0;
                    if let Err(e) = self.db.save_agent_context(session.id, &context) {
                        log::error!("[DISPATCH] Failed to reset agent context iterations: {}", e);
                    }
                }
            }
        }

        // Use clean text (with inline thinking directive removed) for storage
        let message_text = clean_text.as_deref().unwrap_or(&message.text);

        // Estimate tokens for the user message
        let user_tokens = estimate_tokens(message_text);

        // Store user message in session with token count
        if let Err(e) = self.db.add_session_message(
            session.id,
            DbMessageRole::User,
            message_text,
            Some(&message.user_id),
            Some(&message.user_name),
            message.message_id.as_deref(),
            Some(user_tokens),
        ) {
            log::error!("Failed to store user message: {}", e);
        } else {
            // Update context tokens
            self.context_manager.update_context_tokens(session.id, user_tokens);
        }

        // Get active agent settings from database, falling back to kimi defaults
        let settings = match self.db.get_active_agent_settings() {
            Ok(Some(settings)) => settings,
            Ok(None) => {
                log::info!("No agent configured, using default kimi settings");
                AgentSettings::default()
            }
            Err(e) => {
                let error = format!("Database error: {}", e);
                log::error!("{}", error);
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error);
            }
        };

        // Infer archetype from settings
        let archetype_id = AiClient::infer_archetype(&settings);
        log::info!(
            "Using endpoint {} for message dispatch (archetype={}, max_tokens={})",
            settings.endpoint,
            archetype_id,
            settings.max_tokens
        );

        // Create AI client from settings with x402 wallet support
        let client = match AiClient::from_settings_with_wallet(
            &settings,
            self.burner_wallet_private_key.as_deref(),
        ) {
            Ok(c) => c.with_broadcaster(Arc::clone(&self.broadcaster), message.channel_id),
            Err(e) => {
                let error = format!("Failed to create AI client: {}", e);
                log::error!("{}", error);
                self.broadcaster.broadcast(GatewayEvent::agent_error(
                    message.channel_id,
                    &error,
                ));
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error);
            }
        };

        // Add thinking event before AI generation
        self.execution_tracker.add_thinking(message.channel_id, "Processing request...");

        // Get tool configuration for this channel (needed for system prompt)
        let tool_config = self.db.get_effective_tool_config(Some(message.channel_id))
            .unwrap_or_default();

        // Debug: Log tool configuration
        log::info!(
            "[DISPATCH] Tool config - profile: {:?}, allowed_groups: {:?}",
            tool_config.profile,
            tool_config.allowed_groups
        );

        // Build context from memories, tools, skills, and session history
        let system_prompt = self.build_system_prompt(&message, &identity.identity_id, &tool_config);

        // Debug: Log full system prompt
        log::debug!("[DISPATCH] System prompt:\n{}", system_prompt);

        // Get recent session messages for conversation context
        let history = self.db.get_recent_session_messages(session.id, 20).unwrap_or_default();

        // Build messages for the AI
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: system_prompt.clone(),
        }];

        // Add compaction summary if available (provides context from earlier in conversation)
        if let Some(compaction_summary) = self.context_manager.get_compaction_summary(session.id) {
            messages.push(Message {
                role: MessageRole::System,
                content: format!("## Previous Conversation Summary\n{}", compaction_summary),
            });
        }

        // Add conversation history (skip the last one since it's the current message)
        // Also skip tool calls and results as they're not part of the AI conversation format
        for msg in history.iter().take(history.len().saturating_sub(1)) {
            let role = match msg.role {
                DbMessageRole::User => MessageRole::User,
                DbMessageRole::Assistant => MessageRole::Assistant,
                DbMessageRole::System => MessageRole::System,
                // Skip tool calls and results - they're stored for history but not sent to AI
                DbMessageRole::ToolCall | DbMessageRole::ToolResult => continue,
            };
            messages.push(Message {
                role,
                content: msg.content.clone(),
            });
        }

        // Add current user message (use clean text without thinking directive)
        messages.push(Message {
            role: MessageRole::User,
            content: message_text.to_string(),
        });

        // Debug: Log user message
        log::info!("[DISPATCH] User message: {}", message_text);

        // Apply thinking level if set (for Claude models)
        if let Some(level) = thinking_level {
            if client.supports_thinking() {
                log::info!("[DISPATCH] Applying thinking level: {}", level);
                client.set_thinking_level(level);
            }
        }

        // Check if the client supports tools and tools are configured
        let use_tools = client.supports_tools() && !self.tool_registry.is_empty();

        // Debug: Log tool availability
        log::info!(
            "[DISPATCH] Tool support - client_supports: {}, registry_count: {}, use_tools: {}",
            client.supports_tools(),
            self.tool_registry.len(),
            use_tools
        );

        // Build tool context with API keys from database
        let workspace_dir = crate::config::workspace_dir();

        let mut tool_context = ToolContext::new()
            .with_channel(message.channel_id, message.channel_type.clone())
            .with_user(message.user_id.clone())
            .with_session(session.id)
            .with_workspace(workspace_dir.clone())
            .with_broadcaster(self.broadcaster.clone())
            .with_database(self.db.clone());

        // Add SubAgentManager for spawning background AI agents
        if let Some(ref manager) = self.subagent_manager {
            tool_context = tool_context.with_subagent_manager(manager.clone());
            log::debug!("[DISPATCH] SubAgentManager attached to tool context");
        }

        // Add SkillRegistry for skill management
        if let Some(ref registry) = self.skill_registry {
            tool_context = tool_context.with_skill_registry(registry.clone());
            log::debug!("[DISPATCH] SkillRegistry attached to tool context");
        }

        // Add TxQueueManager for web3 transaction queuing
        if let Some(ref tx_queue) = self.tx_queue {
            tool_context = tool_context.with_tx_queue(tx_queue.clone());
            log::debug!("[DISPATCH] TxQueueManager attached to tool context");
        }

        // Ensure workspace directory exists
        let _ = std::fs::create_dir_all(&workspace_dir);

        // Load API keys from database for tools that need them
        // Each key is stored individually (e.g., "GITHUB_TOKEN", "DISCORD_BOT_TOKEN")
        // Keys are added to both ToolContext AND environment variables for maximum compatibility
        let mut github_token_loaded = false;
        if let Ok(keys) = self.db.list_api_keys() {
            for key in keys {
                // Add to tool context (for tools that use context.get_api_key)
                tool_context = tool_context.with_api_key(&key.service_name, key.api_key.clone());

                // Also set as environment variables (for tools that use std::env)
                // Use the ApiKeyId to get all env var names for this key
                // SAFETY: We're setting env vars at startup before spawning threads that read them
                if let Ok(key_id) = ApiKeyId::from_str(&key.service_name) {
                    if key_id == ApiKeyId::GithubToken {
                        github_token_loaded = true;
                    }
                    if let Some(env_vars) = key_id.env_vars() {
                        for env_var in env_vars {
                            unsafe { std::env::set_var(env_var, &key.api_key); }
                        }
                    }
                }
            }
        }

        // If GitHub token is loaded, query GitHub API to get authenticated user
        // and set GITHUB_USER env var for use in git/gh commands
        if github_token_loaded {
            if let Ok(github_user) = self.get_github_authenticated_user().await {
                log::info!("[DISPATCH] GitHub authenticated as: {}", github_user);
                unsafe { std::env::set_var("GITHUB_USER", &github_user); }
                tool_context.extra.insert(
                    "github_user".to_string(),
                    serde_json::json!(github_user),
                );
            }
        }

        // Load bot config from bot_settings for git commits etc.
        if let Ok(bot_settings) = self.db.get_bot_settings() {
            tool_context = tool_context.with_bot_config(bot_settings.bot_name.clone(), bot_settings.bot_email.clone());

            // Add RPC configuration to context for x402_rpc tool
            tool_context.extra.insert(
                "rpc_provider".to_string(),
                serde_json::json!(bot_settings.rpc_provider),
            );
            if let Some(ref endpoints) = bot_settings.custom_rpc_endpoints {
                tool_context.extra.insert(
                    "custom_rpc_endpoints".to_string(),
                    serde_json::json!(endpoints),
                );
            }
        }

        // Generate response with optional tool execution loop
        let final_response = if use_tools {
            self.generate_with_tool_loop(
                &client,
                messages,
                &tool_config,
                &tool_context,
                &identity.identity_id,
                session.id,
                &message,
                archetype_id,
            ).await
        } else {
            // Simple generation without tools - with x402 event emission
            match client.generate_text_with_events(messages, &self.broadcaster, message.channel_id).await {
                Ok((content, payment)) => {
                    // Save x402 payment if one was made
                    if let Some(ref payment_info) = payment {
                        if let Err(e) = self.db.record_x402_payment(
                            Some(message.channel_id),
                            None,
                            payment_info.resource.as_deref(),
                            &payment_info.amount,
                            &payment_info.amount_formatted,
                            &payment_info.asset,
                            &payment_info.pay_to,
                            payment_info.tx_hash.as_deref(),
                            &payment_info.status.to_string(),
                        ) {
                            log::error!("[DISPATCH] Failed to record x402 payment: {}", e);
                        }
                    }
                    Ok(content)
                }
                Err(e) => Err(e),
            }
        };

        match final_response {
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

                // Estimate tokens for the response
                let response_tokens = estimate_tokens(&clean_response);

                // Store AI response in session with token count
                if let Err(e) = self.db.add_session_message(
                    session.id,
                    DbMessageRole::Assistant,
                    &clean_response,
                    None,
                    None,
                    None,
                    Some(response_tokens),
                ) {
                    log::error!("Failed to store AI response: {}", e);
                } else {
                    // Update context tokens
                    self.context_manager.update_context_tokens(session.id, response_tokens);

                    // Check if compaction is needed
                    if self.context_manager.needs_compaction(session.id) {
                        log::info!("[COMPACTION] Context limit reached for session {}, triggering compaction", session.id);
                        if let Err(e) = self.context_manager.compact_session(
                            session.id,
                            &client,
                            Some(&identity.identity_id),
                        ).await {
                            log::error!("[COMPACTION] Failed to compact session: {}", e);
                        }
                    }
                }

                // Emit response event
                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &clean_response,
                ));

                log::info!(
                    "Generated response for {} on channel {} using {} archetype",
                    message.user_name,
                    message.channel_id,
                    archetype_id
                );

                // Complete execution tracking
                self.execution_tracker.complete_execution(message.channel_id);

                DispatchResult::success(clean_response)
            }
            Err(e) => {
                let error = format!("AI generation error ({}): {}", archetype_id, e);
                log::error!("{}", error);

                // Broadcast error to frontend
                self.broadcaster.broadcast(GatewayEvent::agent_error(
                    message.channel_id,
                    &error,
                ));

                // Complete execution tracking on error
                self.execution_tracker.complete_execution(message.channel_id);

                DispatchResult::error(error)
            }
        }
    }

    /// Generate a response with tool execution loop (supports both native and text-based tool calling)
    /// Now always runs in multi-agent mode with Explore → Plan → Perform flow
    async fn generate_with_tool_loop(
        &self,
        client: &AiClient,
        messages: Vec<Message>,
        tool_config: &ToolConfig,
        tool_context: &ToolContext,
        _identity_id: &str,
        session_id: i64,
        original_message: &NormalizedMessage,
        archetype_id: ArchetypeId,
    ) -> Result<String, String> {
        // Load existing agent context or create new one
        let mut orchestrator = match self.db.get_agent_context(session_id) {
            Ok(Some(context)) => {
                log::info!(
                    "[MULTI_AGENT] Resuming session {} (iteration {})",
                    session_id,
                    context.mode_iterations
                );
                let mut orch = Orchestrator::from_context(context);
                // Clear active skill at the start of each new message to prevent stale skills
                // from being used. Skills should only be active for the turn they were invoked.
                orch.clear_active_skill();
                orch
            }
            Ok(None) => {
                log::info!(
                    "[MULTI_AGENT] Starting new orchestrator for session {}",
                    session_id
                );
                Orchestrator::new(original_message.text.clone())
            }
            Err(e) => {
                log::warn!(
                    "[MULTI_AGENT] Failed to load context for session {}: {}, starting fresh",
                    session_id, e
                );
                Orchestrator::new(original_message.text.clone())
            }
        };

        // Broadcast initial mode
        let initial_mode = orchestrator.current_mode();
        self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
            original_message.channel_id,
            &initial_mode.to_string(),
            initial_mode.label(),
            Some("Processing request"),
        ));

        // Broadcast initial task state
        self.broadcast_tasks_update(original_message.channel_id, &orchestrator);

        // Get the current subtype
        let subtype = orchestrator.current_subtype();

        log::info!(
            "[MULTI_AGENT] Started in {} mode ({} subtype) for request: {}",
            initial_mode,
            subtype.label(),
            original_message.text.chars().take(50).collect::<String>()
        );

        // Broadcast initial subtype
        self.broadcaster.broadcast(GatewayEvent::agent_subtype_change(
            original_message.channel_id,
            subtype.as_str(),
            subtype.label(),
        ));

        // Get regular tools from registry, filtered by subtype
        // If there's an active skill with requires_tools, force-include those tools
        let mut tools = if let Some(ref active_skill) = orchestrator.context().active_skill {
            if !active_skill.requires_tools.is_empty() {
                log::info!(
                    "[TOOL_LOOP] Active skill '{}' requires tools: {:?}",
                    active_skill.name,
                    active_skill.requires_tools
                );
                self.tool_registry
                    .get_tool_definitions_for_subtype_with_required(
                        tool_config,
                        subtype,
                        &active_skill.requires_tools,
                    )
            } else {
                self.tool_registry.get_tool_definitions_for_subtype(tool_config, subtype)
            }
        } else {
            self.tool_registry.get_tool_definitions_for_subtype(tool_config, subtype)
        };

        // Add skills as a "use_skill" pseudo-tool if any are enabled
        // Skills are also filtered by subtype tags
        if let Some(skill_tool) = self.create_skill_tool_definition_for_subtype(subtype) {
            tools.push(skill_tool);
        }

        // Add orchestrator mode-specific tools
        let mode_tools = orchestrator.get_mode_tools();
        tools.extend(mode_tools);

        // Debug: Log available tools
        log::info!(
            "[TOOL_LOOP] Available tools ({}): {:?}",
            tools.len(),
            tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        // Broadcast toolset update to UI
        self.broadcast_toolset_update(
            original_message.channel_id,
            &orchestrator.current_mode().to_string(),
            orchestrator.current_subtype().as_str(),
            &tools,
        );

        if tools.is_empty() {
            log::warn!("[TOOL_LOOP] No tools available, falling back to text-only generation");
            let (content, payment) = client.generate_text_with_events(messages, &self.broadcaster, original_message.channel_id).await?;
            // Save x402 payment if one was made
            if let Some(ref payment_info) = payment {
                if let Err(e) = self.db.record_x402_payment(
                    Some(original_message.channel_id),
                    None,
                    payment_info.resource.as_deref(),
                    &payment_info.amount,
                    &payment_info.amount_formatted,
                    &payment_info.asset,
                    &payment_info.pay_to,
                    payment_info.tx_hash.as_deref(),
                    &payment_info.status.to_string(),
                ) {
                    log::error!("[TOOL_LOOP] Failed to record x402 payment: {}", e);
                }
            }
            return Ok(content);
        }

        // Get the archetype for this request
        let archetype = self.archetype_registry.get(archetype_id)
            .unwrap_or_else(|| self.archetype_registry.default_archetype());

        log::info!(
            "[TOOL_LOOP] Using archetype: {} (native_tool_calling: {})",
            archetype.id(),
            archetype.uses_native_tool_calling()
        );

        // Branch based on archetype type
        if archetype.uses_native_tool_calling() {
            self.generate_with_native_tools_orchestrated(
                client, messages, tools, tool_config, tool_context,
                original_message, archetype, &mut orchestrator, session_id
            ).await
        } else {
            self.generate_with_text_tools_orchestrated(
                client, messages, tools, tool_config, tool_context,
                original_message, archetype, &mut orchestrator, session_id
            ).await
        }
    }

    /// Broadcast the current toolset to the UI for debug panel visibility
    fn broadcast_toolset_update(
        &self,
        channel_id: i64,
        mode: &str,
        subtype: &str,
        tools: &[ToolDefinition],
    ) {
        let tool_summaries: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "group": format!("{:?}", t.group),
                })
            })
            .collect();

        self.broadcaster.broadcast(GatewayEvent::agent_toolset_update(
            channel_id,
            mode,
            subtype,
            tool_summaries,
        ));
    }

    /// Create a "use_skill" tool definition if skills are enabled
    fn create_skill_tool_definition(&self) -> Option<ToolDefinition> {
        // Default to Finance subtype for backwards compatibility
        self.create_skill_tool_definition_for_subtype(AgentSubtype::Finance)
    }

    /// Create a "use_skill" tool definition showing ALL enabled skills
    /// (no subtype filtering - AI can see all skills and switch subtypes if needed)
    fn create_skill_tool_definition_for_subtype(
        &self,
        _subtype: AgentSubtype,
    ) -> Option<ToolDefinition> {
        use crate::tools::{PropertySchema, ToolGroup, ToolInputSchema};

        let skills = self.db.list_enabled_skills().ok()?;

        if skills.is_empty() {
            return None;
        }

        let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();

        let mut properties = std::collections::HashMap::new();
        properties.insert(
            "skill_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: format!("The skill to execute. Options: {}", skill_names.join(", ")),
                default: None,
                items: None,
                enum_values: Some(skill_names),
            },
        );
        properties.insert(
            "input".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Input or query for the skill".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        // Format skill descriptions with newlines for better readability
        let formatted_skills = skills
            .iter()
            .map(|s| format!("  - {}: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n");

        Some(ToolDefinition {
            name: "use_skill".to_string(),
            description: format!(
                "Execute a specialized skill. YOU MUST use this tool when a user asks for something that matches a skill.\n\nAvailable skills:\n{}",
                formatted_skills
            ),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties,
                required: vec!["skill_name".to_string(), "input".to_string()],
            },
            group: ToolGroup::System,
        })
    }

    /// Broadcast status update event for the debug panel
    fn broadcast_tasks_update(&self, channel_id: i64, orchestrator: &Orchestrator) {
        let context = orchestrator.context();
        let mode = context.mode;

        // Send simplified status (no task list anymore)
        let stats_json = serde_json::json!({
            "iterations": context.mode_iterations,
            "total_iterations": context.total_iterations,
            "notes_count": context.exploration_notes.len()
        });

        self.broadcaster.broadcast(GatewayEvent::agent_tasks_update(
            channel_id,
            &mode.to_string(),
            mode.label(),
            serde_json::json!([]), // Empty tasks array
            stats_json,
        ));

        // Also broadcast task queue update if there are tasks
        if !context.task_queue.is_empty() {
            let current_task_id = context.task_queue.current_task().map(|t| t.id);
            self.broadcaster.broadcast(GatewayEvent::task_queue_update(
                channel_id,
                &context.task_queue.tasks,
                current_task_id,
            ));
        }
    }

    /// Broadcast task queue update (full queue state)
    fn broadcast_task_queue_update(&self, channel_id: i64, orchestrator: &Orchestrator) {
        let task_queue = orchestrator.task_queue();
        let current_task_id = task_queue.current_task().map(|t| t.id);

        // Store tasks in execution tracker for API access (page refresh)
        self.execution_tracker.set_planner_tasks(channel_id, task_queue.tasks.clone());

        self.broadcaster.broadcast(GatewayEvent::task_queue_update(
            channel_id,
            &task_queue.tasks,
            current_task_id,
        ));
    }

    /// Broadcast task status change
    fn broadcast_task_status_change(&self, channel_id: i64, task_id: u32, status: &str, description: &str) {
        self.broadcaster.broadcast(GatewayEvent::task_status_change(
            channel_id,
            task_id,
            status,
            description,
        ));
    }

    /// Broadcast session complete
    fn broadcast_session_complete(&self, channel_id: i64, session_id: i64) {
        // Clear stored planner tasks since session is complete
        self.execution_tracker.clear_planner_tasks(channel_id);

        self.broadcaster.broadcast(GatewayEvent::session_complete(
            channel_id,
            session_id,
        ));
    }

    /// Generate response using native API tool calling with multi-agent orchestration
    async fn generate_with_native_tools_orchestrated(
        &self,
        client: &AiClient,
        messages: Vec<Message>,
        mut tools: Vec<ToolDefinition>,
        tool_config: &ToolConfig,
        tool_context: &ToolContext,
        original_message: &NormalizedMessage,
        archetype: &dyn ModelArchetype,
        orchestrator: &mut Orchestrator,
        session_id: i64,
    ) -> Result<String, String> {
        // Get max tool iterations from bot settings
        let max_tool_iterations = self.db.get_bot_settings()
            .map(|s| s.max_tool_iterations as usize)
            .unwrap_or(FALLBACK_MAX_TOOL_ITERATIONS);

        // Build conversation with orchestrator's system prompt prepended
        let mut conversation = messages.clone();
        if let Some(system_msg) = conversation.first_mut() {
            if system_msg.role == MessageRole::System {
                // Prepend orchestrator context to the existing system prompt
                let orchestrator_prompt = orchestrator.get_system_prompt();
                system_msg.content = format!(
                    "{}\n\n---\n\n{}",
                    orchestrator_prompt,
                    archetype.enhance_system_prompt(&system_msg.content, &tools)
                );
            }
        }

        // Clear waiting_for_user_context now that it's been consumed into the prompt
        orchestrator.clear_waiting_for_user_context();

        let mut tool_history: Vec<ToolHistoryEntry> = Vec::new();
        let mut iterations = 0;
        let mut tool_call_log: Vec<String> = Vec::new();
        let mut orchestrator_complete = false;
        let mut final_summary = String::new();
        let mut waiting_for_user_response = false;
        let mut user_question_content = String::new();
        let mut was_cancelled = false;

        loop {
            iterations += 1;
            log::info!(
                "[ORCHESTRATED_LOOP] Iteration {} in {} mode",
                iterations,
                orchestrator.current_mode()
            );

            // === DETERMINE TOOLS FOR CURRENT MODE ===
            // In TaskPlanner mode (first iteration), use only define_tasks tool
            let current_tools = if orchestrator.current_mode() == AgentMode::TaskPlanner && !orchestrator.context().planner_completed {
                log::info!("[ORCHESTRATED_LOOP] Using TaskPlanner mode tools (define_tasks only)");
                // Update conversation with planner prompt
                if let Some(system_msg) = conversation.first_mut() {
                    if system_msg.role == MessageRole::System {
                        let planner_prompt = orchestrator.get_planner_prompt();
                        system_msg.content = planner_prompt;
                    }
                }
                crate::ai::multi_agent::tools::get_planner_tools()
            } else {
                tools.clone()
            };

            // Emit an iteration task for visibility (after first iteration)
            if iterations > 1 {
                if let Some(ref exec_id) = self.execution_tracker.get_execution_id(original_message.channel_id) {
                    let iter_task = self.execution_tracker.start_task(
                        original_message.channel_id,
                        exec_id,
                        Some(exec_id),
                        crate::models::TaskType::Thinking,
                        format!("Iteration {} - {}", iterations, orchestrator.current_mode().label()),
                        Some(&format!("Processing iteration {}...", iterations)),
                    );
                    self.execution_tracker.complete_task(&iter_task);
                }
            }

            // Check if execution was cancelled (e.g., user sent /new or stop button)
            if self.execution_tracker.is_cancelled(original_message.channel_id) {
                log::info!("[ORCHESTRATED_LOOP] Execution cancelled by user, stopping loop");
                was_cancelled = true;
                break;
            }

            // Check for pending task deletions
            let pending_deletions = self.execution_tracker.take_pending_task_deletions(original_message.channel_id);
            for task_id in pending_deletions {
                let (deleted, was_current) = orchestrator.delete_task(task_id);
                if deleted {
                    log::info!("[ORCHESTRATED_LOOP] Deleted task {}", task_id);
                    // Broadcast the updated task queue
                    self.broadcast_task_queue_update(original_message.channel_id, orchestrator);

                    // If we deleted the current task, move to the next one
                    if was_current {
                        log::info!("[ORCHESTRATED_LOOP] Deleted task was the current task, moving to next");
                        // Pop next task if available
                        if let Some(next_task) = orchestrator.pop_next_task() {
                            log::info!(
                                "[ORCHESTRATED_LOOP] Starting next task after deletion: {} - {}",
                                next_task.id,
                                next_task.description
                            );
                            self.broadcast_task_status_change(
                                original_message.channel_id,
                                next_task.id,
                                "in_progress",
                                &next_task.description,
                            );
                            self.broadcast_task_queue_update(original_message.channel_id, orchestrator);
                        } else if orchestrator.task_queue_is_empty() || orchestrator.all_tasks_complete() {
                            // No more tasks, complete the session
                            log::info!("[ORCHESTRATED_LOOP] No more tasks after deletion, completing session");
                            orchestrator_complete = true;
                            if let Err(e) = self.db.update_session_completion_status(session_id, CompletionStatus::Complete) {
                                log::error!("[ORCHESTRATED_LOOP] Failed to update session completion status: {}", e);
                            }
                            self.broadcast_session_complete(original_message.channel_id, session_id);
                            break;
                        }
                    }
                } else {
                    log::warn!("[ORCHESTRATED_LOOP] Task {} not found for deletion", task_id);
                }
            }

            // Check if session was marked as complete (defensive check against infinite loops)
            // This catches cases where task_fully_completed was called but the loop didn't break
            if let Ok(Some(status)) = self.db.get_session_completion_status(session_id) {
                if status.should_stop() {
                    log::info!("[ORCHESTRATED_LOOP] Session status is {:?}, stopping loop", status);
                    // Mark orchestrator as complete to avoid misleading error messages
                    if status == CompletionStatus::Complete {
                        orchestrator_complete = true;
                    }
                    break;
                }
            }

            if iterations > max_tool_iterations {
                log::warn!("Orchestrated tool loop exceeded max iterations ({})", max_tool_iterations);
                break;
            }

            // === TASK PLANNER MODE (first iteration, planner not yet completed) ===
            // If planner just completed (define_tasks was called), pop first task and continue
            if orchestrator.context().planner_completed && orchestrator.context().task_queue.current_task().is_none() {
                if let Some(first_task) = orchestrator.pop_next_task() {
                    log::info!(
                        "[ORCHESTRATED_LOOP] Starting first task: {} - {}",
                        first_task.id,
                        first_task.description
                    );
                    self.broadcast_task_status_change(
                        original_message.channel_id,
                        first_task.id,
                        "in_progress",
                        &first_task.description,
                    );
                    // Broadcast full task queue update
                    self.broadcast_task_queue_update(original_message.channel_id, orchestrator);

                    // Broadcast mode change to assistant
                    self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
                        original_message.channel_id,
                        "assistant",
                        "Assistant",
                        Some("Executing tasks"),
                    ));

                    // Update tools for assistant mode
                    let subtype = orchestrator.current_subtype();
                    tools = self.tool_registry.get_tool_definitions_for_subtype(tool_config, subtype);
                    if let Some(skill_tool) = self.create_skill_tool_definition_for_subtype(subtype) {
                        tools.push(skill_tool);
                    }
                    tools.extend(orchestrator.get_mode_tools());

                    // Broadcast toolset update
                    self.broadcast_toolset_update(
                        original_message.channel_id,
                        "assistant",
                        subtype.as_str(),
                        &tools,
                    );

                    // Update system prompt for new mode with current task
                    if let Some(system_msg) = conversation.first_mut() {
                        if system_msg.role == MessageRole::System {
                            let orchestrator_prompt = orchestrator.get_system_prompt();
                            system_msg.content = format!(
                                "{}\n\n---\n\n{}",
                                orchestrator_prompt,
                                archetype.enhance_system_prompt(&messages[0].content, &tools)
                            );
                        }
                    }
                }
            }

            // Check for forced mode transition
            if let Some(transition) = orchestrator.check_forced_transition() {
                log::info!(
                    "[ORCHESTRATOR] Forced transition: {} → {} ({})",
                    transition.from, transition.to, transition.reason
                );

                // Emit a task for the mode transition
                if let Some(ref exec_id) = self.execution_tracker.get_execution_id(original_message.channel_id) {
                    let transition_task = self.execution_tracker.start_task(
                        original_message.channel_id,
                        exec_id,
                        Some(exec_id),
                        crate::models::TaskType::PlanMode,
                        format!("Switching to {} mode", transition.to.label()),
                        Some(&format!("Transitioning: {}", transition.reason)),
                    );
                    self.execution_tracker.complete_task(&transition_task);
                }

                self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
                    original_message.channel_id,
                    &transition.to.to_string(),
                    transition.to.label(),
                    Some(&transition.reason),
                ));

                // Update tools for new mode (using current subtype)
                let subtype = orchestrator.current_subtype();
                tools = self
                    .tool_registry
                    .get_tool_definitions_for_subtype(tool_config, subtype);
                if let Some(skill_tool) = self.create_skill_tool_definition_for_subtype(subtype) {
                    tools.push(skill_tool);
                }
                tools.extend(orchestrator.get_mode_tools());

                // Emit task for toolset update
                if let Some(ref exec_id) = self.execution_tracker.get_execution_id(original_message.channel_id) {
                    let toolset_task = self.execution_tracker.start_task(
                        original_message.channel_id,
                        exec_id,
                        Some(exec_id),
                        crate::models::TaskType::Loading,
                        format!("Loading {} tools for {} mode", tools.len(), subtype.label()),
                        Some("Configuring available tools..."),
                    );
                    self.execution_tracker.complete_task(&toolset_task);
                }

                // Broadcast toolset update
                self.broadcast_toolset_update(
                    original_message.channel_id,
                    &transition.to.to_string(),
                    subtype.as_str(),
                    &tools,
                );

                // Update system prompt for new mode
                if let Some(system_msg) = conversation.first_mut() {
                    if system_msg.role == MessageRole::System {
                        let orchestrator_prompt = orchestrator.get_system_prompt();
                        system_msg.content = format!(
                            "{}\n\n---\n\n{}",
                            orchestrator_prompt,
                            archetype.enhance_system_prompt(&messages[0].content, &tools)
                        );
                    }
                }
            }

            // Generate with native tool support and progress notifications
            let ai_response = match self.generate_with_progress(
                &client,
                conversation.clone(),
                tool_history.clone(),
                current_tools.clone(),
                original_message.channel_id,
            ).await {
                Ok(response) => response,
                Err(e) => {
                    // Check if this is a client error (4xx) that might be recoverable
                    if e.is_client_error() && iterations <= 2 {
                        if e.is_context_too_large() {
                            log::warn!(
                                "[ORCHESTRATED_LOOP] Context too large error ({}), clearing tool history ({} entries) and retrying",
                                e.status_code.unwrap_or(0),
                                tool_history.len()
                            );
                            let recovery_entry = crate::ai::types::handle_context_overflow(
                                &mut tool_history,
                                &iterations.to_string(),
                            );
                            tool_history.push(recovery_entry);
                            continue;
                        }

                        // Other client errors - add guidance but don't clear history
                        log::warn!(
                            "[ORCHESTRATED_LOOP] Client error ({}), feeding back to AI: {}",
                            e.status_code.unwrap_or(0),
                            e
                        );
                        tool_history.push(crate::ai::types::create_error_feedback(&e, &iterations.to_string()));
                        continue;
                    }

                    // AI generation failed - save summary of work done so far
                    let error_str = e.to_string();
                    if !tool_call_log.is_empty() {
                        let summary = format!(
                            "[Session interrupted by error. Work completed before failure:]\n{}\n\nError: {}",
                            tool_call_log.join("\n"),
                            error_str
                        );
                        log::info!("[ORCHESTRATED_LOOP] Saving error summary with {} tool calls", tool_call_log.len());
                        let _ = self.db.add_session_message(
                            session_id,
                            DbMessageRole::Assistant,
                            &summary,
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                    // Save context before returning error
                    let _ = self.db.save_agent_context(session_id, orchestrator.context());
                    return Err(error_str);
                }
            };

            log::info!(
                "[ORCHESTRATED_LOOP] Response - content_len: {}, tool_calls: {}",
                ai_response.content.len(),
                ai_response.tool_calls.len()
            );

            // Handle x402 payments
            if let Some(ref payment_info) = ai_response.x402_payment {
                self.broadcaster.broadcast(GatewayEvent::x402_payment(
                    original_message.channel_id,
                    &payment_info.amount,
                    &payment_info.amount_formatted,
                    &payment_info.asset,
                    &payment_info.pay_to,
                    payment_info.resource.as_deref(),
                ));
                let _ = self.db.record_x402_payment(
                    Some(original_message.channel_id),
                    None,
                    payment_info.resource.as_deref(),
                    &payment_info.amount,
                    &payment_info.amount_formatted,
                    &payment_info.asset,
                    &payment_info.pay_to,
                    payment_info.tx_hash.as_deref(),
                    &payment_info.status.to_string(),
                );
            }

            // If no tool calls, check if this is allowed
            if ai_response.tool_calls.is_empty() {
                // Check if the agent should have called tools but didn't
                if let Some((warning_msg, attempt)) = orchestrator.check_tool_call_required() {
                    log::warn!(
                        "[ORCHESTRATED_LOOP] Agent skipped tool calls (attempt {}/5), forcing back into loop",
                        attempt
                    );

                    // Broadcast warning to UI so user has visibility
                    self.broadcaster.broadcast(GatewayEvent::agent_warning(
                        original_message.channel_id,
                        "no_tool_calls",
                        &format!(
                            "Agent tried to respond without calling tools (attempt {}/5). Forcing retry...",
                            attempt
                        ),
                        attempt,
                    ));

                    // Add a system message telling the agent to call tools
                    conversation.push(Message {
                        role: MessageRole::Assistant,
                        content: ai_response.content.clone(),
                    });
                    conversation.push(Message {
                        role: MessageRole::User,
                        content: format!(
                            "[SYSTEM ERROR] {}\n\nYou MUST call tools to gather information. Do not respond with made-up data.",
                            warning_msg
                        ),
                    });

                    // Continue the loop to force tool calling
                    continue;
                }

                if orchestrator_complete {
                    let response = if tool_call_log.is_empty() {
                        format!("{}\n\n{}", final_summary, ai_response.content)
                    } else {
                        let tool_log_text = tool_call_log.join("\n");
                        format!("{}\n\n{}\n\n{}", tool_log_text, final_summary, ai_response.content)
                    };
                    return Ok(response);
                } else {
                    // No tool calls but not complete - return content as-is
                    if tool_call_log.is_empty() {
                        return Ok(ai_response.content);
                    } else {
                        let tool_log_text = tool_call_log.join("\n");
                        return Ok(format!("{}\n\n{}", tool_log_text, ai_response.content));
                    }
                }
            }

            // Process tool calls
            let mut tool_responses = Vec::new();
            for call in &ai_response.tool_calls {
                let args_pretty = serde_json::to_string_pretty(&call.arguments)
                    .unwrap_or_else(|_| call.arguments.to_string());

                log::info!(
                    "[TOOL_CALL] Agent calling tool '{}' with args:\n{}",
                    call.name,
                    args_pretty
                );

                tool_call_log.push(format!(
                    "🔧 **Tool Call:** `{}`\n```json\n{}\n```",
                    call.name,
                    args_pretty
                ));

                self.broadcaster.broadcast(GatewayEvent::agent_tool_call(
                    original_message.channel_id,
                    &call.name,
                    &call.arguments,
                ));

                // Save tool call to session
                let tool_call_content = format!(
                    "🔧 **Tool Call:** `{}`\n```json\n{}\n```",
                    call.name,
                    args_pretty
                );
                if let Err(e) = self.db.add_session_message(
                    session_id,
                    DbMessageRole::ToolCall,
                    &tool_call_content,
                    None,
                    Some(&call.name),
                    None,
                    None,
                ) {
                    log::error!("Failed to save tool call to session: {}", e);
                }

                // Check if this is an orchestrator tool
                let orchestrator_result = orchestrator.process_tool_result(&call.name, &call.arguments);

                match orchestrator_result {
                    OrchestratorResult::Complete(summary) => {
                        log::info!("[ORCHESTRATOR] Execution complete: {}", summary);
                        orchestrator_complete = true;
                        final_summary = summary.clone();
                        tool_responses.push(ToolResponse::success(
                            call.id.clone(),
                            format!("Execution complete: {}", summary),
                        ));
                    }
                    OrchestratorResult::ToolResult(result) => {
                        tool_responses.push(ToolResponse::success(call.id.clone(), result));
                    }
                    OrchestratorResult::Error(err) => {
                        tool_responses.push(ToolResponse::error(call.id.clone(), err));
                    }
                    OrchestratorResult::Continue => {
                        // Not an orchestrator tool, execute normally
                        // Broadcast that tool is starting execution
                        self.broadcaster.broadcast(GatewayEvent::tool_execution(
                            original_message.channel_id,
                            &call.name,
                            &call.arguments,
                        ));

                        let result = if call.name == "use_skill" {
                            // Execute skill and set active skill on orchestrator
                            let skill_result = self.execute_skill_tool(&call.arguments, Some(session_id)).await;

                            // Also set active skill directly on orchestrator (in-memory)
                            if skill_result.success {
                                if let Some(skill_name) = call.arguments.get("skill_name").and_then(|v| v.as_str()) {
                                    if let Ok(Some(skill)) = self.db.get_enabled_skill_by_name(skill_name) {
                                        let skills_dir = crate::config::skills_dir();
                                        let skill_base_dir = format!("{}/{}", skills_dir, skill.name);
                                        let instructions = skill.body.replace("{baseDir}", &skill_base_dir);

                                        let requires_tools = skill.requires_tools.clone();
                                        log::info!(
                                            "[SKILL] Activating skill '{}' with requires_tools: {:?}",
                                            skill.name,
                                            requires_tools
                                        );

                                        orchestrator.context_mut().active_skill = Some(crate::ai::multi_agent::types::ActiveSkill {
                                            name: skill.name,
                                            instructions,
                                            activated_at: chrono::Utc::now().to_rfc3339(),
                                            tool_calls_made: 0,
                                            requires_tools: requires_tools.clone(),
                                        });

                                        // Force-include required tools in the toolset
                                        if !requires_tools.is_empty() {
                                            let subtype = orchestrator.current_subtype();
                                            tools = self.tool_registry
                                                .get_tool_definitions_for_subtype_with_required(
                                                    tool_config,
                                                    subtype,
                                                    &requires_tools,
                                                );
                                            if let Some(skill_tool) = self.create_skill_tool_definition_for_subtype(subtype) {
                                                tools.push(skill_tool);
                                            }
                                            tools.extend(orchestrator.get_mode_tools());
                                            log::info!(
                                                "[SKILL] Refreshed toolset with {} tools (including {} required by skill)",
                                                tools.len(),
                                                requires_tools.len()
                                            );
                                        }
                                    }
                                }
                            }
                            skill_result
                        } else {
                            // Check if subtype is None - only allow set_agent_subtype in that case
                            let current_subtype = orchestrator.current_subtype();
                            if !current_subtype.is_selected() && call.name != "set_agent_subtype" {
                                log::warn!(
                                    "[SUBTYPE] Blocked tool '{}' - no subtype selected. Must call set_agent_subtype first.",
                                    call.name
                                );
                                crate::tools::ToolResult::error(format!(
                                    "❌ No toolbox selected! You MUST call `set_agent_subtype` FIRST before using '{}'.\n\n\
                                    Choose based on the user's request:\n\
                                    • set_agent_subtype(subtype=\"finance\") - for crypto/DeFi operations\n\
                                    • set_agent_subtype(subtype=\"code_engineer\") - for code/git operations\n\
                                    • set_agent_subtype(subtype=\"secretary\") - for social/messaging",
                                    call.name
                                ))
                            } else {
                                // Run tool validators before execution
                                if let Some(ref validator_registry) = self.validator_registry {
                                    let validation_ctx = crate::tool_validators::ValidationContext::new(
                                        call.name.clone(),
                                        call.arguments.clone(),
                                        Arc::new(tool_context.clone()),
                                    );
                                    let validation_result = validator_registry.validate(&validation_ctx).await;
                                    if let Some(error_msg) = validation_result.to_error_message() {
                                        crate::tools::ToolResult::error(error_msg)
                                    } else {
                                        // Execute regular tool and record the call for skill tracking
                                        let tool_result = self.tool_registry
                                            .execute(&call.name, call.arguments.clone(), tool_context, Some(tool_config))
                                            .await;

                                        // Record this tool call for active skill tracking
                                        if tool_result.success {
                                            orchestrator.record_tool_call(&call.name);
                                        }

                                        tool_result
                                    }
                                } else {
                                    // Execute regular tool and record the call for skill tracking
                                    let tool_result = self.tool_registry
                                        .execute(&call.name, call.arguments.clone(), tool_context, Some(tool_config))
                                        .await;

                                    // Record this tool call for active skill tracking
                                    if tool_result.success {
                                        orchestrator.record_tool_call(&call.name);
                                    }

                                    tool_result
                                }
                            }
                        };

                        // Handle subtype change: update orchestrator and refresh tools
                        if call.name == "set_agent_subtype" && result.success {
                            if let Some(subtype_str) = call.arguments.get("subtype").and_then(|v| v.as_str()) {
                                if let Some(new_subtype) = AgentSubtype::from_str(subtype_str) {
                                    orchestrator.set_subtype(new_subtype);
                                    log::info!(
                                        "[SUBTYPE] Changed to {} mode",
                                        new_subtype.label()
                                    );

                                    // Refresh tools for new subtype
                                    tools = self
                                        .tool_registry
                                        .get_tool_definitions_for_subtype(tool_config, new_subtype);
                                    if let Some(skill_tool) =
                                        self.create_skill_tool_definition_for_subtype(new_subtype)
                                    {
                                        tools.push(skill_tool);
                                    }
                                    tools.extend(orchestrator.get_mode_tools());

                                    // Broadcast toolset update
                                    self.broadcast_toolset_update(
                                        original_message.channel_id,
                                        &orchestrator.current_mode().to_string(),
                                        new_subtype.as_str(),
                                        &tools,
                                    );
                                }
                            }
                        }

                        // Handle retry backoff
                        let result = if let Some(retry_secs) = result.retry_after_secs {
                            self.broadcaster.broadcast(GatewayEvent::tool_waiting(
                                original_message.channel_id,
                                &call.name,
                                retry_secs,
                            ));
                            tokio::time::sleep(std::time::Duration::from_secs(retry_secs)).await;
                            crate::tools::ToolResult::error(format!(
                                "{}\n\n🔄 Paused for {} seconds. Please retry.",
                                result.error.unwrap_or_else(|| "Unknown error".to_string()),
                                retry_secs
                            ))
                        } else {
                            result
                        };

                        // Check if this tool requires user response (e.g., ask_user)
                        // If so, we should break the loop after processing to wait for user input
                        if let Some(metadata) = &result.metadata {
                            if metadata.get("requires_user_response").and_then(|v| v.as_bool()).unwrap_or(false) {
                                waiting_for_user_response = true;
                                user_question_content = result.content.clone();
                                log::info!("[ORCHESTRATED_LOOP] Tool requires user response, will break after processing");
                            }
                            // Check if task_fully_completed was called - agent signals it's done
                            // CRITICAL: This tool MUST stop the loop immediately to prevent infinite iteration
                            if metadata.get("task_fully_completed").and_then(|v| v.as_bool()).unwrap_or(false) {
                                let summary = metadata.get("summary")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&result.content)
                                    .to_string();

                                log::info!("[ORCHESTRATED_LOOP] task_fully_completed called, stopping loop");

                                // Mark current task as completed and broadcast (if task queue exists)
                                if let Some(completed_task_id) = orchestrator.complete_current_task() {
                                    log::info!("[ORCHESTRATED_LOOP] Task {} completed", completed_task_id);
                                    self.broadcast_task_status_change(
                                        original_message.channel_id,
                                        completed_task_id,
                                        "completed",
                                        &summary,
                                    );
                                }

                                // ALWAYS stop the loop when task_fully_completed is called
                                // The agent explicitly signaled it's done - respect that signal
                                orchestrator_complete = true;
                                final_summary = summary.clone();

                                // Mark session as complete in database
                                if let Err(e) = self.db.update_session_completion_status(session_id, CompletionStatus::Complete) {
                                    log::error!("[ORCHESTRATED_LOOP] Failed to update session completion status: {}", e);
                                }

                                // Broadcast session complete event
                                self.broadcast_session_complete(original_message.channel_id, session_id);
                            }
                        }

                        // Extract duration_ms from metadata if available
                        let duration_ms = result.metadata.as_ref()
                            .and_then(|m| m.get("duration_ms"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);

                        self.broadcaster.broadcast(GatewayEvent::tool_result(
                            original_message.channel_id,
                            &call.name,
                            result.success,
                            duration_ms,
                            &result.content,
                        ));

                        // Execute AfterToolCall hooks (for auto-memory, etc.)
                        if let Some(hook_manager) = &self.hook_manager {
                            use crate::hooks::{HookContext, HookEvent, HookResult};
                            let mut hook_context = HookContext::new(HookEvent::AfterToolCall)
                                .with_channel(original_message.channel_id, Some(session_id))
                                .with_tool(call.name.clone(), call.arguments.clone())
                                .with_tool_result(serde_json::json!({
                                    "success": result.success,
                                    "content": result.content,
                                }));
                            let hook_result = hook_manager.execute(HookEvent::AfterToolCall, &mut hook_context).await;
                            if let HookResult::Error(e) = hook_result {
                                log::warn!("Hook execution failed for tool '{}': {}", call.name, e);
                            }
                        }

                        // Save tool result to session
                        let tool_result_content = format!(
                            "**{}:** {}\n{}",
                            if result.success { "Result" } else { "Error" },
                            call.name,
                            result.content
                        );
                        if let Err(e) = self.db.add_session_message(
                            session_id,
                            DbMessageRole::ToolResult,
                            &tool_result_content,
                            None,
                            Some(&call.name),
                            None,
                            None,
                        ) {
                            log::error!("Failed to save tool result to session: {}", e);
                        }

                        tool_responses.push(if result.success {
                            ToolResponse::success(call.id.clone(), result.content)
                        } else {
                            ToolResponse::error(call.id.clone(), result.content)
                        });
                    }
                }

                // Broadcast task list update after any orchestrator tool processing
                self.broadcast_tasks_update(original_message.channel_id, orchestrator);
            }

            // Add to tool history (keep only last N entries to prevent context bloat)
            const MAX_TOOL_HISTORY: usize = 10;
            tool_history.push(ToolHistoryEntry::new(
                ai_response.tool_calls,
                tool_responses,
            ));
            if tool_history.len() > MAX_TOOL_HISTORY {
                // Remove oldest entries, keeping the most recent
                tool_history.drain(0..tool_history.len() - MAX_TOOL_HISTORY);
            }

            // If orchestrator is complete, break the loop
            if orchestrator_complete {
                break;
            }

            // If a tool requires user response (e.g., ask_user), break the loop
            // and return the question content. Context is preserved for when user responds.
            if waiting_for_user_response {
                log::info!("[ORCHESTRATED_LOOP] Breaking loop to wait for user response");
                break;
            }
        }

        // Save orchestrator context for next turn
        if let Err(e) = self.db.save_agent_context(session_id, orchestrator.context()) {
            log::warn!("[MULTI_AGENT] Failed to save context for session {}: {}", session_id, e);
        }

        // If cancelled with work done, save a summary so context is preserved on resume
        if was_cancelled && !tool_call_log.is_empty() {
            let summary = format!(
                "[Session stopped by user. Work completed before stop:]\n{}",
                tool_call_log.join("\n")
            );
            log::info!("[ORCHESTRATED_LOOP] Saving cancellation summary with {} tool calls", tool_call_log.len());
            if let Err(e) = self.db.add_session_message(
                session_id,
                DbMessageRole::Assistant,
                &summary,
                None,
                None,
                None,
                None,
            ) {
                log::error!("Failed to save cancellation summary: {}", e);
            }
        }

        // Return final response
        if waiting_for_user_response {
            // Save the tool call log to the orchestrator context so the AI knows what it already did
            // This will be included in the system prompt on the next turn
            if !tool_call_log.is_empty() {
                let context_summary = format!(
                    "Before asking the user, I already completed these actions:\n{}",
                    tool_call_log.join("\n")
                );
                orchestrator.context_mut().waiting_for_user_context = Some(context_summary);
                // Re-save context with the waiting_for_user_context
                if let Err(e) = self.db.save_agent_context(session_id, orchestrator.context()) {
                    log::warn!("[MULTI_AGENT] Failed to save context with user_context: {}", e);
                }
            }
            // Return the question content - context is saved, will continue when user responds
            Ok(user_question_content)
        } else if orchestrator_complete {
            Ok(final_summary)
        } else if tool_call_log.is_empty() {
            Err(format!(
                "Tool loop hit max iterations ({}) without completion",
                max_tool_iterations
            ))
        } else {
            // Max iterations with work done - save summary so context is preserved
            let summary = format!(
                "[Session hit max iterations. Work completed before limit:]\n{}",
                tool_call_log.join("\n")
            );
            log::info!("[ORCHESTRATED_LOOP] Saving max-iterations summary with {} tool calls", tool_call_log.len());
            let _ = self.db.add_session_message(
                session_id,
                DbMessageRole::Assistant,
                &summary,
                None,
                None,
                None,
                None,
            );
            Err(format!(
                "Tool loop hit max iterations ({}). Work has been saved.",
                max_tool_iterations
            ))
        }
    }

    /// Generate response using text-based tool calling with multi-agent orchestration
    async fn generate_with_text_tools_orchestrated(
        &self,
        client: &AiClient,
        messages: Vec<Message>,
        mut tools: Vec<ToolDefinition>,
        tool_config: &ToolConfig,
        tool_context: &ToolContext,
        original_message: &NormalizedMessage,
        archetype: &dyn ModelArchetype,
        orchestrator: &mut Orchestrator,
        session_id: i64,
    ) -> Result<String, String> {
        // Get max tool iterations from bot settings
        let max_tool_iterations = self.db.get_bot_settings()
            .map(|s| s.max_tool_iterations as usize)
            .unwrap_or(FALLBACK_MAX_TOOL_ITERATIONS);

        // Build conversation with orchestrator's system prompt
        let mut conversation = messages.clone();
        if let Some(system_msg) = conversation.first_mut() {
            if system_msg.role == MessageRole::System {
                let orchestrator_prompt = orchestrator.get_system_prompt();
                system_msg.content = format!(
                    "{}\n\n---\n\n{}",
                    orchestrator_prompt,
                    archetype.enhance_system_prompt(&system_msg.content, &tools)
                );
            }
        }

        // Clear waiting_for_user_context now that it's been consumed into the prompt
        orchestrator.clear_waiting_for_user_context();

        let mut final_response = String::new();
        let mut iterations = 0;
        let mut tool_call_log: Vec<String> = Vec::new();
        let mut orchestrator_complete = false;
        let mut waiting_for_user_response = false;
        let mut user_question_content = String::new();
        let mut was_cancelled = false;

        loop {
            iterations += 1;
            log::info!(
                "[TEXT_ORCHESTRATED] Iteration {} in {} mode",
                iterations,
                orchestrator.current_mode()
            );

            // Check if execution was cancelled (e.g., user sent /new or stop button)
            if self.execution_tracker.is_cancelled(original_message.channel_id) {
                log::info!("[TEXT_ORCHESTRATED] Execution cancelled by user, stopping loop");
                was_cancelled = true;
                break;
            }

            // Check if session was marked as complete (defensive check against infinite loops)
            if let Ok(Some(status)) = self.db.get_session_completion_status(session_id) {
                if status.should_stop() {
                    log::info!("[TEXT_ORCHESTRATED] Session status is {:?}, stopping loop", status);
                    // Mark orchestrator as complete to avoid misleading error messages
                    if status == CompletionStatus::Complete {
                        orchestrator_complete = true;
                    }
                    break;
                }
            }

            if iterations > max_tool_iterations {
                log::warn!("Text orchestrated loop exceeded max iterations ({})", max_tool_iterations);
                break;
            }

            // Check for forced mode transition
            if let Some(transition) = orchestrator.check_forced_transition() {
                self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
                    original_message.channel_id,
                    &transition.to.to_string(),
                    transition.to.label(),
                    Some(&transition.reason),
                ));

                // Update tools (using current subtype)
                let subtype = orchestrator.current_subtype();
                tools = self
                    .tool_registry
                    .get_tool_definitions_for_subtype(tool_config, subtype);
                if let Some(skill_tool) = self.create_skill_tool_definition_for_subtype(subtype) {
                    tools.push(skill_tool);
                }
                tools.extend(orchestrator.get_mode_tools());

                // Broadcast toolset update
                self.broadcast_toolset_update(
                    original_message.channel_id,
                    &transition.to.to_string(),
                    subtype.as_str(),
                    &tools,
                );

                // Update system prompt
                if let Some(system_msg) = conversation.first_mut() {
                    if system_msg.role == MessageRole::System {
                        let orchestrator_prompt = orchestrator.get_system_prompt();
                        system_msg.content = format!(
                            "{}\n\n---\n\n{}",
                            orchestrator_prompt,
                            archetype.enhance_system_prompt(&messages[0].content, &tools)
                        );
                    }
                }
            }

            let (ai_content, payment) = match client.generate_text_with_events(
                conversation.clone(),
                &self.broadcaster,
                original_message.channel_id,
            ).await {
                Ok(result) => result,
                Err(e) => {
                    // AI generation failed - save summary of work done so far
                    if !tool_call_log.is_empty() {
                        let summary = format!(
                            "[Session interrupted by error. Work completed before failure:]\n{}\n\nError: {}",
                            tool_call_log.join("\n"),
                            e
                        );
                        log::info!("[TEXT_ORCHESTRATED] Saving error summary with {} tool calls", tool_call_log.len());
                        let _ = self.db.add_session_message(
                            session_id,
                            DbMessageRole::Assistant,
                            &summary,
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                    // Save context before returning error
                    let _ = self.db.save_agent_context(session_id, orchestrator.context());
                    return Err(e);
                }
            };

            if let Some(ref payment_info) = payment {
                let _ = self.db.record_x402_payment(
                    Some(original_message.channel_id),
                    None,
                    payment_info.resource.as_deref(),
                    &payment_info.amount,
                    &payment_info.amount_formatted,
                    &payment_info.asset,
                    &payment_info.pay_to,
                    payment_info.tx_hash.as_deref(),
                    &payment_info.status.to_string(),
                );
            }

            let parsed = archetype.parse_response(&ai_content);

            match parsed {
                Some(agent_response) => {
                    if let Some(tool_call) = agent_response.tool_call {
                        let args_pretty = serde_json::to_string_pretty(&tool_call.tool_params)
                            .unwrap_or_else(|_| tool_call.tool_params.to_string());

                        tool_call_log.push(format!(
                            "🔧 **Tool Call:** `{}`\n```json\n{}\n```",
                            tool_call.tool_name,
                            args_pretty
                        ));

                        self.broadcaster.broadcast(GatewayEvent::agent_tool_call(
                            original_message.channel_id,
                            &tool_call.tool_name,
                            &tool_call.tool_params,
                        ));

                        // Save tool call to session
                        let tool_call_content = format!(
                            "🔧 **Tool Call:** `{}`\n```json\n{}\n```",
                            tool_call.tool_name,
                            args_pretty
                        );
                        if let Err(e) = self.db.add_session_message(
                            session_id,
                            DbMessageRole::ToolCall,
                            &tool_call_content,
                            None,
                            Some(&tool_call.tool_name),
                            None,
                            None,
                        ) {
                            log::error!("Failed to save tool call to session: {}", e);
                        }

                        // Check if orchestrator tool
                        let orchestrator_result = orchestrator.process_tool_result(
                            &tool_call.tool_name,
                            &tool_call.tool_params,
                        );

                        let tool_result_content = match orchestrator_result {
                            OrchestratorResult::Complete(summary) => {
                                orchestrator_complete = true;
                                final_response = summary.clone();
                                format!("Execution complete: {}", summary)
                            }
                            OrchestratorResult::ToolResult(result) => result,
                            OrchestratorResult::Error(err) => format!("Error: {}", err),
                            OrchestratorResult::Continue => {
                                // Execute regular tool
                                let result = if tool_call.tool_name == "use_skill" {
                                    // Execute skill and set active skill on orchestrator
                                    let skill_result = self.execute_skill_tool(&tool_call.tool_params, Some(session_id)).await;

                                    // Also set active skill directly on orchestrator (in-memory)
                                    if skill_result.success {
                                        if let Some(skill_name) = tool_call.tool_params.get("skill_name").and_then(|v| v.as_str()) {
                                            if let Ok(Some(skill)) = self.db.get_enabled_skill_by_name(skill_name) {
                                                let skills_dir = crate::config::skills_dir();
                                                let skill_base_dir = format!("{}/{}", skills_dir, skill.name);
                                                let instructions = skill.body.replace("{baseDir}", &skill_base_dir);

                                                let requires_tools = skill.requires_tools.clone();
                                                log::info!(
                                                    "[SKILL] Activating skill '{}' with requires_tools: {:?}",
                                                    skill.name,
                                                    requires_tools
                                                );

                                                orchestrator.context_mut().active_skill = Some(crate::ai::multi_agent::types::ActiveSkill {
                                                    name: skill.name,
                                                    instructions,
                                                    activated_at: chrono::Utc::now().to_rfc3339(),
                                                    tool_calls_made: 0,
                                                    requires_tools: requires_tools.clone(),
                                                });

                                                // Force-include required tools in the toolset
                                                if !requires_tools.is_empty() {
                                                    let subtype = orchestrator.current_subtype();
                                                    tools = self.tool_registry
                                                        .get_tool_definitions_for_subtype_with_required(
                                                            tool_config,
                                                            subtype,
                                                            &requires_tools,
                                                        );
                                                    if let Some(skill_tool) = self.create_skill_tool_definition_for_subtype(subtype) {
                                                        tools.push(skill_tool);
                                                    }
                                                    tools.extend(orchestrator.get_mode_tools());
                                                    log::info!(
                                                        "[SKILL] Refreshed toolset with {} tools (including {} required by skill)",
                                                        tools.len(),
                                                        requires_tools.len()
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    skill_result
                                } else {
                                    // Check if subtype is None - only allow set_agent_subtype in that case
                                    let current_subtype = orchestrator.current_subtype();
                                    if !current_subtype.is_selected() && tool_call.tool_name != "set_agent_subtype" {
                                        log::warn!(
                                            "[SUBTYPE] Blocked tool '{}' - no subtype selected. Must call set_agent_subtype first.",
                                            tool_call.tool_name
                                        );
                                        crate::tools::ToolResult::error(format!(
                                            "❌ No toolbox selected! You MUST call `set_agent_subtype` FIRST before using '{}'.\n\n\
                                            Choose based on the user's request:\n\
                                            • set_agent_subtype(subtype=\"finance\") - for crypto/DeFi operations\n\
                                            • set_agent_subtype(subtype=\"code_engineer\") - for code/git operations\n\
                                            • set_agent_subtype(subtype=\"secretary\") - for social/messaging",
                                            tool_call.tool_name
                                        ))
                                    } else {
                                        // Run tool validators before execution
                                        if let Some(ref validator_registry) = self.validator_registry {
                                            let validation_ctx = crate::tool_validators::ValidationContext::new(
                                                tool_call.tool_name.clone(),
                                                tool_call.tool_params.clone(),
                                                Arc::new(tool_context.clone()),
                                            );
                                            let validation_result = validator_registry.validate(&validation_ctx).await;
                                            if let Some(error_msg) = validation_result.to_error_message() {
                                                crate::tools::ToolResult::error(error_msg)
                                            } else {
                                                // Execute regular tool and record the call for skill tracking
                                                let tool_result = self.tool_registry.execute(
                                                    &tool_call.tool_name,
                                                    tool_call.tool_params.clone(),
                                                    tool_context,
                                                    Some(tool_config),
                                                ).await;

                                                // Record this tool call for active skill tracking
                                                if tool_result.success {
                                                    orchestrator.record_tool_call(&tool_call.tool_name);
                                                }

                                                tool_result
                                            }
                                        } else {
                                            // Execute regular tool and record the call for skill tracking
                                            let tool_result = self.tool_registry.execute(
                                                &tool_call.tool_name,
                                                tool_call.tool_params.clone(),
                                                tool_context,
                                                Some(tool_config),
                                            ).await;

                                            // Record this tool call for active skill tracking
                                            if tool_result.success {
                                                orchestrator.record_tool_call(&tool_call.tool_name);
                                            }

                                            tool_result
                                        }
                                    }
                                };

                                // Handle subtype change: update orchestrator and refresh tools
                                if tool_call.tool_name == "set_agent_subtype" && result.success {
                                    if let Some(subtype_str) = tool_call.tool_params.get("subtype").and_then(|v| v.as_str()) {
                                        if let Some(new_subtype) = AgentSubtype::from_str(subtype_str) {
                                            orchestrator.set_subtype(new_subtype);
                                            log::info!(
                                                "[SUBTYPE] Changed to {} mode",
                                                new_subtype.label()
                                            );

                                            // Refresh tools for new subtype
                                            tools = self
                                                .tool_registry
                                                .get_tool_definitions_for_subtype(tool_config, new_subtype);
                                            if let Some(skill_tool) =
                                                self.create_skill_tool_definition_for_subtype(new_subtype)
                                            {
                                                tools.push(skill_tool);
                                            }
                                            tools.extend(orchestrator.get_mode_tools());

                                            // Broadcast toolset update
                                            self.broadcast_toolset_update(
                                                original_message.channel_id,
                                                &orchestrator.current_mode().to_string(),
                                                new_subtype.as_str(),
                                                &tools,
                                            );
                                        }
                                    }
                                }

                                // Check if this tool requires user response (e.g., ask_user)
                                if let Some(metadata) = &result.metadata {
                                    if metadata.get("requires_user_response").and_then(|v| v.as_bool()).unwrap_or(false) {
                                        waiting_for_user_response = true;
                                        user_question_content = result.content.clone();
                                        log::info!("[TEXT_ORCHESTRATED] Tool requires user response, will break after processing");
                                    }
                                    // Check if task_fully_completed was called - agent signals it's done
                                    if metadata.get("task_fully_completed").and_then(|v| v.as_bool()).unwrap_or(false) {
                                        orchestrator_complete = true;
                                        if let Some(summary) = metadata.get("summary").and_then(|v| v.as_str()) {
                                            final_response = summary.to_string();
                                        } else {
                                            final_response = result.content.clone();
                                        }
                                        log::info!("[TEXT_ORCHESTRATED] Task fully completed signal received");
                                    }
                                }

                                // Extract duration_ms from metadata if available
                                let duration_ms = result.metadata.as_ref()
                                    .and_then(|m| m.get("duration_ms"))
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);

                                self.broadcaster.broadcast(GatewayEvent::tool_result(
                                    original_message.channel_id,
                                    &tool_call.tool_name,
                                    result.success,
                                    duration_ms,
                                    &result.content,
                                ));

                                // Execute AfterToolCall hooks (for auto-memory, etc.)
                                if let Some(hook_manager) = &self.hook_manager {
                                    use crate::hooks::{HookContext, HookEvent, HookResult};
                                    let mut hook_context = HookContext::new(HookEvent::AfterToolCall)
                                        .with_channel(original_message.channel_id, Some(session_id))
                                        .with_tool(tool_call.tool_name.clone(), tool_call.tool_params.clone())
                                        .with_tool_result(serde_json::json!({
                                            "success": result.success,
                                            "content": result.content,
                                        }));
                                    let hook_result = hook_manager.execute(HookEvent::AfterToolCall, &mut hook_context).await;
                                    if let HookResult::Error(e) = hook_result {
                                        log::warn!("Hook execution failed for tool '{}': {}", tool_call.tool_name, e);
                                    }
                                }

                                // Save tool result to session
                                let tool_result_msg = format!(
                                    "**{}:** {}\n{}",
                                    if result.success { "Result" } else { "Error" },
                                    tool_call.tool_name,
                                    result.content
                                );
                                if let Err(e) = self.db.add_session_message(
                                    session_id,
                                    DbMessageRole::ToolResult,
                                    &tool_result_msg,
                                    None,
                                    Some(&tool_call.tool_name),
                                    None,
                                    None,
                                ) {
                                    log::error!("Failed to save tool result to session: {}", e);
                                }

                                result.content
                            }
                        };

                        // Broadcast task list update after any orchestrator tool processing
                        self.broadcast_tasks_update(original_message.channel_id, orchestrator);

                        // Add to conversation
                        conversation.push(Message {
                            role: MessageRole::Assistant,
                            content: ai_content.clone(),
                        });
                        conversation.push(Message {
                            role: MessageRole::User,
                            content: archetype.format_tool_followup(
                                &tool_call.tool_name,
                                &tool_result_content,
                                true,
                            ),
                        });

                        // Truncate conversation to prevent context bloat
                        // Keep system prompt(s) at start + last N message pairs
                        const MAX_CONVERSATION_MESSAGES: usize = 20;
                        let system_count = conversation.iter()
                            .take_while(|m| m.role == MessageRole::System)
                            .count();
                        if conversation.len() > system_count + MAX_CONVERSATION_MESSAGES {
                            let remove_count = conversation.len() - system_count - MAX_CONVERSATION_MESSAGES;
                            conversation.drain(system_count..system_count + remove_count);
                        }

                        if orchestrator_complete {
                            break;
                        }
                        // If a tool requires user response (e.g., ask_user), break the loop
                        if waiting_for_user_response {
                            log::info!("[TEXT_ORCHESTRATED] Breaking loop to wait for user response");
                            break;
                        }
                        continue;
                    } else {
                        // No tool call - check if this is allowed
                        if let Some((warning_msg, attempt)) = orchestrator.check_tool_call_required() {
                            log::warn!(
                                "[TEXT_ORCHESTRATED] Agent skipped tool calls (attempt {}/5), forcing back into loop",
                                attempt
                            );

                            // Broadcast warning to UI so user has visibility
                            self.broadcaster.broadcast(GatewayEvent::agent_warning(
                                original_message.channel_id,
                                "no_tool_calls",
                                &format!(
                                    "Agent tried to respond without calling tools (attempt {}/5). Forcing retry...",
                                    attempt
                                ),
                                attempt,
                            ));

                            // Add messages to force tool calling
                            conversation.push(Message {
                                role: MessageRole::Assistant,
                                content: agent_response.body.clone(),
                            });
                            conversation.push(Message {
                                role: MessageRole::User,
                                content: format!(
                                    "[SYSTEM ERROR] {}\n\nYou MUST call tools to gather information. Do not respond with made-up data.",
                                    warning_msg
                                ),
                            });

                            // Continue the loop to force tool calling
                            continue;
                        }

                        if tool_call_log.is_empty() {
                            final_response = agent_response.body;
                        } else {
                            let tool_log_text = tool_call_log.join("\n");
                            final_response = format!("{}\n\n{}", tool_log_text, agent_response.body);
                        }
                        break;
                    }
                }
                None => {
                    // Broadcast that parsing failed - show the raw AI content for debugging
                    log::warn!("[TEXT_ORCHESTRATED] Failed to parse AI response, using raw content");
                    self.broadcaster.broadcast(GatewayEvent::agent_thinking(
                        original_message.channel_id,
                        &format!("Parse failed, raw AI response:\n{}", &ai_content[..ai_content.len().min(500)]),
                    ));

                    if tool_call_log.is_empty() {
                        final_response = ai_content;
                    } else {
                        let tool_log_text = tool_call_log.join("\n");
                        final_response = format!("{}\n\n{}", tool_log_text, ai_content);
                    }
                    break;
                }
            }
        }

        // Save orchestrator context for next turn
        if let Err(e) = self.db.save_agent_context(session_id, orchestrator.context()) {
            log::warn!("[MULTI_AGENT] Failed to save context for session {}: {}", session_id, e);
        }

        // If cancelled with work done, save a summary so context is preserved on resume
        if was_cancelled && !tool_call_log.is_empty() {
            let summary = format!(
                "[Session stopped by user. Work completed before stop:]\n{}",
                tool_call_log.join("\n")
            );
            log::info!("[TEXT_ORCHESTRATED] Saving cancellation summary with {} tool calls", tool_call_log.len());
            if let Err(e) = self.db.add_session_message(
                session_id,
                DbMessageRole::Assistant,
                &summary,
                None,
                None,
                None,
                None,
            ) {
                log::error!("Failed to save cancellation summary: {}", e);
            }
        }

        // If waiting for user response, save context and return the question content
        if waiting_for_user_response {
            // Save the tool call log to the orchestrator context so the AI knows what it already did
            if !tool_call_log.is_empty() {
                let context_summary = format!(
                    "Before asking the user, I already completed these actions:\n{}",
                    tool_call_log.join("\n")
                );
                orchestrator.context_mut().waiting_for_user_context = Some(context_summary);
                // Re-save context with the waiting_for_user_context
                if let Err(e) = self.db.save_agent_context(session_id, orchestrator.context()) {
                    log::warn!("[MULTI_AGENT] Failed to save context with user_context: {}", e);
                }
            }
            return Ok(user_question_content);
        }

        if final_response.is_empty() {
            // Empty response with work done - save summary
            if !tool_call_log.is_empty() {
                let summary = format!(
                    "[Session ended with empty response. Work completed:]\n{}",
                    tool_call_log.join("\n")
                );
                log::info!("[TEXT_ORCHESTRATED] Saving empty-response summary with {} tool calls", tool_call_log.len());
                let _ = self.db.add_session_message(
                    session_id,
                    DbMessageRole::Assistant,
                    &summary,
                    None,
                    None,
                    None,
                    None,
                );
            }
            return Err("AI returned empty response".to_string());
        }

        Ok(final_response)
    }

    /// Execute the special "use_skill" tool
    /// If session_id is provided, saves the active skill to the agent context for persistence
    async fn execute_skill_tool(&self, params: &Value, session_id: Option<i64>) -> crate::tools::ToolResult {
        use crate::ai::multi_agent::types::ActiveSkill;

        let skill_name = params.get("skill_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let input = params.get("input")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        log::info!("[SKILL] Executing skill '{}' with input: {}", skill_name, input);

        // Look up the specific skill by name (more efficient than loading all skills)
        let skill = match self.db.get_enabled_skill_by_name(skill_name) {
            Ok(s) => s,
            Err(e) => {
                return crate::tools::ToolResult::error(format!("Failed to load skill: {}", e));
            }
        };

        match skill {
            Some(skill) => {
                // Determine the skills directory path
                let skills_dir = crate::config::skills_dir();
                let skill_base_dir = format!("{}/{}", skills_dir, skill.name);

                // Replace {baseDir} placeholder with actual skill directory
                let instructions = if !skill.body.is_empty() {
                    skill.body.replace("{baseDir}", &skill_base_dir)
                } else {
                    String::new()
                };

                // Save active skill to agent context for persistence
                if let Some(sid) = session_id {
                    if let Ok(Some(mut context)) = self.db.get_agent_context(sid) {
                        context.active_skill = Some(ActiveSkill {
                            name: skill.name.clone(),
                            instructions: instructions.clone(),
                            activated_at: chrono::Utc::now().to_rfc3339(),
                            tool_calls_made: 0, // Reset counter - agent must call actual tools
                            requires_tools: skill.requires_tools.clone(),
                        });
                        if let Err(e) = self.db.save_agent_context(sid, &context) {
                            log::warn!("[SKILL] Failed to save active skill to context: {}", e);
                        } else {
                            log::info!(
                                "[SKILL] Saved active skill '{}' to session {} (tool_calls_made=0, requires_tools={:?})",
                                skill.name, sid, skill.requires_tools
                            );
                        }
                    }
                }

                // Return the skill's instructions/body along with context
                let mut result = format!("## Skill: {}\n\n", skill.name);
                result.push_str(&format!("Description: {}\n\n", skill.description));

                if !instructions.is_empty() {
                    result.push_str("### Instructions:\n");
                    result.push_str(&instructions);
                    result.push_str("\n\n");
                }

                result.push_str(&format!("### User Query:\n{}\n\n", input));
                result.push_str("**IMPORTANT:** Now call the actual tools mentioned in the instructions above. Do NOT call use_skill again.");

                crate::tools::ToolResult::success(&result)
            }
            None => {
                // Fetch available skills for the error message
                let available = self.db.list_enabled_skills()
                    .map(|skills| skills.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_else(|_| "unknown".to_string());
                crate::tools::ToolResult::error(format!(
                    "Skill '{}' not found or not enabled. Available skills: {}",
                    skill_name,
                    available
                ))
            }
        }
    }

    /// Load SOUL.md content if it exists
    fn load_soul() -> Option<String> {
        // Try multiple locations for SOUL.md
        let paths = [
            "SOUL.md",
            "./SOUL.md",
            "/app/SOUL.md",
        ];

        for path in paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                log::debug!("[SOUL] Loaded from {}", path);
                return Some(content);
            }
        }

        log::debug!("[SOUL] No SOUL.md found, using default personality");
        None
    }

    /// Build the base system prompt with context from memories and user info
    /// Note: Tool-related instructions are added by the archetype's enhance_system_prompt
    fn build_system_prompt(
        &self,
        message: &NormalizedMessage,
        identity_id: &str,
        _tool_config: &ToolConfig,
    ) -> String {
        let mut prompt = String::new();

        // Load SOUL.md if available, otherwise use default intro
        if let Some(soul) = Self::load_soul() {
            prompt.push_str(&soul);
            prompt.push_str("\n\n");
        } else {
            prompt.push_str("You are StarkBot, an AI agent who can respond to users and operate tools.\n\n");
        }

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
                prompt.push_str("## User Context\n");
                for mem in memories {
                    prompt.push_str(&format!("- {}\n", mem.content));
                }
                prompt.push('\n');
            }
        }

        // Add recent session summaries (past conversations)
        if let Ok(summaries) = self.db.get_session_summaries(Some(identity_id), 3) {
            if !summaries.is_empty() {
                prompt.push_str("## Previous Sessions\n");
                for summary in summaries {
                    prompt.push_str(&format!("{}\n\n", summary.content));
                }
            }
        }

        // Phase 6: Add cross-session memories from other channels
        if self.memory_config.enable_cross_session_memory {
            if let Ok(cross_memories) = self.db.get_cross_channel_memories(
                identity_id,
                Some(&message.channel_type),
                self.memory_config.cross_session_memory_limit,
            ) {
                if !cross_memories.is_empty() {
                    prompt.push_str("## Context from Other Channels\n");
                    for mem in cross_memories {
                        let channel_label = mem.source_channel_type
                            .as_deref()
                            .unwrap_or("unknown");
                        let type_label = match mem.memory_type {
                            MemoryType::Preference => "preference",
                            MemoryType::Fact => "fact",
                            MemoryType::Task => "task",
                            _ => "memory",
                        };
                        prompt.push_str(&format!("- [{}:{}] {}\n", channel_label, type_label, mem.content));
                    }
                    prompt.push('\n');
                }
            }
        }

        // Add available API keys (so the agent knows what credentials are configured)
        if let Ok(keys) = self.db.list_api_keys() {
            if !keys.is_empty() {
                prompt.push_str("## Available API Keys\n");
                prompt.push_str("The following API keys are configured and available as environment variables when using the exec tool:\n");
                for key in &keys {
                    prompt.push_str(&format!("- ${}\n", key.service_name));
                }
                prompt.push('\n');
            }
        }

        // Add context
        prompt.push_str(&format!(
            "## Current Request\nUser: {} | Channel: {}\n",
            message.user_name, message.channel_type
        ));

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

        for marker in &self.memory_markers {
            for cap in marker.pattern.captures_iter(response) {
                if let Some(content) = cap.get(1) {
                    let content_str = content.as_str().trim();
                    if !content_str.is_empty() {
                        let date = if marker.use_today_date { Some(today) } else { None };
                        if let Err(e) = self.db.create_memory(
                            marker.memory_type.clone(),
                            content_str,
                            None,
                            None,
                            marker.importance,
                            Some(identity_id),
                            Some(session_id),
                            Some(channel_type),
                            message_id,
                            date,
                            None,
                        ) {
                            log::error!("Failed to create {}: {}", marker.name, e);
                        } else {
                            log::info!("Created {}: {}", marker.name, content_str);
                        }
                    }
                }
            }
        }
    }

    /// Remove memory markers from the response before returning to user
    fn clean_response(&self, response: &str) -> String {
        let mut clean = response.to_string();
        for marker in &self.memory_markers {
            clean = marker.pattern.replace_all(&clean, "").to_string();
        }
        // Clean up any double spaces or trailing whitespace
        clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        clean.trim().to_string()
    }

    /// Handle thinking directive messages (e.g., "/think:medium" sets session default)
    async fn handle_thinking_directive(&self, message: &NormalizedMessage) -> Option<DispatchResult> {
        let text = message.text.trim();

        // Check if this is a standalone thinking directive
        if let Some(captures) = THINKING_DIRECTIVE_PATTERN.captures(text) {
            let level_str = captures.get(1).map(|m| m.as_str()).unwrap_or("low");

            if let Some(level) = ThinkingLevel::from_str(level_str) {
                // Store the thinking level preference for this session
                // For now, we just acknowledge it (session storage could be added later)
                let response = format!(
                    "Thinking level set to **{}**. {}",
                    level,
                    match level {
                        ThinkingLevel::Off => "Extended thinking is now disabled.",
                        ThinkingLevel::Minimal => "Using minimal thinking (~1K tokens).",
                        ThinkingLevel::Low => "Using low thinking (~4K tokens).",
                        ThinkingLevel::Medium => "Using medium thinking (~10K tokens).",
                        ThinkingLevel::High => "Using high thinking (~32K tokens).",
                        ThinkingLevel::XHigh => "Using maximum thinking (~64K tokens).",
                    }
                );

                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &response,
                ));

                log::info!(
                    "Thinking level set to {} for user {} on channel {}",
                    level,
                    message.user_name,
                    message.channel_id
                );

                return Some(DispatchResult::success(response));
            } else {
                // Invalid level specified
                let response = format!(
                    "Invalid thinking level '{}'. Valid options: off, minimal, low, medium, high, xhigh",
                    level_str
                );
                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &response,
                ));
                return Some(DispatchResult::success(response));
            }
        }

        None
    }

    /// Parse inline thinking directive from message (e.g., "/think:high What is...")
    /// Returns the thinking level and the clean message text
    fn parse_inline_thinking(&self, text: &str) -> (Option<ThinkingLevel>, Option<String>) {
        let text = text.trim();

        // Use static pattern to avoid recompiling on every call
        if let Some(captures) = INLINE_THINKING_PATTERN.captures(text) {
            let level_str = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let clean_text = captures.get(2).map(|m| m.as_str().to_string());

            if let Some(level) = ThinkingLevel::from_str(level_str) {
                return (Some(level), clean_text);
            }
        }

        // No inline thinking directive found
        (None, None)
    }

    /// Call AI with progress notifications for long-running requests
    /// Broadcasts "still waiting" events every 30 seconds and handles timeout errors gracefully
    /// Also emits granular thinking phase tasks for better UI visibility
    async fn generate_with_progress(
        &self,
        client: &AiClient,
        conversation: Vec<Message>,
        tool_history: Vec<ToolHistoryEntry>,
        tools: Vec<ToolDefinition>,
        channel_id: i64,
    ) -> Result<AiResponse, crate::ai::AiError> {
        let broadcaster = self.broadcaster.clone();
        let mut elapsed_secs = 0u64;

        // Get execution ID for task tracking
        let execution_id = self.execution_tracker.get_execution_id(channel_id);

        // Emit granular thinking phase tasks
        let thinking_task_id = if let Some(ref exec_id) = execution_id {
            // Determine context for the thinking task
            let (phase_desc, phase_active) = if !tool_history.is_empty() {
                ("Processing tool results", "Analyzing results...")
            } else if !tools.is_empty() {
                ("Analyzing request", "Reasoning about approach...")
            } else {
                ("Generating response", "Composing response...")
            };

            let task_id = self.execution_tracker.start_task(
                channel_id,
                exec_id,
                Some(exec_id),
                crate::models::TaskType::Thinking,
                phase_desc,
                Some(phase_active),
            );
            Some(task_id)
        } else {
            None
        };

        // Get cancellation token for immediate interruption
        let cancel_token = self.execution_tracker.get_cancellation_token(channel_id);

        // Spawn the actual AI request
        let ai_future = client.generate_with_tools(conversation, tool_history, tools.clone());
        tokio::pin!(ai_future);

        // Create a ticker for progress updates (shorter interval for more visibility)
        let mut progress_ticker = interval(Duration::from_secs(AI_PROGRESS_INTERVAL_SECS));
        progress_ticker.tick().await; // First tick is immediate, skip it

        // Thinking phase messages for variety
        let thinking_phases = [
            "Analyzing context...",
            "Evaluating options...",
            "Considering approach...",
            "Reviewing information...",
            "Formulating response...",
            "Deep thinking...",
        ];
        let mut phase_idx = 0;

        loop {
            tokio::select! {
                // Highest priority: check for cancellation via token (immediate)
                _ = cancel_token.cancelled() => {
                    log::info!("[AI_PROGRESS] Execution cancelled via token while waiting for AI response");

                    // Complete the thinking task
                    if let Some(ref task_id) = thinking_task_id {
                        self.execution_tracker.complete_task(task_id);
                    }

                    return Err(crate::ai::AiError::new("Execution cancelled by user"));
                }
                result = &mut ai_future => {
                    // Complete the thinking task
                    if let Some(ref task_id) = thinking_task_id {
                        self.execution_tracker.complete_task(task_id);
                    }

                    match result {
                        Ok(response) => {
                            // If there are tool calls, emit a planning task
                            if !response.tool_calls.is_empty() {
                                if let Some(ref exec_id) = execution_id {
                                    let plan_desc = format!("Planning {} tool calls", response.tool_calls.len());
                                    let plan_task = self.execution_tracker.start_task(
                                        channel_id,
                                        exec_id,
                                        Some(exec_id),
                                        crate::models::TaskType::Planning,
                                        &plan_desc,
                                        Some("Preparing tool execution..."),
                                    );
                                    self.execution_tracker.complete_task(&plan_task);
                                }
                            }
                            return Ok(response);
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            // Check if it's a timeout error
                            if error_msg.contains("timed out") || error_msg.contains("timeout") {
                                log::error!("[AI_PROGRESS] Request timed out after {}s: {}", elapsed_secs, error_msg);
                                broadcaster.broadcast(GatewayEvent::agent_error(
                                    channel_id,
                                    &format!("AI request timed out after {} seconds. The AI service may be overloaded. Please try again.", elapsed_secs + AI_PROGRESS_INTERVAL_SECS),
                                ));
                            }
                            return Err(e);
                        }
                    }
                }
                _ = progress_ticker.tick() => {
                    elapsed_secs += AI_PROGRESS_INTERVAL_SECS;
                    let phase_msg = thinking_phases[phase_idx % thinking_phases.len()];
                    phase_idx += 1;

                    log::info!("[AI_PROGRESS] Still waiting for AI response... ({}s elapsed)", elapsed_secs);
                    broadcaster.broadcast(GatewayEvent::agent_thinking(
                        channel_id,
                        &format!("{} ({}s)", phase_msg, elapsed_secs),
                    ));

                    // Update the thinking task's active form
                    if let Some(ref task_id) = thinking_task_id {
                        self.execution_tracker.update_task_active_form(
                            task_id,
                            &format!("{} ({}s)", phase_msg, elapsed_secs),
                        );
                    }
                }
            }
        }
    }

    /// Handle /new or /reset commands
    async fn handle_reset_command(&self, message: &NormalizedMessage) -> DispatchResult {
        // Cancel any ongoing execution for this channel
        self.execution_tracker.cancel_execution(message.channel_id);

        // Cancel all subagents for this channel
        if let Some(ref manager) = self.subagent_manager {
            let cancelled = manager.cancel_all_for_channel(message.channel_id);
            if cancelled > 0 {
                log::info!(
                    "[RESET] Cancelled {} subagents for channel {}",
                    cancelled,
                    message.channel_id
                );
            }
        }

        // Brief delay to ensure in-flight operations acknowledge cancellation
        tokio::time::sleep(Duration::from_millis(50)).await;

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
                // Get identity for memory storage
                let identity_id = self.db.get_or_create_identity(
                    &message.channel_type,
                    &message.user_id,
                    Some(&message.user_name),
                ).ok().map(|id| id.identity_id);

                // Save session memory before reset (session memory hook)
                let message_count = self.db.count_session_messages(session.id).unwrap_or(0);
                if message_count >= 2 {
                    // Only save if there are meaningful messages
                    if let Ok(Some(settings)) = self.db.get_active_agent_settings() {
                        if let Ok(client) = AiClient::from_settings(&settings) {
                            match context::save_session_memory(
                                &self.db,
                                &client,
                                session.id,
                                identity_id.as_deref(),
                                15, // Save last 15 messages
                            ).await {
                                Ok(memory_id) => {
                                    log::info!("[SESSION_MEMORY] Saved session memory (id={}) before reset", memory_id);
                                }
                                Err(e) => {
                                    log::warn!("[SESSION_MEMORY] Failed to save session memory: {}", e);
                                }
                            }
                        }
                    }
                }

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

    /// Query GitHub API to get the authenticated user's login name
    /// Uses `gh api user` command which respects the GH_TOKEN env var
    async fn get_github_authenticated_user(&self) -> Result<String, String> {
        use tokio::process::Command;

        let mut cmd = Command::new("gh");
        cmd.args(["api", "user", "--jq", ".login"]);

        // Set GitHub token if available from stored API keys
        if let Ok(Some(key)) = self.db.get_api_key("GITHUB_TOKEN") {
            cmd.env("GH_TOKEN", key.api_key);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to execute gh CLI: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh api user failed: {}", stderr));
        }

        let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if login.is_empty() {
            return Err("GitHub API returned empty login".to_string());
        }

        Ok(login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that memory marker patterns correctly extract content
    #[test]
    fn test_memory_marker_patterns() {
        let markers = create_memory_markers();

        // Test REMEMBER pattern
        let remember_marker = markers.iter().find(|m| m.name == "long-term memory").unwrap();

        // Simple case
        let text = "[REMEMBER: user prefers dark mode]";
        let caps: Vec<_> = remember_marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "user prefers dark mode");

        // Embedded in text
        let text = "Here is my response. [REMEMBER: user name is Andy] More text follows.";
        let caps: Vec<_> = remember_marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "user name is Andy");

        // Multiple markers
        let text = "[REMEMBER: fact one] and [REMEMBER: fact two]";
        let caps: Vec<_> = remember_marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "fact one");
        assert_eq!(caps[1].get(1).unwrap().as_str(), "fact two");
    }

    #[test]
    fn test_remember_important_pattern() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "important memory").unwrap();

        let text = "[REMEMBER_IMPORTANT: API key is in vault]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "API key is in vault");

        // Verify importance is 9
        assert_eq!(marker.importance, 9);
    }

    #[test]
    fn test_daily_log_pattern() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "daily log").unwrap();

        let text = "[DAILY_LOG: fixed authentication bug]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "fixed authentication bug");

        // Verify it uses today's date
        assert!(marker.use_today_date);
    }

    #[test]
    fn test_preference_pattern() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "user preference").unwrap();

        let text = "[PREFERENCE: prefers concise answers]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "prefers concise answers");
    }

    #[test]
    fn test_fact_pattern() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "user fact").unwrap();

        let text = "[FACT: lives in San Francisco]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "lives in San Francisco");
    }

    #[test]
    fn test_task_pattern() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "task/commitment").unwrap();

        let text = "[TASK: review PR #123 tomorrow]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "review PR #123 tomorrow");

        // Verify it uses today's date for tasks
        assert!(marker.use_today_date);
    }

    #[test]
    fn test_all_markers_in_single_response() {
        let markers = create_memory_markers();

        let text = r#"
            I've noted your preferences. [REMEMBER: user is a Rust developer]
            [PREFERENCE: prefers functional style] [FACT: works at TechCorp]
            [DAILY_LOG: helped with async code] [TASK: follow up on performance]
            [REMEMBER_IMPORTANT: has production deadline Friday]
        "#;

        let mut total_matches = 0;
        for marker in &markers {
            let count = marker.pattern.captures_iter(text).count();
            total_matches += count;
        }

        assert_eq!(total_matches, 6, "Should find all 6 markers");
    }

    #[test]
    fn test_marker_with_special_characters() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "long-term memory").unwrap();

        // Content with special chars (but not closing bracket)
        let text = "[REMEMBER: user's email is test@example.com]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str(), "user's email is test@example.com");
    }

    #[test]
    fn test_marker_strips_from_response() {
        let markers = create_memory_markers();

        let response = "Hello! [REMEMBER: user name is Andy] How can I help you today?";

        let mut clean = response.to_string();
        for marker in &markers {
            clean = marker.pattern.replace_all(&clean, "").to_string();
        }
        clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");

        assert_eq!(clean, "Hello! How can I help you today?");
        assert!(!clean.contains("[REMEMBER"));
    }

    #[test]
    fn test_multiple_same_markers_stripped() {
        let markers = create_memory_markers();

        let response = "[REMEMBER: fact1] Some text [REMEMBER: fact2] more text [REMEMBER: fact3]";

        let mut clean = response.to_string();
        for marker in &markers {
            clean = marker.pattern.replace_all(&clean, "").to_string();
        }
        clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");

        assert_eq!(clean, "Some text more text");
    }

    #[test]
    fn test_empty_content_not_matched() {
        let markers = create_memory_markers();
        let marker = markers.iter().find(|m| m.name == "long-term memory").unwrap();

        // Empty content after colon - the .+? requires at least one char
        let text = "[REMEMBER: ]";
        let caps: Vec<_> = marker.pattern.captures_iter(text).collect();
        // This will match the space, so we check for empty after trim in actual code
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].get(1).unwrap().as_str().trim(), "");
    }

    #[test]
    fn test_marker_types_are_correct() {
        let markers = create_memory_markers();

        for marker in &markers {
            match marker.name {
                "daily log" => assert!(matches!(marker.memory_type, MemoryType::DailyLog)),
                "long-term memory" => assert!(matches!(marker.memory_type, MemoryType::LongTerm)),
                "important memory" => assert!(matches!(marker.memory_type, MemoryType::LongTerm)),
                "user preference" => assert!(matches!(marker.memory_type, MemoryType::Preference)),
                "user fact" => assert!(matches!(marker.memory_type, MemoryType::Fact)),
                "task/commitment" => assert!(matches!(marker.memory_type, MemoryType::Task)),
                _ => panic!("Unknown marker: {}", marker.name),
            }
        }
    }
}
