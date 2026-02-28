//! Sub-agent manager for spawning and coordinating background agent instances
//!
//! This module provides:
//! - Concurrency control via semaphores
//! - Database persistence for sub-agent state
//! - Isolated session creation for sub-agents
//! - Real-time event broadcasting for sub-agent lifecycle

use crate::ai::archetypes::minimax::strip_think_blocks;
use crate::ai::multi_agent::types::{SubAgentConfig, SubAgentContext, SubAgentStatus, TaskQueue};
use crate::ai::{AiClient, Message, MessageRole, ToolHistoryEntry};
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{AgentSettings, MessageRole as DbMessageRole, SessionScope};
use crate::notes::NoteStore;
use crate::tools::{ToolContext, ToolDefinition, ToolRegistry};
use crate::skills::SkillRegistry;

use dashmap::DashMap;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::{oneshot, Semaphore};
use tokio::time::{timeout, Duration};

/// Truncate a string to at most `max_bytes` bytes at a valid UTF-8 boundary.
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest char boundary <= max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

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
    wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    /// Skill registry for sub-agent tool context
    skill_registry: OnceLock<Arc<SkillRegistry>>,
    /// Transaction queue manager for web3 tx tools
    tx_queue: OnceLock<Arc<crate::tx_queue::TxQueueManager>>,
    /// Process manager for bash execution tracking
    process_manager: OnceLock<Arc<crate::execution::ProcessManager>>,
    /// Disk quota manager for usage limits
    disk_quota: OnceLock<Arc<crate::disk_quota::DiskQuotaManager>>,
    /// Notes store for Obsidian-compatible notes
    notes_store: OnceLock<Arc<NoteStore>>,
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
            skill_registry: OnceLock::new(),
            tx_queue: OnceLock::new(),
            process_manager: OnceLock::new(),
            disk_quota: OnceLock::new(),
            notes_store: OnceLock::new(),
        }
    }

    /// Set the skill registry for sub-agent tool contexts (can be called after Arc wrapping)
    pub fn set_skill_registry(&self, registry: Arc<SkillRegistry>) {
        let _ = self.skill_registry.set(registry);
    }

    /// Set the tx queue manager for sub-agent tool contexts (can be called after Arc wrapping)
    pub fn set_tx_queue(&self, tq: Arc<crate::tx_queue::TxQueueManager>) {
        let _ = self.tx_queue.set(tq);
    }

    /// Set the process manager for sub-agent tool contexts (can be called after Arc wrapping)
    pub fn set_process_manager(&self, pm: Arc<crate::execution::ProcessManager>) {
        let _ = self.process_manager.set(pm);
    }

    /// Set the disk quota manager for sub-agent tool contexts (can be called after Arc wrapping)
    pub fn set_disk_quota(&self, dq: Arc<crate::disk_quota::DiskQuotaManager>) {
        let _ = self.disk_quota.set(dq);
    }

    /// Set the notes store for sub-agent tool contexts (can be called after Arc wrapping)
    pub fn set_notes_store(&self, store: Arc<NoteStore>) {
        let _ = self.notes_store.set(store);
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
        let skill_registry = self.skill_registry.get().cloned();
        let tx_queue = self.tx_queue.get().cloned();
        let process_manager = self.process_manager.get().cloned();
        let disk_quota = self.disk_quota.get().cloned();
        let notes_store = self.notes_store.get().cloned();
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
                tx_queue,
                process_manager,
                disk_quota,
                notes_store,
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
                    let cleaned_response = strip_think_blocks(&response);
                    final_context.mark_completed(cleaned_response.clone());
                    broadcaster.broadcast(GatewayEvent::subagent_completed(
                        final_context.parent_channel_id,
                        &final_context.id,
                        &final_context.label,
                        &cleaned_response,
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
        tx_queue: Option<Arc<crate::tx_queue::TxQueueManager>>,
        process_manager: Option<Arc<crate::execution::ProcessManager>>,
        disk_quota: Option<Arc<crate::disk_quota::DiskQuotaManager>>,
        notes_store: Option<Arc<NoteStore>>,
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
        let client = match AiClient::from_settings_with_wallet_provider(&effective_settings, wallet_provider.clone(), None) {
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

        // Search for skills relevant to this task and inject their info
        let relevant_skills_hint = {
            let matches = crate::skills::embeddings::search_skills_text(&db, &context.task, 5)
                .unwrap_or_default();

            // Filter by subtype skill tags if applicable
            let allowed_tags = context.agent_subtype.as_ref()
                .map(|key| crate::ai::multi_agent::types::allowed_skill_tags_for_key(key))
                .unwrap_or_default();

            let filtered: Vec<_> = matches.into_iter()
                .filter(|(skill, _)| {
                    skill.enabled && (allowed_tags.is_empty()
                        || skill.tags.iter().any(|t| allowed_tags.contains(t)))
                })
                .take(3)
                .collect();

            if filtered.is_empty() {
                String::new()
            } else {
                let mut hint = String::from("\n\n## Relevant Skills\nThese skills match your task. Use `use_skill` with the exact skill name to invoke them:\n");
                for (skill, score) in &filtered {
                    hint.push_str(&format!(
                        "- **{}** (relevance {:.0}%): {}\n",
                        skill.name,
                        score * 100.0,
                        skill.description
                    ));
                }
                hint.push_str("\nDo NOT invent skill names — only use names listed above or discovered via tools.");
                hint
            }
        };

        // Search for memories relevant to this task and inject into system prompt
        let relevant_memories_hint = {
            let fts_query = crate::memory::fts_utils::normalize_fts_query(&context.task);

            if fts_query.is_empty() {
                String::new()
            } else {
                // Standard mode searches all memories; safe mode handled upstream
                let mem_identity: Option<&str> = None;
                match db.search_memories_fts(&fts_query, mem_identity, 10) {
                    Ok(results) if !results.is_empty() => {
                        let mut hint = String::from("\n\n## Relevant Memories\n");
                        for (i, (mem, _rank)) in results.iter().enumerate() {
                            let snippet: String = mem.content.chars().take(200).collect();
                            hint.push_str(&format!(
                                "[{}] (#{}, {}) {}\n",
                                i + 1, mem.id, mem.memory_type,
                                snippet.replace('\n', " ")
                            ));
                        }
                        hint
                    }
                    _ => String::new(),
                }
            }
        };

        let system_prompt = format!(
            "You are a sub-agent working on a specific task. \
             Complete the following task to the best of your ability. \
             Be thorough but concise in your response.\n\n\
             IMPORTANT: Work efficiently. Aim to accomplish your goal in roughly 20-30 tool calls. \
             Do not research too deeply or go down rabbit holes. Stay focused on the specific task \
             and deliver a clear, useful result without exhaustive exploration.\n\n\
             When you have completed the task, provide a clear summary of what was accomplished.{}{}{}",
            subtype_prompt.unwrap_or_default(),
            relevant_skills_hint,
            relevant_memories_hint
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

        // Attach optional stores so sub-agent tools work (use_skill, memory_search, web3_tx, etc.)
        if let Some(registry) = skill_registry.clone() {
            tool_context = tool_context.with_skill_registry(registry);
        }
        if let Some(wp) = wallet_provider.clone() {
            tool_context = tool_context.with_wallet_provider(wp);
        }
        if let Some(tq) = tx_queue {
            tool_context = tool_context.with_tx_queue(tq);
        }
        if let Some(pm) = process_manager {
            tool_context = tool_context.with_process_manager(pm);
        }
        if let Some(dq) = disk_quota {
            tool_context = tool_context.with_disk_quota(dq);
        }
        if let Some(ns) = notes_store {
            tool_context = tool_context.with_notes_store(ns);
        }

        // Load API keys from database so sub-agent tools can call external services
        if !parent_channel_safe_mode {
            if let Ok(keys) = db.list_api_keys() {
                for key in keys {
                    tool_context = tool_context.with_api_key(&key.service_name, key.api_key.clone());
                }
            }
        }

        // SECURITY: Pass safe_mode flag to tool context so memory tools sandbox to safemode/
        if parent_channel_safe_mode {
            tool_context.extra.insert(
                "safe_mode".to_string(),
                serde_json::json!(true),
            );
        }

        // Pass parent's agent_subtype to sub-agent tool context for memory localization
        if let Some(ref subtype) = context.agent_subtype {
            if !subtype.is_empty() {
                tool_context.extra.insert(
                    "agent_subtype".to_string(),
                    serde_json::json!(subtype),
                );
            }
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
        let mut tools: Vec<ToolDefinition> = tools
            .into_iter()
            .filter(|t| t.group != crate::tools::ToolGroup::SubAgent)
            .collect();

        // Task queue state for multi-task skills (e.g. swap)
        let mut task_queue: Option<TaskQueue> = None;

        // Execute the AI with tool loop
        let max_iterations = 90; // Matches default subtype config
        let mut tool_history: Vec<ToolHistoryEntry> = Vec::new();
        let mut final_response = String::new();
        let mut last_say_to_user_content = String::new();
        let mut client_error_retries = 0;
        const MAX_CLIENT_ERROR_RETRIES: u32 = 2;

        for iteration in 0..max_iterations {
            // Save checkpoint every 25 turns for overflow recovery
            if iteration > 0 && iteration % 25 == 0 {
                let checkpoint = crate::ai::multi_agent::types::WorkerCheckpoint {
                    iteration: iteration as u32,
                    context_snapshot: format!(
                        "Checkpoint at iteration {}: {} messages, {} tool calls",
                        iteration,
                        messages.len(),
                        tool_history.len()
                    ),
                    timestamp: chrono::Utc::now(),
                };
                context.checkpoints.push(checkpoint);
                if let Err(e) = Self::save_subagent_direct(&db, &context) {
                    log::warn!("[SUBAGENT] {} failed to save checkpoint: {}", context.id, e);
                } else {
                    log::info!("[SUBAGENT] {} saved checkpoint at iteration {}", context.id, iteration);
                }
            }

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
                    // Payment/infrastructure errors (402, timeouts) should NOT be retried here —
                    // the HTTP client already retried 3 times. Retrying again just causes the
                    // subagent to hang for minutes before finally failing.
                    let is_payment_or_infra = e.status_code == Some(402)
                        || e.message.contains("timed out")
                        || e.message.contains("operation timed out")
                        || e.message.contains("request failed");

                    if is_payment_or_infra {
                        log::warn!(
                            "[SUBAGENT] {} failing fast on infrastructure error (already retried at HTTP level): {}",
                            context.id,
                            e
                        );
                        return Err(format!("AI inference failed: {}", e));
                    }

                    // Context-too-large errors are recoverable by clearing history
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
            let mut tasks_defined_this_batch = false;
            let mut auto_completed_this_batch = false;
            for tool_call in &response.tool_calls {
                // Skip remaining tool calls if define_tasks replaced the queue this batch
                if tasks_defined_this_batch {
                    log::info!(
                        "[SUBAGENT] {} skipping tool '{}' — define_tasks replaced queue this batch",
                        context.id, tool_call.name
                    );
                    tool_responses.push(crate::ai::ToolResponse {
                        tool_call_id: tool_call.id.clone(),
                        content: "Task queue was just replaced by define_tasks. This tool call was not executed. \
                                  The next iteration will start with the correct task context.".to_string(),
                        is_error: true,
                    });
                    continue;
                }

                log::info!(
                    "[SUBAGENT] {} calling tool: {}",
                    context.id,
                    tool_call.name
                );

                // Broadcast tool_call event
                let params_preview = {
                    let s = tool_call.arguments.to_string();
                    if s.len() > 500 { format!("{}...", truncate_str(&s, 500)) } else { s }
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

                // Broadcast tool_result event (strip <think> blocks from preview)
                let cleaned_content = strip_think_blocks(&result.content);
                let content_preview = if cleaned_content.len() > 500 {
                    format!("{}...", truncate_str(&cleaned_content, 500))
                } else {
                    cleaned_content
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

                // Track say_to_user content so it can be preferred over task_fully_completed summary
                if tool_call.name == "say_to_user" && result.success {
                    log::info!("[SUBAGENT] {} say_to_user captured ({} chars)", context.id, result.content.len());
                    last_say_to_user_content = result.content.clone();
                }

                // Handle set_agent_subtype: refresh tool list for new subtype
                if tool_call.name == "set_agent_subtype" && result.success {
                    if let Some(ref metadata) = result.metadata {
                        if let Some(new_subtype) = metadata.get("subtype").and_then(|v| v.as_str()) {
                            tools = tool_registry
                                .get_tool_definitions_for_subtype(&tool_config, new_subtype);
                            tools.retain(|t| t.group != crate::tools::ToolGroup::SubAgent);
                            log::info!(
                                "[SUBAGENT] {} refreshed toolset after set_agent_subtype('{}'): {} tools",
                                context.id, new_subtype, tools.len()
                            );
                        }
                    }
                }

                // Handle use_skill: refresh tool list and inject skill instructions
                if tool_call.name == "use_skill" && result.success {
                    if let Some(ref metadata) = result.metadata {
                        if let Some(requires) = metadata.get("requires_tools").and_then(|v| v.as_array()) {
                            let required_tools: Vec<String> = requires.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect();
                            let subtype_key = context.agent_subtype.as_deref().unwrap_or("finance");
                            tools = tool_registry
                                .get_tool_definitions_for_subtype_with_required(&tool_config, subtype_key, &required_tools);
                            tools.retain(|t| t.group != crate::tools::ToolGroup::SubAgent);
                            log::info!(
                                "[SUBAGENT] {} refreshed toolset after use_skill: {} tools (requires {:?})",
                                context.id, tools.len(), required_tools
                            );
                        }
                    }
                    // The skill instructions are in result.content (returned by the tool).
                    // Inject them as a persistent system hint so they survive context growth.
                    if !result.content.is_empty() {
                        let skill_name = result.metadata.as_ref()
                            .and_then(|m| m.get("skill_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        log::info!(
                            "[SUBAGENT] {} injecting skill '{}' instructions as system hint ({} chars)",
                            context.id, skill_name, result.content.len()
                        );
                    }
                }

                // Process metadata-driven side effects
                if let Some(ref metadata) = result.metadata {
                    // Handle define_tasks: build task queue from skill's task list
                    if metadata.get("define_tasks").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some(tasks) = metadata.get("tasks").and_then(|v| v.as_array()) {
                            let descriptions: Vec<String> = tasks.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect();
                            if !descriptions.is_empty() {
                                let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
                                task_queue = Some(TaskQueue::from_descriptions_with_tool_matching(
                                    descriptions.clone(), &tool_names,
                                ));
                                if let Some(ref mut q) = task_queue {
                                    q.pop_next();
                                }
                                log::info!(
                                    "[SUBAGENT] {} define_tasks: created queue with {} tasks",
                                    context.id, descriptions.len()
                                );
                                tasks_defined_this_batch = true;
                            }
                        }
                    }

                    // Handle add_task: insert task into existing queue
                    if metadata.get("add_task").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some(desc) = metadata.get("task_description").and_then(|v| v.as_str()) {
                            let position = metadata.get("task_position")
                                .and_then(|v| v.as_str())
                                .unwrap_or("back");
                            if let Some(ref mut q) = task_queue {
                                if position == "front" {
                                    q.insert_after_current(vec![desc.to_string()]);
                                } else {
                                    q.append_tasks(vec![desc.to_string()]);
                                }
                                log::info!(
                                    "[SUBAGENT] {} add_task ({}): '{}' — queue now has {} tasks",
                                    context.id, position, truncate_str(desc, 80), q.total()
                                );
                            } else {
                                // No queue exists yet — create one with this single task
                                let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
                                task_queue = Some(TaskQueue::from_descriptions_with_tool_matching(
                                    vec![desc.to_string()], &tool_names,
                                ));
                                if let Some(ref mut q) = task_queue {
                                    q.pop_next();
                                }
                                log::info!(
                                    "[SUBAGENT] {} add_task created new queue with 1 task",
                                    context.id
                                );
                            }
                        }
                    }

                    // Handle task_fully_completed: advance queue or exit
                    if metadata.get("task_fully_completed").and_then(|v| v.as_bool()).unwrap_or(false) {
                        log::info!("[SUBAGENT] {} task_fully_completed called", context.id);

                        // Capture response (may be overridden later by say_to_user preference)
                        if let Some(summary) = metadata.get("summary").and_then(|v| v.as_str()) {
                            final_response = summary.to_string();
                        } else if !result.content.is_empty() {
                            final_response = result.content.clone();
                        }

                        if let Some(ref mut q) = task_queue {
                            if !tasks_defined_this_batch {
                                q.complete_current();
                                if q.pop_next().is_some() {
                                    log::info!(
                                        "[SUBAGENT] {} advanced to next task ({}/{})",
                                        context.id, q.completed_count(), q.total()
                                    );
                                } else {
                                    log::info!("[SUBAGENT] {} all tasks complete", context.id);
                                    task_completed = true;
                                }
                            }
                            // If tasks_defined_this_batch, suppress (same as dispatcher BatchState)
                        } else {
                            // No task queue — original behavior: exit immediately
                            task_completed = true;
                        }
                    }
                }

                // Auto-complete: if the successful tool matches the current task's auto_complete_tool
                if result.success && !tasks_defined_this_batch && !auto_completed_this_batch {
                    if let Some(ref mut q) = task_queue {
                        if let Some(current) = q.current_task() {
                            if current.auto_complete_tool.as_deref() == Some(&tool_call.name) {
                                log::info!(
                                    "[SUBAGENT] {} auto-completing task via tool '{}'",
                                    context.id, tool_call.name
                                );
                                q.complete_current();
                                if q.pop_next().is_some() {
                                    log::info!(
                                        "[SUBAGENT] {} auto-advanced to next task ({}/{})",
                                        context.id, q.completed_count(), q.total()
                                    );
                                } else {
                                    log::info!("[SUBAGENT] {} all tasks complete (via auto-complete)", context.id);
                                    task_completed = true;
                                }
                                auto_completed_this_batch = true;
                            }
                        }
                    }
                }

                tool_responses.push(crate::ai::ToolResponse {
                    tool_call_id: tool_call.id.clone(),
                    content: result.content.clone(),
                    is_error: !result.success,
                });
            }

            // If all tasks done, break the outer loop
            if task_completed {
                break;
            }

            // Add to history
            tool_history.push(AiClient::build_tool_history_entry(
                response.tool_calls,
                tool_responses,
            ));

            // Inject current task context so the AI knows which task to work on next
            if let Some(ref q) = task_queue {
                if let Some(current) = q.current_task() {
                    let auto_complete_hint = if let Some(ref tool_name) = current.auto_complete_tool {
                        format!(
                            "\n\n**Note:** This task will auto-complete when `{}` succeeds. \
                             You do NOT need to call `task_fully_completed` for this task.",
                            tool_name
                        )
                    } else {
                        String::new()
                    };
                    let task_hint = format!(
                        ">>> CURRENT TASK ({}/{}) <<<\n{}{}\n\nComplete ONLY this task. When done, call task_fully_completed.",
                        q.completed_count() + 1, q.total(), current.description, auto_complete_hint
                    );
                    tool_history.push(crate::ai::types::create_system_hint(&task_hint));
                }
            }

            // If there was content with the tool calls, save it
            if !response.content.is_empty() {
                final_response = response.content;
            }
        }

        // Final response priority: say_to_user > task summary > AI content > fallback
        if !last_say_to_user_content.is_empty() {
            final_response = last_say_to_user_content;
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

        // Serialize checkpoints to JSON
        let checkpoints_json = if context.checkpoints.is_empty() {
            None
        } else {
            serde_json::to_string(&context.checkpoints).ok()
        };

        if exists {
            // Update existing record
            conn.execute(
                "UPDATE sub_agents SET
                    session_id = ?1,
                    status = ?2,
                    result = ?3,
                    error = ?4,
                    completed_at = ?5,
                    mode = ?6,
                    parent_context_snapshot = ?7,
                    checkpoints = ?8
                 WHERE subagent_id = ?9",
                rusqlite::params![
                    context.session_id,
                    context.status.to_string(),
                    context.result,
                    context.error,
                    context.completed_at.map(|t| t.to_rfc3339()),
                    context.mode.to_string(),
                    context.parent_context_snapshot,
                    checkpoints_json,
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
                    parent_subagent_id, depth, mode, parent_context_snapshot, checkpoints
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
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
                    context.mode.to_string(),
                    context.parent_context_snapshot,
                    checkpoints_json,
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
                parent_subagent_id, depth, mode, parent_context_snapshot, checkpoints
             FROM sub_agents WHERE subagent_id = ?1",
            [subagent_id],
            |row| {
                let mode_str: String = row.get::<_, String>(17).unwrap_or_else(|_| "standard".to_string());
                let checkpoints_json: Option<String> = row.get(19).ok();
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
                    identity_id: None,
                    mode: mode_str.parse::<crate::ai::multi_agent::types::SubAgentMode>().unwrap_or_default(),
                    parent_context_snapshot: row.get(18).ok(),
                    checkpoints: checkpoints_json
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default(),
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
                    parent_subagent_id, depth, mode, parent_context_snapshot, checkpoints
                 FROM sub_agents
                 WHERE parent_channel_id = ?1
                 ORDER BY started_at DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([channel_id], |row| {
                let mode_str: String = row.get::<_, String>(17).unwrap_or_else(|_| "standard".to_string());
                let checkpoints_json: Option<String> = row.get(19).ok();
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
                    identity_id: None,
                    mode: mode_str.parse::<crate::ai::multi_agent::types::SubAgentMode>().unwrap_or_default(),
                    parent_context_snapshot: row.get(18).ok(),
                    checkpoints: checkpoints_json
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default(),
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
                "task": if task.len() > 200 { format!("{}...", truncate_str(task, 200)) } else { task.to_string() },
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
                "result": if result.len() > 500 { format!("{}...", truncate_str(result, 500)) } else { result.to_string() },
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
