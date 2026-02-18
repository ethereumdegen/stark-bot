mod system_prompt;

use crate::ai::{
    multi_agent::{types::{self as agent_types, AgentMode}, Orchestrator, SubAgentManager},
    AiClient, ArchetypeId, ArchetypeRegistry, Message, MessageRole, ModelArchetype,
    ThinkingLevel,
};
use crate::channels::types::{DispatchResult, NormalizedMessage};
use crate::config::MemoryConfig;
use crate::context::{self, estimate_tokens, ContextManager};
use crate::db::Database;
use crate::execution::ExecutionTracker;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::session_message::MessageRole as DbMessageRole;
use crate::models::{AgentSettings, CompletionStatus, SessionScope, SpecialRoleGrants, DEFAULT_MAX_TOOL_ITERATIONS};
use crate::qmd_memory::MemoryStore;
use crate::telemetry::{
    self, Rollout, RolloutConfig, RolloutManager, SpanCollector, SpanType,
    RewardEmitter, TelemetryStore, Watchdog, WatchdogConfig, ResourceManager,
};
use crate::tools::{ToolConfig, ToolContext, ToolDefinition, ToolExecution, ToolRegistry};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
mod broadcasting;
mod commands;
mod finalization;
mod skills;
mod tool_loop;
mod tool_processing;

/// Fallback maximum tool iterations (used when db lookup fails)
/// Actual value is configurable via bot settings
pub(super) const FALLBACK_MAX_TOOL_ITERATIONS: usize = DEFAULT_MAX_TOOL_ITERATIONS as usize;

/// Dispatcher routes messages to the AI and returns responses
pub struct MessageDispatcher {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    tool_registry: Arc<ToolRegistry>,
    execution_tracker: Arc<ExecutionTracker>,
    /// Async write-behind buffer for tool call/result session messages
    session_writer: crate::channels::session_writer::SessionMessageWriter,
    /// Wallet provider for x402 payments and transaction signing
    /// Encapsulates both Standard mode (EnvWalletProvider with raw private key)
    /// and Flash mode (FlashWalletProvider with Privy proxy)
    wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    context_manager: ContextManager,
    archetype_registry: ArchetypeRegistry,
    /// Memory configuration (simplified - no longer using memory markers)
    memory_config: MemoryConfig,
    /// QMD Memory store for file-based markdown memory system
    memory_store: Option<Arc<MemoryStore>>,
    /// Hybrid search engine (FTS + vector + graph)
    hybrid_search: Option<Arc<crate::memory::HybridSearchEngine>>,
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
    /// Disk quota manager for enforcing disk usage limits
    disk_quota: Option<Arc<crate::disk_quota::DiskQuotaManager>>,
    /// Telemetry store for persisting execution spans
    telemetry_store: Arc<TelemetryStore>,
    /// Rollout manager for retry lifecycle
    rollout_manager: Arc<RolloutManager>,
    /// Resource manager for versioned prompts/configs
    resource_manager: Arc<ResourceManager>,
    /// Watchdog configuration for timeout enforcement
    watchdog_config: WatchdogConfig,
    /// Message coalescer for batching rapid-fire messages
    coalescer: Option<crate::channels::coalescing::MessageCoalescer>,
    /// Mock AI client for integration tests (bypasses real AI API)
    #[cfg(test)]
    mock_ai_client: Option<crate::ai::MockAiClient>,
}

impl MessageDispatcher {
    pub fn new(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
    ) -> Self {
        Self::new_with_wallet_and_skills(db, broadcaster, tool_registry, execution_tracker, None, None)
    }

    /// Create dispatcher with wallet provider for x402 payments
    /// The wallet_provider encapsulates both Standard mode (EnvWalletProvider)
    /// and Flash mode (FlashWalletProvider)
    pub fn new_with_wallet(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    ) -> Self {
        Self::new_with_wallet_and_skills(
            db,
            broadcaster,
            tool_registry,
            execution_tracker,
            wallet_provider,
            None,
        )
    }

    pub fn new_with_wallet_and_skills(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
        skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
    ) -> Self {
        let memory_config = MemoryConfig::from_env();

        // Create QMD memory store (before SubAgentManager so we can pass it through)
        let memory_dir = std::path::PathBuf::from(memory_config.memory_dir.clone());
        let memory_store = match MemoryStore::new(memory_dir, &memory_config.memory_db_path()) {
            Ok(store) => {
                log::info!("[DISPATCHER] QMD MemoryStore initialized at {}", memory_config.memory_dir);
                Some(Arc::new(store))
            }
            Err(e) => {
                log::error!("[DISPATCHER] Failed to create MemoryStore: {}", e);
                None
            }
        };

        // Create SubAgentManager for spawning background AI agents
        // Uses OnceLock for late-bound fields (tx_queue, disk_quota set via with_* after construction)
        let subagent_manager = Arc::new(SubAgentManager::new_with_config(
            db.clone(),
            broadcaster.clone(),
            tool_registry.clone(),
            Default::default(),
            wallet_provider.clone(),
        ));
        // Set stores that are available now; tx_queue/disk_quota will be set later via with_*
        if let Some(ref registry) = skill_registry {
            subagent_manager.set_skill_registry(registry.clone());
        }
        if let Some(ref store) = memory_store {
            subagent_manager.set_memory_store(store.clone());
        }
        log::info!("[DISPATCHER] SubAgentManager initialized");

        // Create context manager and link memory store to it
        let mut context_manager = ContextManager::new(db.clone())
            .with_memory_config(memory_config.clone());
        if let Some(ref store) = memory_store {
            context_manager = context_manager.with_memory_store(store.clone());
            log::info!("[DISPATCHER] Memory store linked to context manager");
        }

        // Apply compaction thresholds from bot settings (if available)
        if let Ok(bot_settings) = db.get_bot_settings() {
            use crate::context::ThreeTierCompactionConfig;
            let compaction_cfg = ThreeTierCompactionConfig {
                background_threshold: bot_settings.compaction_background_threshold,
                aggressive_threshold: bot_settings.compaction_aggressive_threshold,
                emergency_threshold: bot_settings.compaction_emergency_threshold,
                ..ThreeTierCompactionConfig::default()
            };
            context_manager = context_manager.with_compaction_config(compaction_cfg);
        }

        // Initialize telemetry subsystem
        let telemetry_store = Arc::new(TelemetryStore::new(db.clone()));
        let rollout_manager = Arc::new(RolloutManager::new(db.clone()));
        let resource_manager = Arc::new(ResourceManager::new(db.clone()));
        resource_manager.seed_defaults();

        let session_writer = crate::channels::session_writer::SessionMessageWriter::new(db.clone());

        Self {
            db,
            broadcaster,
            tool_registry,
            execution_tracker,
            session_writer,
            wallet_provider,
            context_manager,
            archetype_registry: ArchetypeRegistry::new(),
            memory_config,
            memory_store,
            hybrid_search: None,
            subagent_manager: Some(subagent_manager),
            skill_registry,
            coalescer: None,
            hook_manager: None,
            validator_registry: None,
            tx_queue: None,
            disk_quota: None,
            telemetry_store,
            rollout_manager,
            resource_manager,
            watchdog_config: WatchdogConfig::default(),
            #[cfg(test)]
            mock_ai_client: None,
        }
    }

