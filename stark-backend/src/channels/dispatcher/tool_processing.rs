use crate::ai::multi_agent::{
    types::{self as agent_types, AgentMode},
    Orchestrator, ProcessResult as OrchestratorResult,
};
use crate::channels::types::NormalizedMessage;
use crate::gateway::protocol::GatewayEvent;
use crate::models::session_message::MessageRole as DbMessageRole;
use crate::telemetry::{self, Watchdog};
use crate::tools::{ToolConfig, ToolContext, ToolDefinition};
use serde_json::Value;
use std::sync::Arc;

use super::finalization::TaskAdvanceResult;
use super::MessageDispatcher;

/// Mutable state within one batch of tool calls (one AI response).
/// Native path: spans multiple tool calls. Text path: spans one.
pub(super) struct BatchState {
    pub(super) define_tasks_replaced_queue: bool,
    pub(super) auto_completed_task: bool,
    /// Tracks whether say_to_user was already broadcast in this batch.
    /// Prevents duplicate messages when AI calls say_to_user multiple times
    /// in a single response.
    pub(super) had_say_to_user: bool,
}

impl BatchState {
    pub(super) fn new() -> Self {
        Self {
            define_tasks_replaced_queue: false,
            auto_completed_task: false,
            had_say_to_user: false,
        }
    }
}

/// Result from processing a single tool call through the shared pipeline.
pub(super) struct ToolCallProcessed {
    /// The tool result content string
    pub(super) result_content: String,
    /// Whether the tool execution succeeded
    pub(super) success: bool,
    /// Whether the orchestrator signaled completion
    pub(super) orchestrator_complete: bool,
    /// Summary from orchestrator completion or task_fully_completed
    pub(super) final_summary: Option<String>,
    /// Whether a tool requires user response (e.g., ask_user)
    pub(super) waiting_for_user_response: bool,
    /// Content to return when waiting for user response
    pub(super) user_question_content: Option<String>,
}

