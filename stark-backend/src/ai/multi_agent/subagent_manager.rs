//! Sub-agent manager for spawning and coordinating background agent instances
//!
//! This module provides:
//! - Concurrency control via semaphores
//! - Database persistence for sub-agent state
//! - Isolated session creation for sub-agents
//! - Real-time event broadcasting for sub-agent lifecycle

use crate::ai::multi_agent::types::{SubAgentConfig, SubAgentContext, SubAgentStatus};
use crate::ai::{AiClient, Message, MessageRole, ToolHistoryEntry};
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{AgentSettings, MessageRole as DbMessageRole, SessionScope};
use crate::tools::{ToolContext, ToolDefinition, ToolRegistry};
use crate::skills::SkillRegistry;
use crate::qmd_memory::MemoryStore;
use dashmap::DashMap;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Semaphore};
use tokio::time::{timeout, Duration};

/// Counter for generating unique sub-agent IDs
static SUBAGENT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Handle for a running sub-agent task
pub struct SubAgentHandle {
    /// Cancel signal sender
    cancel_tx: Option<oneshot::Sender<()>>,
}

impl SubAgentHandle {
    /// Cancel the sub-agent execution
    pub fn cancel(mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Manager for coordinating sub-agent execution
pub struct SubAgentManager {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    tool_registry: Arc<ToolRegistry>,
    config: SubAgentConfig,
    /// Semaphore for limiting total concurrent sub-agents
    total_semaphore: Arc<Semaphore>,
    /// Per-channel semaphores for limiting concurrent sub-agents per channel
    channel_semaphores: DashMap<i64, Arc<Semaphore>>,
    /// Active sub-agents indexed by ID (Arc-wrapped for sharing with spawned tasks)
    active_agents: Arc<DashMap<String, SubAgentHandle>>,
    /// Last activity timestamp per subagent (updated after each tool call result)
    last_activity: Arc<DashMap<String, chrono::DateTime<chrono::Utc>>>,
    /// Wallet provider for x402 payments and transaction signing
    /// Encapsulates both Standard mode (EnvWalletProvider with raw private key)
    /// and Flash mode (FlashWalletProvider with Privy proxy)
    wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    /// Skill registry for sub-agent tool context
    skill_registry: Option<Arc<SkillRegistry>>,
    /// QMD memory store for sub-agent tool context
    memory_store: Option<Arc<MemoryStore>>,
}

impl SubAgentManager {
    /// Create a new sub-agent manager
    pub fn new(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self {
        Self::new_with_config(db, broadcaster, tool_registry, SubAgentConfig::default(), None)
    }

    /// Create a new sub-agent manager with configuration and wallet provider
    /// The wallet_provider encapsulates both Standard mode (EnvWalletProvider)
    /// and Flash mode (FlashWalletProvider)
    pub fn new_with_config(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        config: SubAgentConfig,
        wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    ) -> Self {
        Self {
            db,
            broadcaster,
            tool_registry,
            total_semaphore: Arc::new(Semaphore::new(config.max_total_concurrent)),
            channel_semaphores: DashMap::new(),
            active_agents: Arc::new(DashMap::new()),
            last_activity: Arc::new(DashMap::new()),
            config,
            wallet_provider,
            skill_registry: None,
            memory_store: None,
        }
    }

    /// Set the skill registry for sub-agent tool contexts
    pub fn set_skill_registry(&mut self, registry: Arc<SkillRegistry>) {
        self.skill_registry = Some(registry);
    }

    /// Set the memory store for sub-agent tool contexts
    pub fn set_memory_store(&mut self, store: Arc<MemoryStore>) {
        self.memory_store = Some(store);
    }

    /// Generate a unique sub-agent ID
    pub fn generate_id(label: &str) -> String {
        let counter = SUBAGENT_COUNTER.fetch_add(1, Ordering::SeqCst);
        format!("subagent-{}-{}", label, counter)
    }

    /// Get or create a semaphore for a channel
    fn get_channel_semaphore(&self, channel_id: i64) -> Arc<Semaphore> {
        self.channel_semaphores
            .entry(channel_id)
            .or_insert_with(|| Arc::new(Semaphore::new(self.config.max_concurrent_per_channel)))
            .clone()
    }

    /// Spawn a new sub-agent
    ///
    /// Returns the sub-agent ID immediately. The sub-agent will execute in the background.
    pub async fn spawn(&self, mut context: SubAgentContext) -> Result<String, String> {
        let subagent_id = context.id.clone();

        // Validate timeout
        if context.timeout_secs > self.config.max_timeout_secs {
            context.timeout_secs = self.config.max_timeout_secs;
        }
        if context.timeout_secs == 0 {
            context.timeout_secs = self.config.default_timeout_secs;
        }

        // Persist the initial state
        self.save_subagent(&context)?;

        // Broadcast spawned event
        self.broadcaster.broadcast(GatewayEvent::subagent_spawned(
            context.parent_channel_id,
            &context.id,
            &context.label,
            &context.task,
            context.parent_subagent_id.as_deref(),
            context.depth,
            context.parent_session_id,
            context.agent_subtype.as_deref(),
        ));

        log::info!(
            "[SUBAGENT] Spawning sub-agent '{}' (label: {}, timeout: {}s)",
            subagent_id,
            context.label,
            context.timeout_secs
        );

        // Create cancel channel
        let (cancel_tx, cancel_rx) = oneshot::channel();

        // Store the handle
        self.active_agents.insert(
            subagent_id.clone(),
            SubAgentHandle {
                cancel_tx: Some(cancel_tx),
            },
        );

        // Clone what we need for the spawned task
        let db = self.db.clone();
        let broadcaster = self.broadcaster.clone();
        let tool_registry = self.tool_registry.clone();
        let total_sem = self.total_semaphore.clone();
        let channel_sem = self.get_channel_semaphore(context.parent_channel_id);
        let wallet_provider = self.wallet_provider.clone();
        let skill_registry = self.skill_registry.clone();
        let memory_store = self.memory_store.clone();
        let active_agents = self.active_agents.clone();
        let last_activity = self.last_activity.clone();
        let subagent_id_for_cleanup = subagent_id.clone();

        // Spawn the execution task
        tokio::spawn(async move {
            // Acquire semaphores
            let _total_permit = match total_sem.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    log::error!("[SUBAGENT] Failed to acquire total semaphore for {}", context.id);
                    return;
                }
            };
            let _channel_permit = match channel_sem.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    log::error!("[SUBAGENT] Failed to acquire channel semaphore for {}", context.id);
                    return;
                }
            };

            // Execute with timeout and cancel handling
            let execution = Self::execute_subagent(
                db.clone(),
                broadcaster.clone(),
                tool_registry.clone(),
                context.clone(),
                wallet_provider,
                skill_registry,
                memory_store,
                last_activity.clone(),
            );

            let timeout_duration = Duration::from_secs(context.timeout_secs);
            let result = tokio::select! {
                result = timeout(timeout_duration, execution) => {
                    match result {
                        Ok(r) => r,
                        Err(_) => {
                            log::warn!("[SUBAGENT] {} timed out after {}s", context.id, context.timeout_secs);
                            Err("Execution timed out".to_string())
                        }
                    }
                }
                _ = cancel_rx => {
                    log::info!("[SUBAGENT] {} was cancelled", context.id);
                    Err("Cancelled".to_string())
                }
            };

            // Update the context with result
            let mut final_context = context;
            match result {
                Ok(response) => {
                    final_context.mark_completed(response.clone());
                    broadcaster.broadcast(GatewayEvent::subagent_completed(
                        final_context.parent_channel_id,
                        &final_context.id,
                        &final_context.label,
                        &response,
                        final_context.parent_subagent_id.as_deref(),
                        final_context.depth,
                        final_context.parent_session_id,
                    ));
                }
                Err(error) => {
                    if error == "Cancelled" {
                        final_context.mark_cancelled();
                    } else if error.contains("timed out") {
                        final_context.mark_timed_out();
                    } else {
                        final_context.mark_failed(error.clone());
                    }
                    broadcaster.broadcast(GatewayEvent::subagent_failed(
                        final_context.parent_channel_id,
                        &final_context.id,
                        &final_context.label,
                        final_context.error.as_deref().unwrap_or(&error),
                        final_context.parent_subagent_id.as_deref(),
                        final_context.depth,
                        final_context.parent_session_id,
                    ));
                }
            }

            // Persist final state
            if let Err(e) = Self::save_subagent_direct(&db, &final_context) {
                log::error!("[SUBAGENT] Failed to save final state for {}: {}", final_context.id, e);
            }

            log::info!(
                "[SUBAGENT] {} completed with status: {}",
                final_context.id,
                final_context.status
            );

            // Clean up from active_agents and last_activity DashMaps
            if active_agents.remove(&subagent_id_for_cleanup).is_some() {
                log::debug!(
                    "[SUBAGENT_MANAGER] Cleaned up subagent {} from active_agents",
                    subagent_id_for_cleanup
                );
            }
            last_activity.remove(&subagent_id_for_cleanup);
        });