    /// Set the disk quota manager for enforcing disk usage limits
    pub fn with_disk_quota(mut self, dq: Arc<crate::disk_quota::DiskQuotaManager>) -> Self {
        // Also propagate to SubAgentManager so sub-agents have disk quota in their ToolContext
        if let Some(ref mgr) = self.subagent_manager {
            mgr.set_disk_quota(dq.clone());
        }
        self.disk_quota = Some(dq);
        self
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
        // Also propagate to SubAgentManager so sub-agents can queue web3 transactions
        if let Some(ref mgr) = self.subagent_manager {
            mgr.set_tx_queue(tx_queue.clone());
        }
        self.tx_queue = Some(tx_queue);
        self
    }

    /// Set the hybrid search engine
    pub fn with_hybrid_search(mut self, engine: Arc<crate::memory::HybridSearchEngine>) -> Self {
        self.hybrid_search = Some(engine);
        self
    }

    /// Set message coalescer for batching rapid-fire messages
    pub fn with_coalescer(mut self, coalescer: crate::channels::coalescing::MessageCoalescer) -> Self {
        self.coalescer = Some(coalescer);
        self
    }

    /// Try to coalesce a message. Returns Some(text) if ready to dispatch,
    /// None if still accumulating.
    pub fn coalesce_message(&self, channel_id: i64, user_id: &str, text: &str) -> Option<String> {
        match &self.coalescer {
            Some(coalescer) => coalescer.add_message(channel_id, user_id, text),
            None => Some(text.to_string()), // No coalescer = pass through
        }
    }

    /// Check coalescer timeouts and return any ready batches
    pub fn check_coalesce_timeouts(&self) -> Vec<(String, String)> {
        match &self.coalescer {
            Some(coalescer) => coalescer.check_timeouts(),
            None => Vec::new(),
        }
    }

    /// Set a mock AI client for integration tests (bypasses real AI API)
    #[cfg(test)]
    pub fn with_mock_ai_client(mut self, client: crate::ai::MockAiClient) -> Self {
        self.mock_ai_client = Some(client);
        self
    }

    #[cfg(test)]
    pub fn get_mock_trace(&self) -> Vec<crate::ai::TraceEntry> {
        self.mock_ai_client.as_ref().map(|m| m.get_trace()).unwrap_or_default()
    }

    /// Create a dispatcher without tool support (for backwards compatibility)
    pub fn new_without_tools(db: Arc<Database>, broadcaster: Arc<EventBroadcaster>) -> Self {
        // Create a minimal execution tracker for legacy use
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
        let memory_config = MemoryConfig::from_env();

        // Create QMD memory store
        let memory_dir = std::path::PathBuf::from(memory_config.memory_dir.clone());
        let memory_store = MemoryStore::new(memory_dir, &memory_config.memory_db_path())
            .ok()
            .map(Arc::new);

        // Create context manager and link memory store to it
        let mut context_manager = ContextManager::new(db.clone())
            .with_memory_config(memory_config.clone());
        if let Some(ref store) = memory_store {
            context_manager = context_manager.with_memory_store(store.clone());
        }

        let telemetry_store = Arc::new(TelemetryStore::new(db.clone()));
        let rollout_manager = Arc::new(RolloutManager::new(db.clone()));
        let resource_manager = Arc::new(ResourceManager::new(db.clone()));

        let session_writer = crate::channels::session_writer::SessionMessageWriter::new(db.clone());

        Self {
            db: db.clone(),
            broadcaster,
            tool_registry: Arc::new(ToolRegistry::new()),
            execution_tracker,
            session_writer,
            wallet_provider: None,
            context_manager,
            archetype_registry: ArchetypeRegistry::new(),
            memory_config,
            memory_store,
            hybrid_search: None,
            subagent_manager: None, // No tools = no subagent support
            skill_registry: None,   // No skills without tools
            coalescer: None,
            hook_manager: None,     // No hooks without explicit setup
            validator_registry: None, // No validators without explicit setup
            tx_queue: None,         // No tx queue without explicit setup
            disk_quota: None,       // No disk quota without explicit setup
            telemetry_store,
            rollout_manager,
            resource_manager,
            watchdog_config: WatchdogConfig::default(),
            #[cfg(test)]
            mock_ai_client: None,
        }
    }

    /// Get the QMD MemoryStore (if available)
    pub fn memory_store(&self) -> Option<Arc<MemoryStore>> {
        self.memory_store.clone()
    }