impl MessageDispatcher {
    /// Processes a single tool call: logging, orchestrator dispatch, skill handling,
    /// subtype checks, validators, execution, metadata processing (define_tasks,
    /// task_fully_completed, say_to_user, auto-complete), hooks, and DB persistence.
    ///
    /// Returns `ToolCallProcessed` with the result content and loop-control flags.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn process_tool_call_result(
        &self,
        tool_name: &str,
        tool_arguments: &Value,
        tool_config: &ToolConfig,
        tool_context: &ToolContext,
        original_message: &NormalizedMessage,
        session_id: i64,
        is_safe_mode: bool,
        // Mutable shared state
        tools: &mut Vec<ToolDefinition>,
        batch_state: &mut BatchState,
        last_say_to_user_content: &mut String,
        memory_suppressed: &mut bool,
        tool_call_log: &mut Vec<String>,
        orchestrator: &mut Orchestrator,
        // The current tools visible to the AI this iteration (for subtype check)
        current_tools: &[ToolDefinition],
        watchdog: &Arc<Watchdog>,
    ) -> ToolCallProcessed {
        let args_pretty = serde_json::to_string_pretty(tool_arguments)
            .unwrap_or_else(|_| tool_arguments.to_string());

        log::info!(
            "[TOOL_CALL] Agent calling tool '{}' with args:\n{}",
            tool_name,
            args_pretty
        );

        tool_call_log.push(format!(
            "🔧 **Tool Call:** `{}`\n```json\n{}\n```",
            tool_name,
            args_pretty
        ));

        if crate::tools::types::is_memory_excluded_tool(tool_name) {
            *memory_suppressed = true;
        }

        self.broadcaster.broadcast(GatewayEvent::agent_tool_call(
            original_message.channel_id,
            Some(&original_message.chat_id),
            tool_name,
            tool_arguments,
        ));

        // Save tool call to session via async writer (non-blocking)
        let tool_call_content = format!(
            "🔧 **Tool Call:** `{}`\n```json\n{}\n```",
            tool_name,
            args_pretty
        );
        self.session_writer.send(
            session_id,
            DbMessageRole::ToolCall,
            tool_call_content,
            Some(tool_name),
        );

        // If define_tasks just replaced the queue, skip all remaining tool calls.
        if batch_state.define_tasks_replaced_queue {
            log::info!(
                "[ORCHESTRATED_LOOP] Skipping tool '{}' — define_tasks replaced the queue this batch",
                tool_name
            );
            return ToolCallProcessed {
                result_content: "⚠️ Task queue was just replaced by define_tasks. This tool call was not executed. \
                     The next iteration will start with the correct task context.".to_string(),
                success: false,
                orchestrator_complete: false,
                final_summary: None,
                waiting_for_user_response: false,
                user_question_content: None,
            };
        }

        // Check if this is an orchestrator tool
        let orchestrator_result = orchestrator.process_tool_result(tool_name, tool_arguments);

        let mut processed = ToolCallProcessed {
            result_content: String::new(),
            success: true,
            orchestrator_complete: false,
            final_summary: None,
            waiting_for_user_response: false,
            user_question_content: None,
        };

        match orchestrator_result {
            OrchestratorResult::Complete(summary) => {
                log::info!("[ORCHESTRATOR] Execution complete: {}", summary);
                processed.orchestrator_complete = true;
                processed.final_summary = Some(summary.clone());
                processed.result_content = format!("Execution complete: {}", summary);
                // Broadcast task list update after orchestrator tool processing
                self.broadcast_tasks_update(original_message.channel_id, session_id, orchestrator);
                return processed;
            }
            OrchestratorResult::ToolResult(result) => {
                processed.result_content = result;
                self.broadcast_tasks_update(original_message.channel_id, session_id, orchestrator);
                return processed;
            }
            OrchestratorResult::Error(err) => {
                processed.result_content = err;
                processed.success = false;
                self.broadcast_tasks_update(original_message.channel_id, session_id, orchestrator);
                return processed;
            }
            OrchestratorResult::Continue => {
                // Not an orchestrator tool, execute normally below
            }
        }

        // Broadcast that tool is starting execution
        self.broadcaster.broadcast(GatewayEvent::tool_execution(
            original_message.channel_id,
            tool_name,
            tool_arguments,
        ));

        // Pre-checks for use_skill: guard against disallowed skills and redundant reloads
        let skill_pre_check_result = if tool_name == "use_skill" {
            let requested_skill = tool_arguments.get("skill_name")
                .or_else(|| tool_arguments.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Guard: use_skill must be in the current tool list for this context
            let use_skill_def = current_tools.iter().find(|t| t.name == "use_skill");
            if use_skill_def.is_none() {
                log::warn!(
                    "[SKILL] Blocked use_skill call — not available for current subtype '{}'",
                    orchestrator.current_subtype_key()
                );
                Some(crate::tools::ToolResult::error(
                    "use_skill is not available in the current toolbox. Switch to the appropriate subtype first with set_agent_subtype."
                ))
            } else {
                // Guard: requested skill must be in the allowed enum_values
                let allowed_skills = use_skill_def
                    .and_then(|d| d.input_schema.properties.get("skill_name"))
                    .and_then(|p| p.enum_values.as_ref());
                let skill_allowed = allowed_skills
                    .map(|names| names.iter().any(|n| n == requested_skill))
                    .unwrap_or(false);
                if !skill_allowed {
                    log::warn!(
                        "[SKILL] Blocked skill '{}' — not in allowed list for subtype '{}' (safe_mode={}, profile={:?})",
                        requested_skill,
                        orchestrator.current_subtype_key(),
                        is_safe_mode,
                        tool_config.profile,
                    );
                    let allowed_list = allowed_skills
                        .map(|names| names.join(", "))
                        .unwrap_or_else(|| "none".to_string());
                    Some(crate::tools::ToolResult::error(format!(
                        "Skill '{}' is not available in the current context. Available skills: {}",
                        requested_skill, allowed_list
                    )))
                } else {
                    // Check if already active — avoid redundant reloads
                    let already_active = orchestrator.context().active_skill
                        .as_ref()
                        .map(|s| s.name == requested_skill)
                        .unwrap_or(false);

                    if already_active {
                        let input = tool_arguments.get("input").or_else(|| tool_arguments.get("inputs")).and_then(|v| v.as_str()).unwrap_or("");
                        log::info!(
                            "[SKILL] Skill '{}' already active, skipping redundant reload",
                            requested_skill
                        );

                        // Ensure subtype is set even if skill was pre-activated without it
                        if orchestrator.current_subtype().is_none() {
                            if let Ok(Some(skill)) = self.db.get_enabled_skill_by_name(requested_skill) {
                                if let Some(new_key) = self.apply_skill_subtype(&skill, orchestrator, original_message.channel_id) {
                                    *tools = self.build_tool_list(tool_config, &new_key, orchestrator);
                                    log::info!(
                                        "[SKILL] Late subtype fix: refreshed toolset to {} with {} tools",
                                        agent_types::subtype_label(&new_key),
                                        tools.len()
                                    );
                                }
                            }
                        }

                        Some(crate::tools::ToolResult::success(&format!(
                            "Skill '{}' is already loaded. Follow the instructions already provided and call the actual tools directly. Do NOT call use_skill again.\n\nUser query: {}",
                            requested_skill, input
                        )))
                    } else {
                        None // Allowed and not already active — proceed to normal execution
                    }
                }
            }
        } else {
            None
        };

        let result = if let Some(result) = skill_pre_check_result {
            result
        } else {
            // Normal execution path for all tools (including use_skill)
            // Check if subtype is None - allow System tools and skill-required tools,
            // but block everything else until a subtype is selected
            let is_system_tool = current_tools.iter().any(|t| t.name == tool_name && t.group == crate::tools::types::ToolGroup::System);
            let is_skill_required_tool = orchestrator.context().active_skill.as_ref()
                .map_or(false, |s| s.requires_tools.iter().any(|t| t == tool_name));
            if orchestrator.current_subtype().is_none() && !is_system_tool && !is_skill_required_tool {
                log::warn!(
                    "[SUBTYPE] Blocked tool '{}' - no subtype selected. Must call set_agent_subtype first.",
                    tool_name
                );
                crate::tools::ToolResult::error(format!(
                    "❌ No toolbox selected! You MUST call `set_agent_subtype` FIRST before using '{}'.\n\n\
                    Choose based on the user's request:\n\
                    • set_agent_subtype(subtype=\"finance\") - for crypto/DeFi/tipping operations\n\
                    • set_agent_subtype(subtype=\"code_engineer\") - for code/git operations\n\
                    • set_agent_subtype(subtype=\"secretary\") - for social/messaging",
                    tool_name
                ))
            } else {
                // If a skill is active and requires this tool (and we're not in safe mode),
                // create a config override that allows execution regardless of profile/group.
                let skill_requires_this_tool = !is_safe_mode
                    && orchestrator.context().active_skill.as_ref()
                        .map_or(false, |s| s.requires_tools.iter().any(|t| t == tool_name));
                let effective_config;
                let exec_config = if skill_requires_this_tool {
                    effective_config = {
                        let mut c = tool_config.clone();
                        if !c.allow_list.iter().any(|t| t == tool_name) {
                            c.allow_list.push(tool_name.to_string());
                        }
                        c
                    };
                    &effective_config
                } else {
                    tool_config
                };

                // Run tool validators before execution
                if let Some(ref validator_registry) = self.validator_registry {
                    let validation_ctx = crate::tool_validators::ValidationContext::new(
                        tool_name.to_string(),
                        tool_arguments.clone(),
                        Arc::new(tool_context.clone()),
                    );
                    let validation_result = validator_registry.validate(&validation_ctx).await;
                    if let Some(error_msg) = validation_result.to_error_message() {
                        // Emit a skipped tool span for validator rejection
                        telemetry::emit_annotation("tool_validator_rejected", serde_json::json!({
                            "tool_name": tool_name,
                            "error": error_msg,
                        }));
                        crate::tools::ToolResult::error(error_msg)
                    } else {
                        let start = std::time::Instant::now();
                        let tool_result = match watchdog.guard_tool_call(
                            tool_name,
                            self.tool_registry.execute(tool_name, tool_arguments.clone(), tool_context, Some(exec_config)),
                        ).await {
                            Some(result) => result,
                            None => crate::tools::ToolResult::error(format!(
                                "Tool '{}' timed out after {}s",
                                tool_name, watchdog.config().timeout_for_tool(tool_name).as_secs()
                            )),
                        };
                        let duration_ms = start.elapsed().as_millis() as u64;
                        if tool_result.success {
                            orchestrator.record_tool_call(tool_name);
                        }
                        watchdog.reward_emitter().tool_completed(tool_name, tool_result.success, duration_ms);
                        tool_result
                    }
                } else {
                    let start = std::time::Instant::now();
                    let tool_result = match watchdog.guard_tool_call(
                        tool_name,
                        self.tool_registry.execute(tool_name, tool_arguments.clone(), tool_context, Some(exec_config)),
                    ).await {
                        Some(result) => result,
                        None => crate::tools::ToolResult::error(format!(
                            "Tool '{}' timed out after {}s",
                            tool_name, watchdog.config().timeout_for_tool(tool_name).as_secs()
                        )),
                    };
                    let duration_ms = start.elapsed().as_millis() as u64;
                    if tool_result.success {
                        orchestrator.record_tool_call(tool_name);
                    }
                    watchdog.reward_emitter().tool_completed(tool_name, tool_result.success, duration_ms);
                    tool_result
                }
            }
        };

        // Handle subtype change: update orchestrator and refresh tools
        if tool_name == "set_agent_subtype" && result.success {
            if let Some(subtype_str) = tool_arguments.get("subtype").and_then(|v| v.as_str()) {
                if let Some(new_key) = agent_types::resolve_subtype_key(subtype_str) {
                    orchestrator.set_subtype(Some(new_key.clone()));
                    log::info!(
                        "[SUBTYPE] Changed to {} mode",
                        agent_types::subtype_label(&new_key)
                    );

                    // Check if new subtype should skip or enter TaskPlanner
                    let should_skip = agent_types::get_subtype_config(&new_key)
                        .map(|c| c.skip_task_planner)
                        .unwrap_or(false);
                    if should_skip {
                        // Skip planning for this subtype
                        if !orchestrator.context().planner_completed {
                            log::info!("[SUBTYPE] '{}' has skip_task_planner=true, staying in Assistant mode", new_key);
                            orchestrator.transition_to_assistant();
                        }
                    } else if !orchestrator.context().planner_completed {
                        // Re-enter TaskPlanner so this subtype plans its work
                        // (only if no prior planning phase has completed — otherwise
                        // the existing task queue should be preserved)
                        log::info!("[SUBTYPE] '{}' entering TaskPlanner mode for task planning", new_key);
                        let ctx = orchestrator.context_mut();
                        ctx.planner_completed = false;
                        ctx.mode = AgentMode::TaskPlanner;
                    } else {
                        log::info!("[SUBTYPE] '{}' keeping Assistant mode — tasks already planned", new_key);
                    }

                    // Refresh tools for new subtype
                    *tools = self.build_tool_list(tool_config, &new_key, orchestrator);

                    // Broadcast toolset update
                    self.broadcast_toolset_update(
                        original_message.channel_id,
                        &orchestrator.current_mode().to_string(),
                        &new_key,
                        tools,
                    );
                }
            }
        }

        // Handle skill activation: update orchestrator and refresh tools
        // (mirrors the set_agent_subtype post-execution pattern above)
        if tool_name == "use_skill" && result.success {
            if let Some(skill_name_val) = tool_arguments.get("skill_name").or_else(|| tool_arguments.get("name")).and_then(|v| v.as_str()) {
                if let Ok(Some(skill)) = self.db.get_enabled_skill_by_name(skill_name_val) {
                    let skills_dir = crate::config::skills_dir();
                    let skill_base_dir = format!("{}/{}", skills_dir, skill.name);
                    let instructions = skill.body.replace("{baseDir}", &skill_base_dir);

                    let requires_tools = skill.requires_tools.clone();
                    log::info!(
                        "[SKILL] Activating skill '{}' with requires_tools: {:?}",
                        skill.name,
                        requires_tools
                    );

                    // Auto-set subtype if skill specifies one (before tool refresh)
                    self.apply_skill_subtype(&skill, orchestrator, original_message.channel_id);

                    orchestrator.context_mut().active_skill = Some(crate::ai::multi_agent::types::ActiveSkill {
                        name: skill.name,
                        instructions,
                        activated_at: chrono::Utc::now().to_rfc3339(),
                        tool_calls_made: 0,
                        requires_tools: requires_tools.clone(),
                    });

                    // Refresh tools to include skill-required tools
                    let sk = orchestrator.current_subtype_key().to_string();
                    *tools = self.build_tool_list(tool_config, &sk, orchestrator);
                    log::info!(
                        "[SKILL] Refreshed toolset with {} tools (skill requires {:?})",
                        tools.len(),
                        requires_tools
                    );
                }
            }
        }

        // Handle retry backoff
        let result = if let Some(retry_secs) = result.retry_after_secs {
            self.broadcaster.broadcast(GatewayEvent::tool_waiting(
                original_message.channel_id,
                tool_name,
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

        // Check metadata for various control signals
        if let Some(metadata) = &result.metadata {
            if metadata.get("requires_user_response").and_then(|v| v.as_bool()).unwrap_or(false) {
                processed.waiting_for_user_response = true;
                processed.user_question_content = Some(result.content.clone());
                log::info!("[ORCHESTRATED_LOOP] Tool requires user response, will break after processing");
            }
            // Check if add_task was called
            if metadata.get("add_task").and_then(|v| v.as_bool()).unwrap_or(false) {
                if let Some(desc) = metadata.get("task_description").and_then(|v| v.as_str()) {
                    let position = metadata.get("task_position")
                        .and_then(|v| v.as_str())
                        .unwrap_or("front");
                    let new_ids = match position {
                        "back" => orchestrator.append_task(desc.to_string()),
                        _ => orchestrator.insert_task_front(desc.to_string()),
                    };
                    log::info!(
                        "[ORCHESTRATED_LOOP] add_task: inserted task(s) {:?} at {} — '{}'",
                        new_ids, position, desc
                    );
                    // If task_fully_completed was already processed this turn
                    // (AI called it before add_task), the session was marked complete
                    // with no pending tasks. Now that we've added a task, undo that.
                    if processed.orchestrator_complete && !orchestrator.all_tasks_complete() {
                        processed.orchestrator_complete = false;
                        processed.final_summary = None;
                        log::info!(
                            "[ORCHESTRATED_LOOP] add_task: resetting orchestrator_complete — new pending tasks exist"
                        );
                        self.advance_to_next_task_or_complete(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        );
                    }
                    self.broadcast_task_queue_update(
                        original_message.channel_id,
                        session_id,
                        orchestrator,
                    );
                }
            }
            // Check if define_tasks was called
            if metadata.get("define_tasks").and_then(|v| v.as_bool()).unwrap_or(false) {
                if let Some(tasks) = metadata.get("tasks").and_then(|v| v.as_array()) {
                    let task_descriptions: Vec<String> = tasks
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !task_descriptions.is_empty() {
                        log::info!(
                            "[ORCHESTRATED_LOOP] define_tasks: replacing queue with {} tasks",
                            task_descriptions.len()
                        );
                        let available_tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
                        let ctx = orchestrator.context_mut();
                        ctx.task_queue =
                            crate::ai::multi_agent::types::TaskQueue::from_descriptions_with_tool_matching(task_descriptions, &available_tool_names);
                        ctx.planner_completed = true;
                        ctx.mode = AgentMode::Assistant;
                        self.advance_to_next_task_or_complete(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        );
                        self.broadcast_task_queue_update(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        );
                        // Prevent any task_fully_completed in this same batch from
                        // accidentally completing the newly-started first task
                        batch_state.define_tasks_replaced_queue = true;
                    }
                }
            }
            // Check if task_fully_completed was called
            // Skip if define_tasks just replaced the queue or auto-complete already advanced
            if (batch_state.define_tasks_replaced_queue || batch_state.auto_completed_task)
                && metadata.get("task_fully_completed").and_then(|v| v.as_bool()).unwrap_or(false)
            {
                log::info!(
                    "[ORCHESTRATED_LOOP] Ignoring task_fully_completed — \
                     task already advanced (define_tasks={}, auto_complete={})",
                    batch_state.define_tasks_replaced_queue, batch_state.auto_completed_task
                );
            } else if metadata.get("task_fully_completed").and_then(|v| v.as_bool()).unwrap_or(false) {
                let summary = metadata.get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&result.content)
                    .to_string();

                log::info!("[ORCHESTRATED_LOOP] task_fully_completed called");

                // Mark current task as completed and broadcast
                if let Some(completed_task_id) = orchestrator.complete_current_task() {
                    log::info!("[ORCHESTRATED_LOOP] Task {} completed", completed_task_id);
                    self.broadcast_task_status_change(
                        original_message.channel_id,
                        session_id,
                        completed_task_id,
                        "completed",
                        &summary,
                    );
                }

                match self.advance_to_next_task_or_complete(
                    original_message.channel_id,
                    session_id,
                    orchestrator,
                ) {
                    TaskAdvanceResult::AllTasksComplete => {
                        processed.orchestrator_complete = true;
                        processed.final_summary = Some(summary.clone());
                        // Don't set last_say_to_user_content here — task_fully_completed
                        // was NOT broadcast via tool.result as say_to_user. The summary
                        // will flow through final_summary → finalize_tool_loop → dispatch()
                        // → agent.response event. Setting last_say_to_user_content would
                        // incorrectly mark the response as "already delivered" and suppress
                        // the agent.response broadcast.
                    }
                    TaskAdvanceResult::InconsistentState => {
                        log::warn!("[ORCHESTRATED_LOOP] task_fully_completed: inconsistent task state, terminating");
                        processed.orchestrator_complete = true;
                        processed.final_summary = Some(summary.clone());
                    }
                    TaskAdvanceResult::NextTaskStarted => {
                        // Continue loop for next task
                    }
                }
            }
        }

        // Capture say_to_user content for session memory
        // Skip duplicate say_to_user calls within the same batch — AI sometimes returns
        // multiple say_to_user calls in a single response, causing duplicate messages.
        let is_duplicate_say_to_user = tool_name == "say_to_user" && result.success && batch_state.had_say_to_user;
        if tool_name == "say_to_user" && result.success {
            if is_duplicate_say_to_user {
                log::warn!("[ORCHESTRATED_LOOP] Skipping duplicate say_to_user in same batch (already broadcast)");
            } else {
                *last_say_to_user_content = result.content.clone();
                batch_state.had_say_to_user = true;
                // Content will be returned as the final result by finalize_tool_loop
                // and stored as assistant message by dispatch() — no need to store here.
            }
        }

        // say_to_user with finished_task=true completes the current task.
        // In safe mode, say_to_user always terminates (no ongoing tasks).
        // When define_tasks replaced the queue or auto_completed_task in this batch,
        // skip task advancement for non-safe-mode, but still terminate in safe mode.
        if tool_name == "say_to_user" && result.success {
            let finished_task = result.metadata.as_ref()
                .and_then(|m| m.get("finished_task"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_safe_mode && !batch_state.define_tasks_replaced_queue && orchestrator.task_queue_is_empty() {
                // Safe mode with no task queue: terminate immediately
                log::info!("[ORCHESTRATED_LOOP] say_to_user terminating loop (safe_mode=true, no task queue)");
                processed.orchestrator_complete = true;
            } else if is_safe_mode && !batch_state.define_tasks_replaced_queue && finished_task && !orchestrator.task_queue_is_empty() {
                // Safe mode with task queue and finished_task: advance like normal mode
                if let Some(completed_task_id) = orchestrator.complete_current_task() {
                    log::info!("[ORCHESTRATED_LOOP] say_to_user (safe_mode) completed task {}", completed_task_id);
                    self.broadcast_task_status_change(
                        original_message.channel_id,
                        session_id,
                        completed_task_id,
                        "completed",
                        &format!("Completed via say_to_user"),
                    );
                }
                match self.advance_to_next_task_or_complete(
                    original_message.channel_id,
                    session_id,
                    orchestrator,
                ) {
                    TaskAdvanceResult::AllTasksComplete => {
                        log::info!("[ORCHESTRATED_LOOP] say_to_user (safe_mode): all tasks done, terminating loop");
                        processed.orchestrator_complete = true;
                    }
                    TaskAdvanceResult::NextTaskStarted => {
                        log::info!("[ORCHESTRATED_LOOP] say_to_user (safe_mode): advanced to next task, continuing loop");
                    }
                    TaskAdvanceResult::InconsistentState => {
                        log::warn!("[ORCHESTRATED_LOOP] say_to_user (safe_mode): inconsistent task state, terminating");
                        processed.orchestrator_complete = true;
                    }
                }
            } else if is_safe_mode && batch_state.define_tasks_replaced_queue {
                // Safe mode but define_tasks just created tasks in this batch — don't terminate
                log::info!(
                    "[ORCHESTRATED_LOOP] say_to_user (safe_mode): ignoring termination — define_tasks just replaced queue"
                );
            } else if finished_task && !batch_state.define_tasks_replaced_queue && !batch_state.auto_completed_task {
                if !orchestrator.task_queue_is_empty() {
                    // Complete current task and try to advance
                    if let Some(completed_task_id) = orchestrator.complete_current_task() {
                        log::info!("[ORCHESTRATED_LOOP] say_to_user completed task {}", completed_task_id);
                        self.broadcast_task_status_change(
                            original_message.channel_id,
                            session_id,
                            completed_task_id,
                            "completed",
                            &format!("Completed via say_to_user"),
                        );
                    }
                    match self.advance_to_next_task_or_complete(
                        original_message.channel_id,
                        session_id,
                        orchestrator,
                    ) {
                        TaskAdvanceResult::AllTasksComplete => {
                            log::info!("[ORCHESTRATED_LOOP] say_to_user: all tasks done, terminating loop");
                            processed.orchestrator_complete = true;
                        }
                        TaskAdvanceResult::NextTaskStarted => {
                            log::info!("[ORCHESTRATED_LOOP] say_to_user: advanced to next task, continuing loop");
                        }
                        TaskAdvanceResult::InconsistentState => {
                            log::warn!("[ORCHESTRATED_LOOP] say_to_user: inconsistent task state, terminating");
                            processed.orchestrator_complete = true;
                        }
                    }
                } else {
                    // No task queue — terminate immediately
                    log::info!("[ORCHESTRATED_LOOP] say_to_user terminating loop (finished_task={}, no task queue)", finished_task);
                    processed.orchestrator_complete = true;
                }
            } else if batch_state.define_tasks_replaced_queue || batch_state.auto_completed_task {
                log::info!(
                    "[ORCHESTRATED_LOOP] Ignoring say_to_user finished_task — \
                     task already advanced (define_tasks={}, auto_complete={})",
                    batch_state.define_tasks_replaced_queue, batch_state.auto_completed_task
                );
            }
        }

        // AUTO-COMPLETE: Check if this successful tool matches current task's trigger
        if result.success && !batch_state.define_tasks_replaced_queue && !processed.orchestrator_complete {
            if let Some(current_task) = orchestrator.task_queue().current_task() {
                if let Some(ref trigger_tool) = current_task.auto_complete_tool {
                    if trigger_tool == tool_name {
                        let task_desc = current_task.description.clone();
                        log::info!(
                            "[AUTO_COMPLETE] Tool '{}' succeeded — auto-completing task: {}",
                            tool_name, task_desc
                        );
                        if let Some(completed_task_id) = orchestrator.complete_current_task() {
                            self.broadcast_task_status_change(
                                original_message.channel_id,
                                session_id,
                                completed_task_id,
                                "completed",
                                &format!("Auto-completed via {}", tool_name),
                            );
                        }
                        match self.advance_to_next_task_or_complete(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        ) {
                            TaskAdvanceResult::AllTasksComplete => {
                                // DON'T terminate the loop here. The raw tool result (e.g. JSON)
                                // isn't a user-friendly response. Let the AI continue for one more
                                // iteration so it can call say_to_user with a properly formatted
                                // message (e.g. presenting an image URL, summarizing results).
                                // The loop will terminate naturally when the AI calls say_to_user
                                // (with finished_task=true and no pending tasks) or returns
                                // content-only (no tool calls with all tasks complete).
                                log::info!("[AUTO_COMPLETE] All tasks done — letting AI present result via say_to_user");
                            }
                            TaskAdvanceResult::NextTaskStarted => {
                                log::info!("[AUTO_COMPLETE] Advanced to next task, continuing loop");
                            }
                            TaskAdvanceResult::InconsistentState => {
                                log::warn!("[AUTO_COMPLETE] Inconsistent task state, terminating");
                                processed.orchestrator_complete = true;
                            }
                        }
                        self.broadcast_task_queue_update(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        );
                        batch_state.auto_completed_task = true;
                    }
                }
            }
        }

        // Extract duration_ms from metadata if available
        let duration_ms = result.metadata.as_ref()
            .and_then(|m| m.get("duration_ms"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // Broadcast tool result event. say_to_user events are still broadcast for
        // channels that capture them (e.g. Twitter). Minimal-style channels (Discord,
        // Telegram, AgentChat) skip say_to_user in their event handlers and instead
        // receive the content via the final result.response.
        if !is_duplicate_say_to_user {
            self.broadcaster.broadcast(GatewayEvent::tool_result(
                original_message.channel_id,
                Some(&original_message.chat_id),
                tool_name,
                result.success,
                duration_ms,
                &result.content,
                is_safe_mode,
            ));
        }

        // Execute AfterToolCall hooks
        if let Some(hook_manager) = &self.hook_manager {
            use crate::hooks::{HookContext, HookEvent, HookResult};
            let mut hook_context = HookContext::new(HookEvent::AfterToolCall)
                .with_channel(original_message.channel_id, Some(session_id))
                .with_tool(tool_name.to_string(), tool_arguments.clone())
                .with_tool_result(serde_json::json!({
                    "success": result.success,
                    "content": result.content,
                }));
            let hook_result = hook_manager.execute(HookEvent::AfterToolCall, &mut hook_context).await;
            if let HookResult::Error(e) = hook_result {
                log::warn!("Hook execution failed for tool '{}': {}", tool_name, e);
            }
        }

        // Save tool result to session via async writer (non-blocking)
        if !is_duplicate_say_to_user {
            let tool_result_content = format!(
                "**{}:** {}\n{}",
                if result.success { "Result" } else { "Error" },
                tool_name,
                result.content
            );
            self.session_writer.send(
                session_id,
                DbMessageRole::ToolResult,
                tool_result_content,
                Some(tool_name),
            );
        }

        // Broadcast task list update after any orchestrator tool processing
        self.broadcast_tasks_update(original_message.channel_id, session_id, orchestrator);

        processed.result_content = result.content;
        processed.success = result.success;
        processed
    }
}