        Ok(subagent_id)
    }

    /// Execute a sub-agent (internal)
    async fn execute_subagent(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        mut context: SubAgentContext,
        wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
        skill_registry: Option<Arc<SkillRegistry>>,
        memory_store: Option<Arc<MemoryStore>>,
        last_activity: Arc<DashMap<String, chrono::DateTime<chrono::Utc>>>,
    ) -> Result<String, String> {
        log::info!("[SUBAGENT] Starting execution for {}", context.id);

        // Create an isolated session for the sub-agent
        let session_key = format!("subagent:{}:{}", context.parent_channel_id, context.id);
        let session = db
            .get_or_create_chat_session(
                "subagent",
                context.parent_channel_id,
                &session_key,
                SessionScope::Dm,
                None,
            )
            .map_err(|e| format!("Failed to create session: {}", e))?;

        // Mark as running with session ID
        context.mark_running(session.id);
        Self::save_subagent_direct(&db, &context)?;

        // Broadcast session_ready so the frontend can link to the session
        broadcaster.broadcast(GatewayEvent::subagent_session_ready(
            context.parent_channel_id,
            &context.id,
            session.id,
        ));

        // Get agent settings
        let settings = db
            .get_active_agent_settings()
            .map_err(|e| format!("Failed to get agent settings: {}", e))?
            .unwrap_or_default();

        // Apply model override if specified
        let effective_settings = if let Some(ref model) = context.model_override {
            AgentSettings {
                model_archetype: model.clone(),
                ..settings
            }
        } else {
            settings
        };

        // Create AI client with wallet provider for x402 payments
        let client = match AiClient::from_settings_with_wallet_provider(&effective_settings, wallet_provider.clone()) {
            Ok(c) => c.with_broadcaster(Arc::clone(&broadcaster), context.parent_channel_id),
            Err(e) => return Err(format!("Failed to create AI client: {}", e)),
        };

        // Build the task prompt
        let mut task_prompt = context.task.clone();
        if let Some(ref additional_context) = context.context {
            task_prompt = format!("{}\n\n## Additional Context:\n{}", task_prompt, additional_context);
        }

        // Build system prompt for sub-agent — include subtype prompt if set
        let subtype_prompt = context.agent_subtype.as_ref().and_then(|key| {
            crate::ai::multi_agent::types::get_subtype_config(key).map(|config| {
                format!(
                    "\n\n## Agent Role: {} {}\n{}\n",
                    config.emoji, config.label, config.prompt
                )
            })
        });

        let system_prompt = format!(
            "You are a sub-agent working on a specific task. \
             Complete the following task to the best of your ability. \
             Be thorough but concise in your response.\n\n\
             IMPORTANT: Work efficiently. Aim to accomplish your goal in roughly 20-30 tool calls. \
             Do not research too deeply or go down rabbit holes. Stay focused on the specific task \
             and deliver a clear, useful result without exhaustive exploration.\n\n\
             When you have completed the task, provide a clear summary of what was accomplished.{}",
            subtype_prompt.unwrap_or_default()
        );

        // Build messages
        let messages = vec![
            Message {
                role: MessageRole::System,
                content: system_prompt,
            },
            Message {
                role: MessageRole::User,
                content: task_prompt.clone(),
            },
        ];

        // Persist the task prompt as a User message in the session
        let _ = db.add_session_message(
            session.id,
            DbMessageRole::User,
            &task_prompt,
            None,
            Some("subagent"),
            None,
            None,
        );

        // SECURITY: Check if parent channel is in safe mode BEFORE building tool context
        let parent_channel_safe_mode = db
            .get_channel(context.parent_channel_id)
            .ok()
            .flatten()
            .map(|ch| ch.safe_mode)
            .unwrap_or(false);

        // Build tool context
        let workspace_dir = crate::config::workspace_dir();
        let mut tool_context = ToolContext::new()
            .with_channel(context.parent_channel_id, "subagent".to_string())
            .with_session(session.id)
            .with_workspace(workspace_dir)
            .with_broadcaster(broadcaster.clone())
            .with_database(db.clone())
            .with_subagent_identity(context.id.clone(), context.depth);

        // Attach optional stores so sub-agent tools (use_skill, manage_skills, memory_search, etc.) work
        if let Some(registry) = skill_registry.clone() {
            tool_context = tool_context.with_skill_registry(registry);
        }
        if let Some(store) = memory_store.clone() {
            tool_context = tool_context.with_memory_store(store);
        }
        if let Some(wp) = wallet_provider.clone() {
            tool_context = tool_context.with_wallet_provider(wp);
        }

        // SECURITY: Pass safe_mode flag to tool context so memory tools sandbox to safemode/
        if parent_channel_safe_mode {
            tool_context.extra.insert(
                "safe_mode".to_string(),
                serde_json::json!(true),
            );
        }

        // Get tool configuration — enforce safe mode and read_only restrictions
        let mut tool_config = db
            .get_effective_tool_config(Some(context.parent_channel_id))
            .unwrap_or_default();

        // SECURITY: If parent channel is in safe mode, override to safe mode config.
        // Defense-in-depth — the subagent tool shouldn't be callable in safe mode,
        // but if we ever get here, enforce the restriction.
        if parent_channel_safe_mode {
            log::info!(
                "[SUBAGENT] {} parent channel is safe mode — restricting to safe mode tools + sandboxed memory",
                context.id
            );
            tool_config = crate::tools::ToolConfig::safe_mode();
        }

        // Get available tools — filtered by safety level and agent subtype
        let tools: Vec<ToolDefinition> = if parent_channel_safe_mode {
            tool_registry.get_tool_definitions_at_safety_level(
                &tool_config,
                crate::tools::ToolSafetyLevel::SafeMode,
            )
        } else if context.read_only {
            log::info!(
                "[SUBAGENT] {} is read_only — restricting to read-only tools",
                context.id
            );
            tool_registry.get_tool_definitions_at_safety_level(
                &tool_config,
                crate::tools::ToolSafetyLevel::ReadOnly,
            )
        } else if let Some(ref subtype_key) = context.agent_subtype {
            log::info!(
                "[SUBAGENT] {} using agent_subtype '{}' — filtering tools by subtype",
                context.id,
                subtype_key
            );
            tool_registry.get_tool_definitions_for_subtype(&tool_config, subtype_key)
        } else {
            tool_registry.get_tool_definitions(&tool_config)
        };

        // Filter out SubAgent tools — sub-agents don't get spawn_subagents
        // (recursion is controlled by which subtypes include the SubAgent tool group)
        let tools: Vec<ToolDefinition> = tools
            .into_iter()
            .filter(|t| t.group != crate::tools::ToolGroup::SubAgent)
            .collect();

        // Execute the AI with tool loop
        let max_iterations = 90; // Matches default subtype config
        let mut tool_history: Vec<ToolHistoryEntry> = Vec::new();
        let mut final_response = String::new();
        let mut client_error_retries = 0;
        const MAX_CLIENT_ERROR_RETRIES: u32 = 2;

        for iteration in 0..max_iterations {
            log::debug!(
                "[SUBAGENT] {} iteration {} starting",
                context.id,
                iteration + 1
            );

            // Generate response
            let response = match client
                .generate_with_tools(messages.clone(), tool_history.clone(), tools.clone())
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // Check if this is a client error (4xx) that the AI might be able to recover from
                    if e.is_client_error() && client_error_retries < MAX_CLIENT_ERROR_RETRIES {
                        client_error_retries += 1;

                        if e.is_context_too_large() {
                            log::warn!(
                                "[SUBAGENT] {} context too large (retry {}/{}), clearing tool history ({} entries)",
                                context.id,
                                client_error_retries,
                                MAX_CLIENT_ERROR_RETRIES,
                                tool_history.len()
                            );
                            let recovery_entry = crate::ai::types::handle_context_overflow(
                                &mut tool_history,
                                &iteration.to_string(),
                            );
                            tool_history.push(recovery_entry);
                            continue;
                        }

                        // Other client errors - add guidance but don't clear history
                        log::warn!(
                            "[SUBAGENT] {} got client error (retry {}/{}): {}",
                            context.id,
                            client_error_retries,
                            MAX_CLIENT_ERROR_RETRIES,
                            e
                        );
                        tool_history.push(crate::ai::types::create_error_feedback(&e, &iteration.to_string()));
                        continue;
                    }

                    return Err(format!("AI generation failed: {}", e));
                }
            };

            // Check if we have tool calls
            if response.tool_calls.is_empty() {
                // No tool calls - we're done
                final_response = response.content;
                break;
            }

            // Execute tool calls
            let mut tool_responses = Vec::new();
            let mut task_completed = false;
            for tool_call in &response.tool_calls {
                log::info!(
                    "[SUBAGENT] {} calling tool: {}",
                    context.id,
                    tool_call.name
                );

                // Broadcast tool_call event
                let params_preview = {
                    let s = tool_call.arguments.to_string();
                    if s.len() > 500 { format!("{}...", &s[..500]) } else { s }
                };
                broadcaster.broadcast(GatewayEvent::subagent_tool_call(
                    context.parent_channel_id,
                    &context.id,
                    &context.label,
                    &tool_call.name,
                    &params_preview,
                    session.id,
                ));

                // Persist tool_call to session
                let tool_call_content = format!(
                    "**Tool Call:** {}\n```json\n{}\n```",
                    tool_call.name, params_preview
                );
                let _ = db.add_session_message(
                    session.id,
                    DbMessageRole::ToolCall,
                    &tool_call_content,
                    None,
                    None,
                    None,
                    None,
                );

                let result = tool_registry
                    .execute(&tool_call.name, tool_call.arguments.clone(), &tool_context, Some(&tool_config))
                    .await;

                // Broadcast tool_result event
                let content_preview = if result.content.len() > 500 {
                    format!("{}...", &result.content[..500])
                } else {
                    result.content.clone()
                };
                broadcaster.broadcast(GatewayEvent::subagent_tool_result(
                    context.parent_channel_id,
                    &context.id,
                    &context.label,
                    &tool_call.name,
                    result.success,
                    &content_preview,
                    session.id,
                ));

                // Update last activity timestamp for heartbeat tracking
                last_activity.insert(context.id.clone(), chrono::Utc::now());

                // Persist tool_result to session
                let status_label = if result.success { "Result" } else { "Error" };
                let tool_result_content = format!(
                    "**{}:** {}\n{}",
                    status_label, tool_call.name, content_preview
                );
                let _ = db.add_session_message(
                    session.id,
                    DbMessageRole::ToolResult,
                    &tool_result_content,
                    None,
                    None,
                    None,
                    None,
                );

                // Check if task_fully_completed was called - stop the loop
                if let Some(ref metadata) = result.metadata {
                    if metadata.get("task_fully_completed").and_then(|v| v.as_bool()).unwrap_or(false) {
                        log::info!("[SUBAGENT] {} task_fully_completed called, stopping loop", context.id);
                        if let Some(summary) = metadata.get("summary").and_then(|v| v.as_str()) {
                            final_response = summary.to_string();
                        } else if !result.content.is_empty() {
                            final_response = result.content.clone();
                        }
                        task_completed = true;
                    }
                }

                tool_responses.push(crate::ai::ToolResponse {
                    tool_call_id: tool_call.id.clone(),
                    content: result.content.clone(),
                    is_error: !result.success,
                });
            }

            // If task_fully_completed was called, break the loop immediately
            if task_completed {
                break;
            }

            // Add to history
            tool_history.push(AiClient::build_tool_history_entry(
                response.tool_calls,
                tool_responses,
            ));

            // If there was content with the tool calls, save it
            if !response.content.is_empty() {
                final_response = response.content;
            }
        }

        if final_response.is_empty() {
            final_response = "Task completed (no explicit response generated)".to_string();
        }

        // Persist the final response as an Assistant message
        let _ = db.add_session_message(
            session.id,
            DbMessageRole::Assistant,
            &final_response,
            None,
            Some("subagent"),
            None,
            None,
        );

        Ok(final_response)
    }

    /// Save sub-agent state to database
    fn save_subagent(&self, context: &SubAgentContext) -> Result<(), String> {
        Self::save_subagent_direct(&self.db, context)
    }

    /// Save sub-agent state to database (static version)
    fn save_subagent_direct(db: &Database, context: &SubAgentContext) -> Result<(), String> {
        let conn = db.conn();

        // Check if record exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sub_agents WHERE subagent_id = ?1",
                [&context.id],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            // Update existing record
            conn.execute(
                "UPDATE sub_agents SET
                    session_id = ?1,
                    status = ?2,
                    result = ?3,
                    error = ?4,
                    completed_at = ?5
                 WHERE subagent_id = ?6",
                rusqlite::params![
                    context.session_id,
                    context.status.to_string(),
                    context.result,
                    context.error,
                    context.completed_at.map(|t| t.to_rfc3339()),
                    context.id,
                ],
            )
            .map_err(|e| format!("Failed to update sub-agent: {}", e))?;
        } else {
            // Insert new record
            conn.execute(
                "INSERT INTO sub_agents (
                    subagent_id, parent_session_id, parent_channel_id, session_id,
                    label, task, status, model_override, thinking_level, timeout_secs,
                    context, result, error, started_at, completed_at,
                    parent_subagent_id, depth
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                rusqlite::params![
                    context.id,
                    context.parent_session_id,
                    context.parent_channel_id,
                    context.session_id,
                    context.label,
                    context.task,
                    context.status.to_string(),
                    context.model_override,
                    context.thinking_level,
                    context.timeout_secs as i64,
                    context.context,
                    context.result,
                    context.error,
                    context.started_at.to_rfc3339(),
                    context.completed_at.map(|t| t.to_rfc3339()),
                    context.parent_subagent_id,
                    context.depth,
                ],
            )
            .map_err(|e| format!("Failed to insert sub-agent: {}", e))?;
        }

        Ok(())
    }

    /// Get the status of a sub-agent by ID
    pub fn get_status(&self, subagent_id: &str) -> Result<Option<SubAgentContext>, String> {
        let conn = self.db.conn();

        let result = conn.query_row(
            "SELECT
                subagent_id, parent_session_id, parent_channel_id, session_id,
                label, task, status, model_override, thinking_level, timeout_secs,
                context, result, error, started_at, completed_at,
                parent_subagent_id, depth
             FROM sub_agents WHERE subagent_id = ?1",
            [subagent_id],
            |row| {
                Ok(SubAgentContext {
                    id: row.get(0)?,
                    parent_session_id: row.get(1)?,
                    parent_channel_id: row.get(2)?,
                    session_id: row.get(3)?,
                    label: row.get(4)?,
                    task: row.get(5)?,
                    status: SubAgentStatus::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                    model_override: row.get(7)?,
                    thinking_level: row.get(8)?,
                    timeout_secs: row.get::<_, i64>(9)? as u64,
                    context: row.get(10)?,
                    result: row.get(11)?,
                    error: row.get(12)?,
                    started_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    completed_at: row
                        .get::<_, Option<String>>(14)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc)),
                    read_only: false,
                    parent_subagent_id: row.get(15)?,
                    depth: row.get::<_, i64>(16).unwrap_or(0) as u32,
                    agent_subtype: None,
                })
            },
        );

        match result {
            Ok(ctx) => Ok(Some(ctx)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get sub-agent status: {}", e)),
        }
    }

    /// List all sub-agents for a channel
    pub fn list_by_channel(&self, channel_id: i64) -> Result<Vec<SubAgentContext>, String> {
        let conn = self.db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT
                    subagent_id, parent_session_id, parent_channel_id, session_id,
                    label, task, status, model_override, thinking_level, timeout_secs,
                    context, result, error, started_at, completed_at,
                    parent_subagent_id, depth
                 FROM sub_agents
                 WHERE parent_channel_id = ?1
                 ORDER BY started_at DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([channel_id], |row| {
                Ok(SubAgentContext {
                    id: row.get(0)?,
                    parent_session_id: row.get(1)?,
                    parent_channel_id: row.get(2)?,
                    session_id: row.get(3)?,
                    label: row.get(4)?,
                    task: row.get(5)?,
                    status: SubAgentStatus::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                    model_override: row.get(7)?,
                    thinking_level: row.get(8)?,
                    timeout_secs: row.get::<_, i64>(9)? as u64,
                    context: row.get(10)?,
                    result: row.get(11)?,
                    error: row.get(12)?,
                    started_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(13)?)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    completed_at: row
                        .get::<_, Option<String>>(14)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc)),
                    read_only: false,
                    parent_subagent_id: row.get(15)?,
                    depth: row.get::<_, i64>(16).unwrap_or(0) as u32,
                    agent_subtype: None,
                })
            })
            .map_err(|e| format!("Failed to execute query: {}", e))?;

        let mut agents = Vec::new();
        for row in rows {
            agents.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }

        Ok(agents)
    }

    /// Cancel a running sub-agent
    pub fn cancel(&self, subagent_id: &str) -> Result<bool, String> {
        if let Some((_, handle)) = self.active_agents.remove(subagent_id) {
            handle.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Cancel all running sub-agents
    /// Returns the number of agents cancelled
    pub fn cancel_all(&self) -> usize {
        let mut count = 0;
        // Collect all IDs first to avoid holding the lock during cancellation
        let agent_ids: Vec<String> = self.active_agents.iter().map(|entry| entry.key().clone()).collect();

        for id in agent_ids {
            if let Some((_, handle)) = self.active_agents.remove(&id) {
                log::info!("[SUBAGENT_MANAGER] Cancelling subagent: {}", id);
                handle.cancel();
                count += 1;
            }
        }

        log::info!("[SUBAGENT_MANAGER] Cancelled {} running subagents", count);
        count
    }

    /// Cancel all running sub-agents for a specific channel
    /// Returns the number of agents cancelled
    pub fn cancel_all_for_channel(&self, channel_id: i64) -> usize {
        let mut count = 0;
        // We need to check which agents belong to this channel
        // Since we store channel_id in the handle context, we check the database
        if let Ok(agents) = self.list_by_channel(channel_id) {
            for agent in agents {
                if agent.status == SubAgentStatus::Running {
                    if let Some((_, handle)) = self.active_agents.remove(&agent.id) {
                        log::info!("[SUBAGENT_MANAGER] Cancelling subagent {} for channel {}", agent.id, channel_id);
                        handle.cancel();
                        count += 1;
                    }
                }
            }
        }

        log::info!("[SUBAGENT_MANAGER] Cancelled {} running subagents for channel {}", count, channel_id);
        count
    }

    /// Cancel all running sub-agents for a specific channel and wait briefly for cleanup
    /// Returns the number of agents cancelled
    pub async fn cancel_all_for_channel_and_wait(&self, channel_id: i64, wait_duration: Duration) -> usize {
        let count = self.cancel_all_for_channel(channel_id);

        if count > 0 {
            // Brief wait for cancellation signals to be processed
            tokio::time::sleep(wait_duration).await;
            log::info!(
                "[SUBAGENT_MANAGER] Waited {:?} for {} subagent(s) to acknowledge cancellation",
                wait_duration,
                count
            );
        }

        count
    }

    /// Get count of active (running) sub-agents
    pub fn active_count(&self) -> usize {
        self.active_agents.len()
    }

    /// Get count of active sub-agents for a specific channel
    /// Note: This returns total active count as we don't track channel per handle.
    /// The per-channel semaphore enforces the actual limit.
    pub fn active_count_for_channel(&self, _channel_id: i64) -> usize {
        // We use the total count as an approximation since we don't
        // store channel_id in the handle. The per-channel semaphore
        // handles the actual concurrency limiting.
        self.active_agents.len()
    }

    /// Get the last activity timestamp for a sub-agent (updated after each tool call)
    pub fn get_last_activity(&self, subagent_id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        self.last_activity.get(subagent_id).map(|v| *v)
    }
}

// Add new gateway events for sub-agents
impl GatewayEvent {
    /// Sub-agent spawned and starting execution
    pub fn subagent_spawned(
        channel_id: i64,
        subagent_id: &str,
        label: &str,
        task: &str,
        parent_subagent_id: Option<&str>,
        depth: u32,
        session_id: i64,
        agent_subtype: Option<&str>,
    ) -> Self {
        Self::new(
            "subagent.spawned",
            json!({
                "channel_id": channel_id,
                "subagent_id": subagent_id,
                "label": label,
                "task": if task.len() > 200 { format!("{}...", &task[..200]) } else { task.to_string() },
                "parent_subagent_id": parent_subagent_id,
                "depth": depth,
                "session_id": session_id,
                "agent_subtype": agent_subtype,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Sub-agent completed successfully
    pub fn subagent_completed(
        channel_id: i64,
        subagent_id: &str,
        label: &str,
        result: &str,
        parent_subagent_id: Option<&str>,
        depth: u32,
        session_id: i64,
    ) -> Self {
        Self::new(
            "subagent.completed",
            json!({
                "channel_id": channel_id,
                "subagent_id": subagent_id,
                "label": label,
                "result": if result.len() > 500 { format!("{}...", &result[..500]) } else { result.to_string() },
                "parent_subagent_id": parent_subagent_id,
                "depth": depth,
                "session_id": session_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Sub-agent failed
    pub fn subagent_failed(
        channel_id: i64,
        subagent_id: &str,
        label: &str,
        error: &str,
        parent_subagent_id: Option<&str>,
        depth: u32,
        session_id: i64,
    ) -> Self {
        Self::new(
            "subagent.failed",
            json!({
                "channel_id": channel_id,
                "subagent_id": subagent_id,
                "label": label,
                "error": error,
                "parent_subagent_id": parent_subagent_id,
                "depth": depth,
                "session_id": session_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Sub-agent session is ready (session_id now available)
    pub fn subagent_session_ready(
        channel_id: i64,
        subagent_id: &str,
        session_id: i64,
    ) -> Self {
        Self::new(
            "subagent.session_ready",
            json!({
                "channel_id": channel_id,
                "subagent_id": subagent_id,
                "session_id": session_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Sub-agent is calling a tool
    pub fn subagent_tool_call(
        channel_id: i64,
        subagent_id: &str,
        label: &str,
        tool_name: &str,
        params_preview: &str,
        session_id: i64,
    ) -> Self {
        Self::new(
            "subagent.tool_call",
            json!({
                "channel_id": channel_id,
                "subagent_id": subagent_id,
                "label": label,
                "tool_name": tool_name,
                "params_preview": params_preview,
                "session_id": session_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Sub-agent tool call completed
    pub fn subagent_tool_result(
        channel_id: i64,
        subagent_id: &str,
        label: &str,
        tool_name: &str,
        success: bool,
        content_preview: &str,
        session_id: i64,
    ) -> Self {
        Self::new(
            "subagent.tool_result",
            json!({
                "channel_id": channel_id,
                "subagent_id": subagent_id,
                "label": label,
                "tool_name": tool_name,
                "success": success,
                "content_preview": content_preview,
                "session_id": session_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }
}