    /// Get the SubAgentManager (if available)
    pub fn subagent_manager(&self) -> Option<Arc<SubAgentManager>> {
        self.subagent_manager.clone()
    }

    /// Get the TelemetryStore
    pub fn telemetry_store(&self) -> &Arc<TelemetryStore> {
        &self.telemetry_store
    }

    /// Get the ResourceManager
    pub fn resource_manager(&self) -> &Arc<ResourceManager> {
        &self.resource_manager
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

        // Try to coalesce rapid-fire messages (returns None if still accumulating)
        let dispatch_text = match self.coalesce_message(
            message.channel_id,
            &message.user_id,
            &message.text,
        ) {
            Some(text) => text,
            None => {
                // Message is being accumulated; return a no-op result
                return DispatchResult::success("(coalescing)".to_string());
            }
        };

        // Use the coalesced text for the rest of the pipeline
        let message = NormalizedMessage {
            text: dispatch_text,
            ..message
        };

        // Parse inline thinking directive and extract clean message
        let (thinking_level, clean_text) = commands::parse_inline_thinking(&message.text);

        // Start execution tracking with user message for descriptive display
        let user_msg = clean_text.as_deref().unwrap_or(&message.text);
        let execution_id = self.execution_tracker.start_execution(
            message.channel_id,
            Some(&message.chat_id),
            "execute",
            Some(user_msg),
        );

        // Initialize telemetry rollout for this dispatch
        // We use session_id=0 initially; it will be updated once the session is resolved
        let rollout_config = RolloutConfig::default();
        let (mut rollout, span_collector) = self.rollout_manager.start_rollout(
            0, // will be updated once we have the session
            message.channel_id,
            rollout_config,
        );
        let span_collector = Arc::new(span_collector);

        // Set up the watchdog for timeout enforcement
        let reward_emitter = Arc::new(RewardEmitter::new(Arc::clone(&span_collector)));
        let watchdog = Watchdog::new(
            self.watchdog_config.clone(),
            Arc::clone(&span_collector),
            Arc::clone(&reward_emitter),
        );

        // Start heartbeat monitor for long-running executions
        let watchdog = Arc::new(watchdog);
        let heartbeat_handle = watchdog.start_heartbeat_monitor(
            message.channel_id,
            Arc::clone(&self.broadcaster),
        );

        // Install thread-local span collector for emit_* functions
        telemetry::set_active_collector(Arc::clone(&span_collector));

        // Emit a rollout start span
        let mut rollout_span = span_collector.start_span(SpanType::Rollout, "dispatch_start");
        rollout_span.attributes = serde_json::json!({
            "channel_id": message.channel_id,
            "user_name": message.user_name,
            "channel_type": message.channel_type,
            "rollout_id": rollout.rollout_id,
        });
        rollout_span.succeed();
        span_collector.record(rollout_span);

        // Track the resource version used
        rollout.resources_id = self.resource_manager.active_version_id();

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
                self.rollout_manager.fail_attempt(&mut rollout, &error_msg, &span_collector);
                self.telemetry_store.persist_spans(&span_collector);
                heartbeat_handle.abort();
                telemetry::clear_active_collector();
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

        // For gateway channels (Discord, Telegram), create a fresh session for each message
        // to prevent context from growing too large. Previous conversation context is
        // preserved by including the last 10 messages in the system prompt.
        let channel_type_lower = message.channel_type.to_lowercase();
        let is_gateway_channel = channel_type_lower == "discord" || channel_type_lower == "telegram";

        // Collect previous session messages for gateway channels (max 10)
        let previous_gateway_messages: Vec<crate::models::SessionMessage> = if is_gateway_channel {
            const MAX_PREVIOUS_MESSAGES: i32 = 10;

            // Get the current active session (if any) and its messages
            if let Ok(Some(prev_session)) = self.db.get_latest_session_for_channel(
                &message.channel_type,
                message.channel_id,
            ) {
                let messages = self.db.get_recent_session_messages(prev_session.id, MAX_PREVIOUS_MESSAGES)
                    .unwrap_or_default();

                // Deactivate the old session
                if let Err(e) = self.db.deactivate_session(prev_session.id) {
                    log::warn!("[DISPATCH] Failed to deactivate previous session {}: {}", prev_session.id, e);
                } else {
                    log::info!(
                        "[DISPATCH] Deactivated previous {} session {} with {} messages for context",
                        message.channel_type, prev_session.id, messages.len()
                    );
                }

                messages
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Get or create chat session
        let session = if is_gateway_channel {
            // Always create a fresh session for gateway channels
            match self.db.create_gateway_session(
                &message.channel_type,
                message.channel_id,
                scope,
                None,
            ) {
                Ok(s) => {
                    log::info!(
                        "[DISPATCH] Created fresh {} session {} (previous context: {} messages)",
                        message.channel_type, s.id, previous_gateway_messages.len()
                    );
                    s
                }
                Err(e) => {
                    let error_msg = format!("Session error: {}", e);
                    log::error!("Failed to create gateway session: {}", e);
                    self.broadcaster.broadcast(GatewayEvent::agent_error(
                        message.channel_id,
                        &error_msg,
                    ));
                    self.execution_tracker.complete_execution(message.channel_id);
                    self.rollout_manager.fail_attempt(&mut rollout, &error_msg, &span_collector);
                    self.telemetry_store.persist_spans(&span_collector);
                    heartbeat_handle.abort();
                telemetry::clear_active_collector();
                    return DispatchResult::error(error_msg);
                }
            }
        } else {
            // Standard session handling for other channels
            match self.db.get_or_create_chat_session(
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
                    self.rollout_manager.fail_attempt(&mut rollout, &error_msg, &span_collector);
                    self.telemetry_store.persist_spans(&span_collector);
                    heartbeat_handle.abort();
                telemetry::clear_active_collector();
                    return DispatchResult::error(error_msg);
                }
            }
        };

        // Now that session is resolved, update rollout and span collector with real session_id
        rollout.session_id = session.id;
        span_collector.set_session(session.id);

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
                // Store error as assistant message so it's visible in the session
                let _ = self.db.add_session_message(
                    session.id, DbMessageRole::Assistant,
                    &format!("[Error] {}", error), None, None, None, None,
                );
                // Mark session as Failed so it doesn't stay stuck as Active
                let _ = self.db.update_session_completion_status(session.id, CompletionStatus::Failed);
                self.broadcast_session_complete(message.channel_id, session.id);
                self.execution_tracker.complete_execution(message.channel_id);
                self.rollout_manager.fail_attempt(&mut rollout, &error, &span_collector);
                self.telemetry_store.persist_spans(&span_collector);
                heartbeat_handle.abort();
                telemetry::clear_active_collector();
                return DispatchResult::error(error);
            }
        };

        // Infer archetype from settings
        let archetype_id = AiClient::infer_archetype(&settings);
        log::info!(
            "Using endpoint {} for message dispatch (archetype={}, max_response={}, max_context={})",
            settings.endpoint,
            archetype_id,
            settings.max_response_tokens,
            settings.max_context_tokens
        );

        // Sync session's max_context_tokens with agent settings for dynamic compaction
        self.context_manager.sync_max_context_tokens(session.id, settings.max_context_tokens);

        // Create AI client — use mock in tests if configured, otherwise create from settings
        #[cfg(test)]
        let client = if let Some(ref mock) = self.mock_ai_client {
            AiClient::Mock(mock.clone())
        } else {
            match AiClient::from_settings_with_wallet_provider(&settings, self.wallet_provider.clone()) {
                Ok(c) => c.with_broadcaster(Arc::clone(&self.broadcaster), message.channel_id),
                Err(e) => {
                    let error = format!("Failed to create AI client: {}", e);
                    log::error!("{}", error);
                    // Store error as assistant message so it's visible in the session
                    let _ = self.db.add_session_message(
                        session.id, DbMessageRole::Assistant,
                        &format!("[Error] {}", error), None, None, None, None,
                    );
                    // Mark session as Failed so it doesn't stay stuck as Active
                    let _ = self.db.update_session_completion_status(session.id, CompletionStatus::Failed);
                    self.broadcast_session_complete(message.channel_id, session.id);
                    self.broadcaster.broadcast(GatewayEvent::agent_error(message.channel_id, &error));
                    self.execution_tracker.complete_execution(message.channel_id);
                    self.rollout_manager.fail_attempt(&mut rollout, &error, &span_collector);
                    self.telemetry_store.persist_spans(&span_collector);
                    heartbeat_handle.abort();
                    telemetry::clear_active_collector();
                    return DispatchResult::error(error);
                }
            }
        };
        #[cfg(not(test))]
        let client = match AiClient::from_settings_with_wallet_provider(&settings, self.wallet_provider.clone()) {
            Ok(c) => c.with_broadcaster(Arc::clone(&self.broadcaster), message.channel_id),
            Err(e) => {
                let error = format!("Failed to create AI client: {}", e);
                log::error!("{}", error);
                // Store error as assistant message so it's visible in the session
                let _ = self.db.add_session_message(
                    session.id, DbMessageRole::Assistant,
                    &format!("[Error] {}", error), None, None, None, None,
                );
                // Mark session as Failed so it doesn't stay stuck as Active
                let _ = self.db.update_session_completion_status(session.id, CompletionStatus::Failed);
                self.broadcast_session_complete(message.channel_id, session.id);
                self.broadcaster.broadcast(GatewayEvent::agent_error(
                    message.channel_id,
                    &error,
                ));
                self.execution_tracker.complete_execution(message.channel_id);
                self.rollout_manager.fail_attempt(&mut rollout, &error, &span_collector);
                self.telemetry_store.persist_spans(&span_collector);
                heartbeat_handle.abort();
                telemetry::clear_active_collector();
                return DispatchResult::error(error);
            }
        };

        // Add thinking event before AI generation
        self.execution_tracker.add_thinking(message.channel_id, "Processing request...");

        // Get tool configuration for this channel (needed for system prompt)
        let mut tool_config = self.db.get_effective_tool_config(Some(message.channel_id))
            .unwrap_or_default();

        // Check channel safe_mode OR message-level force_safe_mode
        let channel_safe_mode = self.db.get_channel(message.channel_id)
            .ok()
            .flatten()
            .map(|ch| ch.safe_mode)
            .unwrap_or(false);

        let is_safe_mode = channel_safe_mode || message.force_safe_mode;
        let mut special_role_grants: Option<SpecialRoleGrants> = None;

        if is_safe_mode {
            log::info!(
                "[DISPATCH] Safe mode enabled (channel={}, force={}), restricting tools",
                channel_safe_mode,
                message.force_safe_mode
            );
            // Mark session as safe mode for UI display
            if let Err(e) = self.db.set_session_safe_mode(session.id) {
                log::warn!("[DISPATCH] Failed to set session safe_mode: {}", e);
            }
            // Replace the entire tool config with the canonical safe mode config.
            // ToolConfig::safe_mode() is the single source of truth for safe mode permissions.
            // This discards any channel-level overrides — safe mode is absolute.
            tool_config = crate::tools::ToolConfig::safe_mode();

            // Check for special role grants that enrich safe mode for this user
            match self.db.get_special_role_grants(&message.channel_type, &message.user_id) {
                Ok(grants) if !grants.is_empty() => {
                    log::info!(
                        "[DISPATCH] Special role enrichment for user {} on {}: +tools={:?}",
                        message.user_id, message.channel_type, grants.extra_tools
                    );
                    for tool_name in &grants.extra_tools {
                        if !tool_config.allow_list.contains(tool_name) {
                            tool_config.allow_list.push(tool_name.clone());
                        }
                    }

                    // Enrich with skill-required tools from granted skill names.
                    // Each granted skill's requires_tools are auto-added to the allow list
                    // so the user can actually invoke those skills.
                    if !grants.extra_skills.is_empty() {
                        if !tool_config.allow_list.iter().any(|t| t == "use_skill") {
                            tool_config.allow_list.push("use_skill".to_string());
                        }
                        tool_config.extra_skill_names = grants.extra_skills.clone();
                        let mut auto_tools: Vec<String> = Vec::new();
                        for skill_name in &grants.extra_skills {
                            match self.db.get_enabled_skill_by_name(skill_name) {
                                Ok(Some(skill)) => {
                                    for req_tool in &skill.requires_tools {
                                        if !tool_config.allow_list.contains(req_tool)
                                            && !auto_tools.contains(req_tool)
                                        {
                                            auto_tools.push(req_tool.clone());
                                        }
                                    }
                                }
                                Ok(None) => {
                                    log::warn!(
                                        "[DISPATCH] Special role grants skill '{}' but it doesn't exist or is disabled",
                                        skill_name
                                    );
                                }
                                Err(e) => {
                                    log::warn!(
                                        "[DISPATCH] Failed to look up skill '{}' for special role: {}",
                                        skill_name, e
                                    );
                                }
                            }
                        }
                        if !auto_tools.is_empty() {
                            log::info!(
                                "[DISPATCH] Special role skill enrichment for {}: auto-granted tools {:?} from skills {:?}",
                                message.user_id, auto_tools, grants.extra_skills
                            );
                            tool_config.allow_list.extend(auto_tools);
                        }
                    }

                    // Store the special role name on the session for UI badge display
                    if let Some(role_name) = &grants.role_name {
                        if let Err(e) = self.db.set_session_special_role(session.id, role_name) {
                            log::warn!("[DISPATCH] Failed to set session special_role: {}", e);
                        }
                    }

                    special_role_grants = Some(grants);
                }
                Ok(_) => {} // No special role
                Err(e) => log::warn!("[DISPATCH] Failed to check special role grants: {}", e),
            }
        }

        // Twitter has no interactive session — ask_user can never work, so block it.
        if message.channel_type == "twitter" {
            tool_config.deny_list.push("ask_user".to_string());
        }

        // Debug: Log tool configuration
        log::info!(
            "[DISPATCH] Tool config - profile: {:?}, allowed_groups: {:?}, safe_mode: {}",
            tool_config.profile,
            tool_config.allowed_groups,
            is_safe_mode
        );

        // Build context from memories, tools, skills, and session history
        let system_prompt = self.build_system_prompt(&message, &identity.identity_id, &tool_config, is_safe_mode, special_role_grants.as_ref());

        // Debug: Log full system prompt
        log::debug!("[DISPATCH] System prompt:\n{}", system_prompt);

        // Build context with cross-session memory integration
        let memory_identity: Option<&str> = if is_safe_mode { Some("safemode") } else { Some(&identity.identity_id) };
        let (history, context_summary) = self.context_manager.build_context_with_memories(
            session.id,
            memory_identity,
            20,
        );

        // Build messages for the AI
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: system_prompt.clone(),
        }];

        // Add combined context (compaction summary + cross-session memories) if available
        if let Some(context) = context_summary {
            messages.push(Message {
                role: MessageRole::System,
                content: context,
            });
        }

        // Add previous gateway chat messages (for Discord/Telegram fresh sessions)
        // These are the last 10 messages from the previous session, providing continuity
        if !previous_gateway_messages.is_empty() {
            let mut context_text = String::from("## Previous Conversation\nRecent messages from the previous chat session:\n\n");
            for msg in &previous_gateway_messages {
                let role_label = match msg.role {
                    DbMessageRole::User => "User",
                    DbMessageRole::Assistant => "Assistant",
                    DbMessageRole::System => "System",
                    DbMessageRole::ToolCall => "Tool Call",
                    DbMessageRole::ToolResult => "Tool Result",
                };
                // Truncate very long messages to keep context manageable
                let content = if msg.content.len() > 500 {
                    format!("{}...", &msg.content[..500])
                } else {
                    msg.content.clone()
                };
                context_text.push_str(&format!("**{}**: {}\n\n", role_label, content));
            }
            messages.push(Message {
                role: MessageRole::System,
                content: context_text,
            });
            log::info!(
                "[DISPATCH] Added {} previous gateway messages to context",
                previous_gateway_messages.len()
            );
        }

        // Scan user input for key terms (ETH addresses, token symbols) for context bank
        let context_bank_items = crate::tools::scan_input(message_text);
        if !context_bank_items.is_empty() {
            // Create a temporary context bank for formatting
            let temp_bank = crate::tools::ContextBank::new();
            temp_bank.add_all(context_bank_items.clone());
            if let Some(context_bank_text) = temp_bank.format_for_agent() {
                messages.push(Message {
                    role: MessageRole::System,
                    content: format!(
                        "## Context Bank\nThe following key terms were detected in the user's input: {}",
                        context_bank_text
                    ),
                });
            }
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
            // Skip empty assistant messages - some APIs (e.g. Kimi) reject them
            if role == MessageRole::Assistant && msg.content.trim().is_empty() {
                continue;
            }
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
            .with_platform_chat_id(message.chat_id.clone())
            .with_user(message.user_id.clone())
            .with_session(session.id)
            .with_identity(identity.identity_id.clone())
            .with_workspace(workspace_dir.clone())
            .with_broadcaster(self.broadcaster.clone())
            .with_database(self.db.clone())
            .with_selected_network(message.selected_network.clone());

        // Log selected network if present
        if let Some(ref network) = message.selected_network {
            log::info!("[DISPATCH] Selected network from UI: {}", network);
        }

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

        // Add WalletProvider for x402 payments (Flash mode)
        if let Some(ref wallet_provider) = self.wallet_provider {
            tool_context = tool_context.with_wallet_provider(wallet_provider.clone());
            log::debug!("[DISPATCH] WalletProvider attached to tool context ({})", wallet_provider.mode_name());
        }

        // Add MemoryStore for QMD memory tools (memory_search, memory_read)
        if let Some(ref store) = self.memory_store {
            tool_context = tool_context.with_memory_store(store.clone());
            log::debug!("[DISPATCH] MemoryStore attached to tool context");
        }

        // Add HybridSearchEngine for hybrid memory search (FTS + vector + graph)
        if let Some(ref engine) = self.hybrid_search {
            tool_context = tool_context.with_hybrid_search(engine.clone());
            log::debug!("[DISPATCH] HybridSearchEngine attached to tool context");
        }

        // Add DiskQuotaManager for enforcing disk usage limits
        if let Some(ref dq) = self.disk_quota {
            tool_context = tool_context.with_disk_quota(dq.clone());
            log::debug!("[DISPATCH] DiskQuotaManager attached to tool context");
        }

        // Pass safe mode flag to tool context so tools can sandbox themselves
        if is_safe_mode {
            tool_context.extra.insert(
                "safe_mode".to_string(),
                serde_json::json!(true),
            );
        }

        // Populate tool context with the context bank items scanned earlier
        if !context_bank_items.is_empty() {
            tool_context.context_bank.add_all(context_bank_items.clone());
            log::info!(
                "[DISPATCH] Context bank populated with {} items: {:?}",
                tool_context.context_bank.len(),
                tool_context.get_context_bank_for_agent()
            );
            // Broadcast context bank update to frontend
            if let Some(channel_id) = tool_context.channel_id {
                self.broadcaster.broadcast(GatewayEvent::context_bank_update(
                    channel_id,
                    tool_context.context_bank.to_json(),
                ));
            }
        }

        // Ensure workspace directory exists
        let _ = std::fs::create_dir_all(&workspace_dir);

        // Load API keys from database into ToolContext (per-session, no global env mutation)
        // In safe mode, skip loading API keys (discord/telegram/slack tokens come from channel settings)
        if !is_safe_mode {
            if let Ok(keys) = self.db.list_api_keys() {
                log::debug!("[DISPATCH] Loading {} API keys from database into ToolContext", keys.len());
                for key in keys {
                    let preview = if key.api_key.len() > 8 { &key.api_key[..8] } else { &key.api_key };
                    log::debug!("[DISPATCH]   Loading key: {} (len={}, prefix={}...)", key.service_name, key.api_key.len(), preview);
                    tool_context = tool_context.with_api_key(&key.service_name, key.api_key.clone());
                }
            }
        } else {
            log::debug!("[DISPATCH] Safe mode enabled — skipping API key loading");
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

            // Add rogue_mode_enabled for partner mode transaction confirmation
            tool_context.extra.insert(
                "rogue_mode_enabled".to_string(),
                serde_json::json!(bot_settings.rogue_mode_enabled),
            );

            // Configure HTTP proxy for tool requests if set
            if let Some(ref url) = bot_settings.proxy_url {
                if !url.is_empty() {
                    tool_context = tool_context.with_proxy_url(url.clone());
                }
            }
        }

        // Store original user message for verify_intent safety checks
        tool_context.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!(message.text.clone()),
        );

        // Transition rollout to Running now that setup is complete
        self.rollout_manager.mark_running(&mut rollout);
        self.broadcaster.broadcast(GatewayEvent::rollout_status_change(
            message.channel_id, &rollout.rollout_id, "running", rollout.attempt_count(),
        ));

        // Generate response with retry-aware loop.
        // On retryable failures (timeout, LLM error, context overflow), the rollout
        // manager creates a new attempt and we retry the entire generation.
        let final_response = loop {
            let attempt_result = if use_tools {
                self.generate_with_tool_loop(
                    &client,
                    messages.clone(),
                    &tool_config,
                    &tool_context,
                    &identity.identity_id,
                    session.id,
                    &message,
                    archetype_id,
                    is_safe_mode,
                    &watchdog,
                ).await
            } else {
                // Simple generation without tools - with x402 event emission
                match client.generate_text_with_events(messages.clone(), &self.broadcaster, message.channel_id).await {
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
                        Ok((content, false))
                    }
                    Err(e) => Err(e),
                }
            };

            // On success, break out of the retry loop
            match attempt_result {
                Ok(response) => {
                    // Emit retry_succeeded reward if this wasn't the first attempt
                    if rollout.attempt_count() > 1 {
                        reward_emitter.retry_succeeded(rollout.attempt_count() - 1);
                    }
                    break Ok(response);
                }
                Err(ref error_str) => {
                    let error_msg = error_str.to_string();
                    // Populate attempt stats before failing
                    Self::populate_attempt_stats(&mut rollout, &span_collector);
                    let should_retry = self.rollout_manager.fail_attempt(
                        &mut rollout,
                        &error_msg,
                        &span_collector,
                    );
                    if should_retry {
                        let delay_ms = self.rollout_manager.retry_delay(&rollout);
                        log::info!(
                            "[DISPATCH] Retrying after {}ms (attempt {}/{}): {}",
                            delay_ms,
                            rollout.attempt_count(),
                            rollout.config.max_attempts,
                            error_msg,
                        );
                        self.broadcaster.broadcast(GatewayEvent::agent_error(
                            message.channel_id,
                            &format!("Retrying... (attempt {}/{})", rollout.attempt_count(), rollout.config.max_attempts),
                        ));
                        self.broadcaster.broadcast(GatewayEvent::rollout_status_change(
                            message.channel_id, &rollout.rollout_id, "retrying", rollout.attempt_count(),
                        ));
                        // Dispatch OnRolloutRetry hook
                        if let Some(hook_manager) = &self.hook_manager {
                            use crate::hooks::{HookContext, HookEvent};
                            let mut hook_ctx = HookContext::new(HookEvent::OnRolloutRetry)
                                .with_channel(message.channel_id, Some(session.id))
                                .with_error(error_msg.clone());
                            hook_ctx.extra = serde_json::json!({
                                "rollout_id": &rollout.rollout_id,
                                "attempt": rollout.attempt_count(),
                            });
                            let _ = hook_manager.execute(HookEvent::OnRolloutRetry, &mut hook_ctx).await;
                        }
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        continue; // retry
                    } else {
                        self.broadcaster.broadcast(GatewayEvent::rollout_status_change(
                            message.channel_id, &rollout.rollout_id, "failed", rollout.attempt_count(),
                        ));
                        break Err(error_msg);
                    }
                }
            }
        };

        match final_response {
            Ok((response, delivered_via_say_to_user)) => {
                // Estimate tokens for the response
                let response_tokens = estimate_tokens(&response);

                // Store AI response in session with token count
                // Skip storing empty responses (nothing useful to persist)
                if response.trim().is_empty() {
                    log::info!("[DISPATCH] Skipping empty assistant response");
                } else if let Err(e) = self.db.add_session_message(
                    session.id,
                    DbMessageRole::Assistant,
                    &response,
                    None,
                    None,
                    None,
                    Some(response_tokens),
                ) {
                    log::error!("Failed to store AI response: {}", e);
                } else {
                    // Update context tokens
                    self.context_manager.update_context_tokens(session.id, response_tokens);

                    // Check if incremental compaction is needed (earlier trigger, smaller batches)
                    if self.context_manager.needs_incremental_compaction(session.id) {
                        log::info!("[COMPACTION] Context threshold reached for session {}, triggering incremental compaction", session.id);
                        // Broadcast compaction event to UI
                        self.broadcaster.broadcast(GatewayEvent::context_compacting(
                            message.channel_id,
                            session.id,
                            "incremental",
                            "Context threshold reached",
                        ));
                        if let Err(e) = self.context_manager.compact_incremental(
                            session.id,
                            &client,
                            memory_identity,
                        ).await {
                            log::error!("[COMPACTION] Incremental compaction failed: {}", e);
                            // Fall back to full compaction if incremental fails
                            if self.context_manager.needs_compaction(session.id) {
                                log::info!("[COMPACTION] Falling back to full compaction");
                                // Broadcast fallback compaction event
                                self.broadcaster.broadcast(GatewayEvent::context_compacting(
                                    message.channel_id,
                                    session.id,
                                    "full",
                                    "Incremental failed, falling back to full compaction",
                                ));
                                if let Err(e) = self.context_manager.compact_session(
                                    session.id,
                                    &client,
                                    memory_identity,
                                ).await {
                                    log::error!("[COMPACTION] Full compaction also failed: {}", e);
                                }
                            }
                        }
                    } else if self.context_manager.needs_compaction(session.id) {
                        // Hard limit reached - do full compaction
                        log::info!("[COMPACTION] Hard context limit reached for session {}, triggering full compaction", session.id);
                        // Broadcast compaction event to UI
                        self.broadcaster.broadcast(GatewayEvent::context_compacting(
                            message.channel_id,
                            session.id,
                            "full",
                            "Hard context limit reached",
                        ));
                        if let Err(e) = self.context_manager.compact_session(
                            session.id,
                            &client,
                            memory_identity,
                        ).await {
                            log::error!("[COMPACTION] Failed to compact session: {}", e);
                        }
                    }
                }

                // Emit response event — skip if empty or if say_to_user already broadcast it
                if !response.trim().is_empty() && !delivered_via_say_to_user {
                    self.broadcaster.broadcast(GatewayEvent::agent_response(
                        message.channel_id,
                        &message.user_name,
                        &response,
                    ));
                }

                log::info!(
                    "Generated response for {} on channel {} using {} archetype",
                    message.user_name,
                    message.channel_id,
                    archetype_id
                );

                // Complete execution tracking
                self.execution_tracker.complete_execution(message.channel_id);

                // Populate attempt stats from collected spans before completing rollout
                Self::populate_attempt_stats(&mut rollout, &span_collector);

                // Complete telemetry: succeed rollout and persist spans.
                // Note: For the tool-loop path, session_completed reward is emitted
                // in finalize_tool_loop with real counts. Only emit here for non-tool path.
                self.rollout_manager.succeed_rollout(&mut rollout, response.clone());
                self.broadcaster.broadcast(GatewayEvent::rollout_status_change(
                    message.channel_id, &rollout.rollout_id, "succeeded", rollout.attempt_count(),
                ));
                if !use_tools {
                    reward_emitter.session_completed(true, 0, 0, 1);
                }
                self.telemetry_store.persist_spans(&span_collector);
                heartbeat_handle.abort();
                telemetry::clear_active_collector();

                // Safety net: if the session is still Active after a successful response,
                // mark it Complete. This catches early-return paths in the tool loop
                // that bypass finalize_tool_loop (e.g., AI responds with text after a
                // tool error without calling task_fully_completed).
                match self.db.get_session_completion_status(session.id) {
                    Ok(Some(status)) if !status.should_stop() => {
                        log::info!(
                            "[DISPATCH] Session {} still Active after successful response, marking Complete",
                            session.id
                        );
                        let _ = self.db.update_session_completion_status(
                            session.id,
                            CompletionStatus::Complete,
                        );
                        self.broadcast_session_complete(message.channel_id, session.id);
                    }
                    _ => {} // Already finalized (Complete/Failed/Cancelled) or DB error
                }

                DispatchResult::success(response)
            }
            Err(e) => {
                let mut error = format!("AI generation error ({}): {}", archetype_id, e);
                log::error!("{}", error);

                // If this is an x402 endpoint failure, check if it's due to insufficient USDC
                if crate::x402::is_x402_endpoint(&settings.endpoint) {
                    if let Some(ref wp) = self.wallet_provider {
                        let wallet_addr = wp.get_address();
                        match crate::x402::check_usdc_balance(&wallet_addr).await {
                            Ok(balance) => {
                                // 10000 raw units = 0.01 USDC (6 decimals)
                                if balance < ethers::types::U256::from(10000u64) {
                                    log::warn!(
                                        "[X402] AI call failed and USDC balance is near zero ({}) for {}",
                                        balance, wallet_addr
                                    );
                                    error = "Insufficient USDC balance for AI model payments. \
                                             Please add USDC on Base to your wallet to continue using this AI model."
                                        .to_string();
                                }
                            }
                            Err(rpc_err) => {
                                log::warn!("[X402] Failed to check USDC balance: {}", rpc_err);
                            }
                        }
                    }
                }

                // Store error as assistant message so it's visible in the session
                if let Err(db_err) = self.db.add_session_message(
                    session.id,
                    DbMessageRole::Assistant,
                    &format!("[Error] {}", error),
                    None,
                    None,
                    None,
                    None,
                ) {
                    log::error!("Failed to store error message in session: {}", db_err);
                }

                // Mark session as Failed so it doesn't stay stuck as Active with spinner
                if let Err(status_err) = self.db.update_session_completion_status(session.id, CompletionStatus::Failed) {
                    log::error!("[DISPATCH] Failed to mark session {} as Failed: {}", session.id, status_err);
                }
                self.broadcast_session_complete(message.channel_id, session.id);

                // Broadcast error to frontend
                self.broadcaster.broadcast(GatewayEvent::agent_error(
                    message.channel_id,
                    &error,
                ));

                // Complete execution tracking on error
                self.execution_tracker.complete_execution(message.channel_id);

                // Complete telemetry: persist spans (rollout already failed in retry loop).
                // Only emit session_failed reward for non-tool path; tool path handles
                // this in finalize_tool_loop with real iteration/tool counts.
                if !use_tools {
                    reward_emitter.session_completed(false, 0, 0, 1);
                }
                self.telemetry_store.persist_spans(&span_collector);
                heartbeat_handle.abort();
                telemetry::clear_active_collector();

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
        is_safe_mode: bool,
        watchdog: &Arc<Watchdog>,
    ) -> Result<(String, bool), String> {
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

        // Update the selected network from the current message
        // This ensures the agent uses the network the user has selected in the UI
        if let Some(ref network) = original_message.selected_network {
            orchestrator.context_mut().selected_network = Some(network.clone());
            log::info!("[MULTI_AGENT] Selected network set to: {}", network);
        }

        // Config-driven TaskPlanner skip: subtypes with skip_task_planner=true go straight
        // to Assistant mode (e.g. Director delegates planning to specialized agents).
        if orchestrator.current_mode() == AgentMode::TaskPlanner
            && !orchestrator.context().planner_completed
        {
            let subtype_key = orchestrator.current_subtype_key();
            let should_skip = agent_types::get_subtype_config(subtype_key)
                .map(|c| c.skip_task_planner)
                .unwrap_or(false);
            if should_skip {
                log::info!("[MULTI_AGENT] Subtype '{}' has skip_task_planner=true, going to Assistant mode", subtype_key);
                orchestrator.transition_to_assistant();
            }
        }

        // Broadcast initial mode
        let initial_mode = orchestrator.current_mode();
        self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
            original_message.channel_id,
            Some(&original_message.chat_id),
            &initial_mode.to_string(),
            initial_mode.label(),
            Some("Processing request"),
        ));

        // Broadcast initial task state
        self.broadcast_tasks_update(original_message.channel_id, session_id, &orchestrator);

        // Get the current subtype key
        let subtype_key = orchestrator.current_subtype_key().to_string();

        log::info!(
            "[MULTI_AGENT] Started in {} mode ({} subtype) for request: {}",
            initial_mode,
            agent_types::subtype_label(&subtype_key),
            original_message.text.chars().take(50).collect::<String>()
        );

        // Broadcast initial subtype
        self.broadcaster.broadcast(GatewayEvent::agent_subtype_change(
            original_message.channel_id,
            &subtype_key,
            &agent_types::subtype_label(&subtype_key),
        ));

        // Build tool list: subtype-filtered + skill requires_tools + use_skill + mode tools
        let mut tools = self.build_tool_list(tool_config, &subtype_key, &orchestrator);

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
            orchestrator.current_subtype_key(),
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
            return Ok((content, false));
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
                original_message, archetype, &mut orchestrator, session_id, is_safe_mode, watchdog
            ).await
        } else {
            self.generate_with_text_tools_orchestrated(
                client, messages, tools, tool_config, tool_context,
                original_message, archetype, &mut orchestrator, session_id, is_safe_mode, watchdog
            ).await
        }
    }
}

#[cfg(test)]
#[path = "../dispatcher_tests.rs"]
mod dispatcher_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_directive_pattern() {
        // Test the thinking directive pattern
        let pattern = &*commands::THINKING_DIRECTIVE_PATTERN;

        // Basic thinking directive
        let text = "/think";
        assert!(pattern.is_match(text));

        // With level
        let text = "/think:medium";
        let caps = pattern.captures(text).unwrap();
        assert_eq!(caps.get(1).map(|m| m.as_str()), Some("medium"));

        // Alias
        let text = "/t:high";
        let caps = pattern.captures(text).unwrap();
        assert_eq!(caps.get(1).map(|m| m.as_str()), Some("high"));
    }

    #[test]
    fn test_inline_thinking_pattern() {
        let pattern = &*commands::INLINE_THINKING_PATTERN;

        let text = "/t:medium What is the meaning of life?";
        let caps = pattern.captures(text).unwrap();
        assert_eq!(caps.get(1).map(|m| m.as_str()), Some("medium"));
        assert_eq!(caps.get(2).map(|m| m.as_str()), Some("What is the meaning of life?"));
    }
}
