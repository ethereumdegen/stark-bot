use crate::ai::{
    multi_agent::{types::{self as agent_types, AgentMode}, Orchestrator},
    AiClient, AiResponse, Message, MessageRole, ModelArchetype, ToolHistoryEntry, ToolResponse,
};
use crate::channels::types::NormalizedMessage;
use crate::gateway::protocol::GatewayEvent;
use crate::models::session_message::MessageRole as DbMessageRole;
use crate::models::TaskType;
use crate::telemetry::Watchdog;
use crate::tools::{ToolConfig, ToolContext, ToolDefinition};
use std::sync::Arc;

use super::finalization::TaskAdvanceResult;
use super::tool_processing::BatchState;
use super::{MessageDispatcher, FALLBACK_MAX_TOOL_ITERATIONS};

impl MessageDispatcher {
    /// Generate response using native API tool calling with multi-agent orchestration
    pub(super) async fn generate_with_native_tools_orchestrated(
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
        is_safe_mode: bool,
        watchdog: &Arc<Watchdog>,
    ) -> Result<(String, bool), String> {
        // Get max tool iterations from bot settings
        let max_tool_iterations = self.db.get_bot_settings()
            .map(|s| s.max_tool_iterations as usize)
            .unwrap_or(FALLBACK_MAX_TOOL_ITERATIONS);

        // Build conversation with orchestrator's system prompt prepended
        let mut conversation = messages.clone();
        if let Some(system_msg) = conversation.first_mut() {
            if system_msg.role == MessageRole::System {
                // Prepend orchestrator context to the existing system prompt
                let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
                system_msg.content = format!(
                    "{}\n\n---\n\n{}",
                    orchestrator_prompt,
                    archetype.enhance_system_prompt(&system_msg.content, &tools)
                );
            }
        }

        // Some APIs (MiniMax, Kimi) reject conversations with multiple system messages.
        // Merge all system messages into the first one.
        if archetype.requires_single_system_message() {
            let mut merged_content = String::new();
            let mut non_system: Vec<Message> = Vec::new();
            for msg in conversation.drain(..) {
                if msg.role == MessageRole::System {
                    if !merged_content.is_empty() {
                        merged_content.push_str("\n\n---\n\n");
                    }
                    merged_content.push_str(&msg.content);
                } else {
                    non_system.push(msg);
                }
            }
            if !merged_content.is_empty() {
                conversation.push(Message {
                    role: MessageRole::System,
                    content: merged_content,
                });
            }
            conversation.extend(non_system);
        }

        // Clear waiting_for_user_context now that it's been consumed into the prompt
        orchestrator.clear_waiting_for_user_context();

        let mut tool_history: Vec<ToolHistoryEntry> = Vec::new();
        let mut iterations = 0;
        let mut tool_call_log: Vec<String> = Vec::new();
        let mut orchestrator_complete = false;
        let mut memory_suppressed = false;
        let mut final_summary = String::new();
        let mut waiting_for_user_response = false;
        let mut user_question_content = String::new();
        let mut was_cancelled = false;
        let mut last_say_to_user_content = String::new();

        // Loop detection: track recent tool call signatures to detect repetitive behavior
        let mut recent_call_signatures: Vec<String> = Vec::new();
        const MAX_REPEATED_CALLS: usize = 3; // Break loop after 3 identical consecutive calls
        const SIGNATURE_HISTORY_SIZE: usize = 20; // Track last 20 call signatures

        // say_to_user loop prevention: don't allow say_to_user to be called twice in a row
        let mut previous_iteration_had_say_to_user = false;

        // Counter for consecutive iterations where AI returns no tool calls
        // but tasks are pending — caps forced retries to prevent infinite loops
        // (e.g., when a subagent was cancelled and the task can never complete)
        let mut no_tool_pending_retries: u32 = 0;
        const MAX_NO_TOOL_PENDING_RETRIES: u32 = 3;

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

                // Load available skills for the planner prompt
                let skills_text = match self.db.list_enabled_skills() {
                    Ok(skills) if !skills.is_empty() => {
                        skills.iter()
                            .map(|s| format!("- **{}**: {}", s.name, s.description))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                    _ => "No skills currently available.".to_string(),
                };

                // Update conversation with planner prompt including skills
                if let Some(system_msg) = conversation.first_mut() {
                    if system_msg.role == MessageRole::System {
                        let planner_prompt = orchestrator.get_planner_prompt_with_skills(&skills_text);
                        system_msg.content = planner_prompt;
                    }
                }
                // define_tasks is ALWAYS available in TaskPlanner mode, regardless of
                // tool config (safe mode, standard, etc.). Pull directly from registry
                // to bypass tool config filtering.
                match self.tool_registry.get("define_tasks") {
                    Some(tool) => vec![tool.definition()],
                    None => {
                        log::error!("[ORCHESTRATED_LOOP] define_tasks tool not found in registry!");
                        vec![]
                    }
                }
            } else {
                // In assistant mode, tools already have define_tasks stripped
                // by build_tool_list() — just clone.
                tools.clone()
            };

            // Debug: log tools sent to AI on every iteration
            log::info!(
                "[ORCHESTRATED_LOOP] Iter {} → sending {} tools to AI: {:?}",
                iterations,
                current_tools.len(),
                current_tools.iter().map(|t| &t.name).collect::<Vec<_>>()
            );

            // Emit an iteration task for visibility (after first iteration)
            if iterations > 1 {
                if let Some(ref exec_id) = self.execution_tracker.get_execution_id(original_message.channel_id) {
                    let iter_task = self.execution_tracker.start_task(
                        original_message.channel_id,
                        exec_id,
                        Some(exec_id),
                        TaskType::Thinking,
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
                    self.broadcast_task_queue_update(original_message.channel_id, session_id, orchestrator);

                    // If we deleted the current task, move to the next one
                    if was_current {
                        log::info!("[ORCHESTRATED_LOOP] Deleted task was the current task, moving to next");
                        if let TaskAdvanceResult::AllTasksComplete = self.advance_to_next_task_or_complete(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        ) {
                            orchestrator_complete = true;
                            break;
                        }
                    }
                } else {
                    log::warn!("[ORCHESTRATED_LOOP] Task {} not found for deletion", task_id);
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
                        session_id,
                        first_task.id,
                        "in_progress",
                        &first_task.description,
                    );
                    // Broadcast full task queue update
                    self.broadcast_task_queue_update(original_message.channel_id, session_id, orchestrator);

                    // Broadcast mode change to assistant
                    self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
                        original_message.channel_id,
                        Some(&original_message.chat_id),
                        "assistant",
                        "Assistant",
                        Some("Executing tasks"),
                    ));

                    // Update tools for assistant mode
                    let sk = orchestrator.current_subtype_key().to_string();
                    tools = self.build_tool_list(tool_config, &sk, &orchestrator);

                    // Broadcast toolset update
                    self.broadcast_toolset_update(
                        original_message.channel_id,
                        "assistant",
                        &sk,
                        &tools,
                    );

                    // Update system prompt for new mode with current task
                    if let Some(system_msg) = conversation.first_mut() {
                        if system_msg.role == MessageRole::System {
                            let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
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
                        TaskType::PlanMode,
                        format!("Switching to {} mode", transition.to.label()),
                        Some(&format!("Transitioning: {}", transition.reason)),
                    );
                    self.execution_tracker.complete_task(&transition_task);
                }

                self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
                    original_message.channel_id,
                    Some(&original_message.chat_id),
                    &transition.to.to_string(),
                    transition.to.label(),
                    Some(&transition.reason),
                ));

                // Update tools for new mode
                let sk = orchestrator.current_subtype_key().to_string();
                tools = self.build_tool_list(tool_config, &sk, &orchestrator);

                // Emit task for toolset update
                if let Some(ref exec_id) = self.execution_tracker.get_execution_id(original_message.channel_id) {
                    let toolset_task = self.execution_tracker.start_task(
                        original_message.channel_id,
                        exec_id,
                        Some(exec_id),
                        TaskType::Loading,
                        format!("Loading {} tools for {} mode", tools.len(), agent_types::subtype_label(&sk)),
                        Some("Configuring available tools..."),
                    );
                    self.execution_tracker.complete_task(&toolset_task);
                }

                // Broadcast toolset update
                self.broadcast_toolset_update(
                    original_message.channel_id,
                    &transition.to.to_string(),
                    &sk,
                    &tools,
                );

                // Update system prompt for new mode
                if let Some(system_msg) = conversation.first_mut() {
                    if system_msg.role == MessageRole::System {
                        let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
                        system_msg.content = format!(
                            "{}\n\n---\n\n{}",
                            orchestrator_prompt,
                            archetype.enhance_system_prompt(&messages[0].content, &tools)
                        );
                    }
                }
            }

            // Update system prompt every iteration so the AI sees the current task,
            // mode changes, and any context updates from the orchestrator.
            if let Some(system_msg) = conversation.first_mut() {
                if system_msg.role == MessageRole::System {
                    let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
                    system_msg.content = format!(
                        "{}\n\n---\n\n{}",
                        orchestrator_prompt,
                        archetype.enhance_system_prompt(&messages[0].content, &current_tools)
                    );
                }
            }

            // Log available tools for this iteration
            log::debug!(
                "[ORCHESTRATED_LOOP] Iteration {} tools ({}): [{}]",
                iterations,
                current_tools.len(),
                current_tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")
            );

            // Generate with native tool support and progress notifications
            let mut ai_response = match self.generate_with_progress(
                &client,
                conversation.clone(),
                tool_history.clone(),
                current_tools.clone(),
                original_message.channel_id,
                session_id,
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

            // Strip model-specific artifacts (e.g. MiniMax <think> blocks)
            ai_response.content = archetype.clean_content(&ai_response.content);

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

                // If there are pending tasks, don't exit — force the AI to keep working.
                // The AI might respond with just text after a batched define_tasks + say_to_user,
                // but we need it to continue executing tasks.
                // Cap retries to prevent infinite loops (e.g., when a subagent was cancelled
                // and the AI can't complete the task).
                if !orchestrator.task_queue_is_empty() && !orchestrator.all_tasks_complete() {
                    no_tool_pending_retries += 1;

                    if no_tool_pending_retries > MAX_NO_TOOL_PENDING_RETRIES {
                        log::warn!(
                            "[ORCHESTRATED_LOOP] AI returned no tool calls {} times with pending tasks — \
                             auto-skipping current task to prevent infinite loop",
                            no_tool_pending_retries
                        );
                        // Auto-complete the current task so the loop can move on
                        if let Some(task_id) = orchestrator.complete_current_task() {
                            log::info!(
                                "[ORCHESTRATED_LOOP] Auto-completed stuck task {} after {} no-tool retries",
                                task_id, no_tool_pending_retries
                            );
                            self.broadcast_task_status_change(
                                original_message.channel_id,
                                session_id,
                                task_id,
                                "completed",
                                "Auto-completed (agent could not proceed)",
                            );
                            self.broadcast_task_queue_update(
                                original_message.channel_id,
                                session_id,
                                orchestrator,
                            );
                        }
                        // Try to advance to next task or finish
                        match self.advance_to_next_task_or_complete(
                            original_message.channel_id,
                            session_id,
                            orchestrator,
                        ) {
                            TaskAdvanceResult::AllTasksComplete => {
                                orchestrator_complete = true;
                                break;
                            }
                            _ => {
                                // Reset counter for the next task
                                no_tool_pending_retries = 0;
                            }
                        }
                        continue;
                    }

                    log::info!(
                        "[ORCHESTRATED_LOOP] AI returned no tool calls but tasks are pending — forcing retry ({}/{})",
                        no_tool_pending_retries, MAX_NO_TOOL_PENDING_RETRIES
                    );
                    conversation.push(Message {
                        role: MessageRole::Assistant,
                        content: ai_response.content.clone(),
                    });
                    conversation.push(Message {
                        role: MessageRole::User,
                        content: "[SYSTEM] You have pending tasks to complete. Please call the appropriate tools to continue working on the current task. If a sub-agent failed or was cancelled, call `say_to_user` with `finished_task: true` to acknowledge and move on.".to_string(),
                    });
                    continue;
                }

                // say_to_user content takes priority — it IS the final result (already broadcast)
                if !last_say_to_user_content.is_empty() {
                    log::info!("[ORCHESTRATED_LOOP] Returning say_to_user content as final result ({} chars)", last_say_to_user_content.len());
                    return Ok((last_say_to_user_content.clone(), true));
                }

                if orchestrator_complete {
                    // Build response from non-empty parts (exclude tool_call_log —
                    // tool calls are already shown in real-time via events)
                    let mut parts: Vec<&str> = Vec::new();
                    if !final_summary.is_empty() { parts.push(&final_summary); }
                    if !ai_response.content.trim().is_empty() { parts.push(&ai_response.content); }
                    let response = parts.join("\n\n");
                    return Ok((response, false));
                } else {
                    return Ok((ai_response.content, false));
                }
            }

            // Process tool calls — reset the no-tool retry counter since the AI is working
            no_tool_pending_retries = 0;
            let mut tool_responses = Vec::new();

            // Loop detection: check for repetitive tool calls
            let current_signatures: Vec<String> = ai_response.tool_calls.iter()
                .map(|c| format!("{}:{}", c.name, c.arguments.to_string()))
                .collect();

            // Check if all current calls were recently made (loop detection)
            let repeated_count = current_signatures.iter()
                .filter(|sig| recent_call_signatures.iter().filter(|s| s == sig).count() >= MAX_REPEATED_CALLS - 1)
                .count();

            if repeated_count > 0 && repeated_count == current_signatures.len() {
                log::warn!(
                    "[LOOP_DETECTION] Detected {} repeated tool calls, breaking loop to prevent infinite cycling",
                    repeated_count
                );

                // Emit loop detection reward signal via RewardEmitter
                watchdog.reward_emitter().loop_detected(&current_signatures, iterations as u32);

                // Create a feedback entry to guide the AI
                let loop_warning = format!(
                    "⚠️ LOOP DETECTED: You've called the same tool(s) {} times with identical arguments. \
                    The repeated calls are: {}. \
                    Please try a DIFFERENT approach or tool, or explain what you're trying to accomplish.",
                    MAX_REPEATED_CALLS,
                    current_signatures.join(", ")
                );

                // Add as a tool response to guide the AI
                for call in &ai_response.tool_calls {
                    tool_responses.push(ToolResponse::error(
                        call.id.clone(),
                        loop_warning.clone(),
                    ));
                }

                // Add to tool history and continue to next iteration (AI will see the warning)
                tool_history.push(ToolHistoryEntry::new(
                    ai_response.tool_calls.clone(),
                    tool_responses,
                ));

                // Give the AI one more chance to correct, then break
                if iterations > max_tool_iterations / 2 {
                    log::error!(
                        "[LOOP_DETECTION] Loop persists after warning, breaking out. Last attempt: {}",
                        current_signatures.join(", ")
                    );
                    return Err("Sorry, I wasn't able to complete this request. Please try again.".to_string());
                }
                continue;
            }

            // Track signatures for future loop detection
            for sig in &current_signatures {
                recent_call_signatures.push(sig.clone());
            }
            // Keep only recent signatures
            if recent_call_signatures.len() > SIGNATURE_HISTORY_SIZE {
                recent_call_signatures.drain(0..recent_call_signatures.len() - SIGNATURE_HISTORY_SIZE);
            }

            // say_to_user consecutive call detection: if say_to_user is the ONLY tool called
            // in two consecutive iterations (no real work being done), terminate the loop.
            // Skip this check when there are pending tasks — the AI may need to send progress
            // messages between tasks.
            let current_iteration_has_say_to_user = ai_response.tool_calls.iter().any(|c| c.name == "say_to_user");
            let only_say_to_user = current_iteration_has_say_to_user && ai_response.tool_calls.len() == 1;
            let has_pending_tasks = !orchestrator.task_queue_is_empty() && !orchestrator.all_tasks_complete();
            if only_say_to_user && previous_iteration_had_say_to_user && !has_pending_tasks {
                log::warn!("[SAY_TO_USER_LOOP] Detected consecutive say_to_user-only calls with no pending tasks, terminating loop");
                // Don't set final_summary - the message was already broadcast via tool_result
                orchestrator_complete = true;
                break;
            }

            let mut batch_state = BatchState::new();

            for call in &ai_response.tool_calls {
                // Refresh snapshot before each call so that set_agent_subtype
                // or use_skill side-effects (which rebuild `tools`) are visible
                // to subsequent calls in the same batch.
                let current_tools_snapshot = tools.clone();

                let processed = self.process_tool_call_result(
                    &call.name,
                    &call.arguments,
                    tool_config,
                    tool_context,
                    original_message,
                    session_id,
                    is_safe_mode,
                    &mut tools,
                    &mut batch_state,
                    &mut last_say_to_user_content,
                    &mut memory_suppressed,
                    &mut tool_call_log,
                    orchestrator,
                    &current_tools_snapshot,
                    watchdog,
                ).await;

                // Update loop-level flags from the processed result
                if processed.orchestrator_complete {
                    orchestrator_complete = true;
                    if let Some(ref summary) = processed.final_summary {
                        final_summary = summary.clone();
                    }
                }
                if processed.waiting_for_user_response {
                    waiting_for_user_response = true;
                    if let Some(ref content) = processed.user_question_content {
                        user_question_content = content.clone();
                    }
                }

                tool_responses.push(if processed.success {
                    ToolResponse::success(call.id.clone(), processed.result_content)
                } else {
                    ToolResponse::error(call.id.clone(), processed.result_content)
                });
            }

            // If define_tasks just replaced the queue in this batch, any orchestrator_complete
            // set by an earlier tool in the same batch (e.g., say_to_user that ran before
            // define_tasks) is stale — reset it since there's new work to do.
            if batch_state.define_tasks_replaced_queue && orchestrator_complete && !orchestrator.all_tasks_complete() {
                log::info!(
                    "[ORCHESTRATED_LOOP] Resetting orchestrator_complete — define_tasks created new tasks in this batch"
                );
                orchestrator_complete = false;
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

            // Update say_to_user tracking for next iteration (only counts if say_to_user was the sole tool)
            previous_iteration_had_say_to_user = only_say_to_user;
        }

        self.finalize_tool_loop(
            original_message,
            session_id,
            is_safe_mode,
            orchestrator,
            orchestrator_complete,
            was_cancelled,
            waiting_for_user_response,
            memory_suppressed,
            &last_say_to_user_content,
            &tool_call_log,
            &final_summary,
            &user_question_content,
            max_tool_iterations,
            iterations,
            watchdog,
        )
    }

    /// Generate response using text-based tool calling with multi-agent orchestration
    pub(super) async fn generate_with_text_tools_orchestrated(
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
        is_safe_mode: bool,
        watchdog: &Arc<Watchdog>,
    ) -> Result<(String, bool), String> {
        // Get max tool iterations from bot settings
        let max_tool_iterations = self.db.get_bot_settings()
            .map(|s| s.max_tool_iterations as usize)
            .unwrap_or(FALLBACK_MAX_TOOL_ITERATIONS);

        // Note: define_tasks stripping is handled by build_tool_list() at the call site

        // Build conversation with orchestrator's system prompt
        let mut conversation = messages.clone();
        if let Some(system_msg) = conversation.first_mut() {
            if system_msg.role == MessageRole::System {
                let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
                system_msg.content = format!(
                    "{}\n\n---\n\n{}",
                    orchestrator_prompt,
                    archetype.enhance_system_prompt(&system_msg.content, &tools)
                );
            }
        }

        // Some APIs (MiniMax, Kimi) reject conversations with multiple system messages.
        // Merge all system messages into the first one.
        if archetype.requires_single_system_message() {
            let mut merged_content = String::new();
            let mut non_system: Vec<Message> = Vec::new();
            for msg in conversation.drain(..) {
                if msg.role == MessageRole::System {
                    if !merged_content.is_empty() {
                        merged_content.push_str("\n\n---\n\n");
                    }
                    merged_content.push_str(&msg.content);
                } else {
                    non_system.push(msg);
                }
            }
            if !merged_content.is_empty() {
                conversation.push(Message {
                    role: MessageRole::System,
                    content: merged_content,
                });
            }
            conversation.extend(non_system);
        }

        // Clear waiting_for_user_context now that it's been consumed into the prompt
        orchestrator.clear_waiting_for_user_context();

        let mut final_response = String::new();
        let mut iterations = 0;
        let mut tool_call_log: Vec<String> = Vec::new();
        let mut orchestrator_complete = false;
        let mut memory_suppressed = false;
        let mut waiting_for_user_response = false;
        let mut user_question_content = String::new();
        let mut was_cancelled = false;
        let mut last_say_to_user_content = String::new();

        // Loop detection: track recent tool call signatures to detect repetitive behavior
        let mut recent_call_signatures: Vec<String> = Vec::new();
        const MAX_REPEATED_CALLS: usize = 3; // Break loop after 3 identical consecutive calls
        const SIGNATURE_HISTORY_SIZE: usize = 20; // Track last 20 call signatures

        // say_to_user loop prevention: don't allow say_to_user to be called twice in a row
        let mut previous_iteration_had_say_to_user = false;

        // Counter for consecutive no-tool-call retries with pending tasks (text path)
        let mut no_tool_pending_retries: u32 = 0;
        const MAX_NO_TOOL_PENDING_RETRIES: u32 = 3;

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

            if iterations > max_tool_iterations {
                log::warn!("Text orchestrated loop exceeded max iterations ({})", max_tool_iterations);
                break;
            }

            // Check for forced mode transition
            if let Some(transition) = orchestrator.check_forced_transition() {
                self.broadcaster.broadcast(GatewayEvent::agent_mode_change(
                    original_message.channel_id,
                    Some(&original_message.chat_id),
                    &transition.to.to_string(),
                    transition.to.label(),
                    Some(&transition.reason),
                ));

                // Update tools for new mode
                let sk = orchestrator.current_subtype_key().to_string();
                tools = self.build_tool_list(tool_config, &sk, orchestrator);

                // Broadcast toolset update
                self.broadcast_toolset_update(
                    original_message.channel_id,
                    &transition.to.to_string(),
                    &sk,
                    &tools,
                );

                // Update system prompt
                if let Some(system_msg) = conversation.first_mut() {
                    if system_msg.role == MessageRole::System {
                        let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
                        system_msg.content = format!(
                            "{}\n\n---\n\n{}",
                            orchestrator_prompt,
                            archetype.enhance_system_prompt(&messages[0].content, &tools)
                        );
                    }
                }
            }

            // Update system prompt every iteration so the AI sees the current task
            if let Some(system_msg) = conversation.first_mut() {
                if system_msg.role == MessageRole::System {
                    let orchestrator_prompt = orchestrator.get_system_prompt_with_resource_manager_and_channel(&self.resource_manager, Some(&original_message.channel_type));
                    system_msg.content = format!(
                        "{}\n\n---\n\n{}",
                        orchestrator_prompt,
                        archetype.enhance_system_prompt(&messages[0].content, &tools)
                    );
                }
            }

            // Log available tools for this iteration
            log::info!(
                "[TEXT_ORCHESTRATED] Iter {} → sending {} tools to AI: {:?}",
                iterations,
                tools.len(),
                tools.iter().map(|t| &t.name).collect::<Vec<_>>()
            );

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
                        // Loop detection: check for repetitive tool calls
                        let call_signature = format!("{}:{}", tool_call.tool_name, tool_call.tool_params.to_string());
                        let repeated_count = recent_call_signatures.iter()
                            .filter(|s| *s == &call_signature)
                            .count();

                        if repeated_count >= MAX_REPEATED_CALLS - 1 {
                            log::warn!(
                                "[TEXT_LOOP_DETECTION] Detected repeated tool call '{}', breaking loop",
                                tool_call.tool_name
                            );

                            // Emit loop detection reward signal via RewardEmitter
                            watchdog.reward_emitter().loop_detected(
                                &[call_signature.clone()],
                                iterations as u32,
                            );

                            // Feed back to conversation to guide the AI
                            let loop_warning = format!(
                                "⚠️ LOOP DETECTED: You've called `{}` {} times with identical arguments. \
                                Please try a DIFFERENT approach or tool.",
                                tool_call.tool_name,
                                MAX_REPEATED_CALLS
                            );
                            conversation.push(Message {
                                role: MessageRole::User,
                                content: loop_warning,
                            });

                            // Give the AI one more chance to correct, then break
                            if iterations > max_tool_iterations / 2 {
                                log::error!(
                                    "[TEXT_LOOP_DETECTION] Loop persists after warning, breaking out. Tool: {}",
                                    tool_call.tool_name
                                );
                                return Err("Sorry, I wasn't able to complete this request. Please try again.".to_string());
                            }
                            continue;
                        }

                        // Track signature for future loop detection
                        recent_call_signatures.push(call_signature);
                        if recent_call_signatures.len() > SIGNATURE_HISTORY_SIZE {
                            recent_call_signatures.drain(0..recent_call_signatures.len() - SIGNATURE_HISTORY_SIZE);
                        }

                        // say_to_user consecutive call detection: if say_to_user is the ONLY tool called
                        // in two consecutive iterations with no pending tasks, terminate.
                        let current_iteration_has_say_to_user = tool_call.tool_name == "say_to_user";
                        let has_pending_tasks = !orchestrator.task_queue_is_empty() && !orchestrator.all_tasks_complete();
                        if current_iteration_has_say_to_user && previous_iteration_had_say_to_user && !has_pending_tasks {
                            log::warn!("[TEXT_SAY_TO_USER_LOOP] Detected consecutive say_to_user calls with no pending tasks, terminating loop");
                            // Don't set final_response - the message was already broadcast via tool_result
                            orchestrator_complete = true;
                            break;
                        }

                        // Text path: one tool call per batch
                        let mut batch_state = BatchState::new();
                        let current_tools_snapshot = tools.clone();
                        let processed = self.process_tool_call_result(
                            &tool_call.tool_name,
                            &tool_call.tool_params,
                            tool_config,
                            tool_context,
                            original_message,
                            session_id,
                            is_safe_mode,
                            &mut tools,
                            &mut batch_state,
                            &mut last_say_to_user_content,
                            &mut memory_suppressed,
                            &mut tool_call_log,
                            orchestrator,
                            &current_tools_snapshot,
                            watchdog,
                        ).await;

                        // Update loop-level flags
                        if processed.orchestrator_complete {
                            orchestrator_complete = true;
                            if let Some(ref summary) = processed.final_summary {
                                final_response = summary.clone();
                            }
                        }
                        if processed.waiting_for_user_response {
                            waiting_for_user_response = true;
                            if let Some(ref content) = processed.user_question_content {
                                user_question_content = content.clone();
                            }
                        }

                        let tool_result_content = processed.result_content;

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

                        // Update say_to_user tracking for next iteration
                        previous_iteration_had_say_to_user = current_iteration_has_say_to_user;
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

                        // If there are pending tasks, force the AI to keep working (with cap)
                        if !orchestrator.task_queue_is_empty() && !orchestrator.all_tasks_complete() {
                            no_tool_pending_retries += 1;

                            if no_tool_pending_retries > MAX_NO_TOOL_PENDING_RETRIES {
                                log::warn!(
                                    "[TEXT_ORCHESTRATED] AI returned no tool calls {} times with pending tasks — \
                                     auto-skipping current task",
                                    no_tool_pending_retries
                                );
                                if let Some(task_id) = orchestrator.complete_current_task() {
                                    self.broadcast_task_status_change(
                                        original_message.channel_id,
                                        session_id,
                                        task_id,
                                        "completed",
                                        "Auto-completed (agent could not proceed)",
                                    );
                                    self.broadcast_task_queue_update(
                                        original_message.channel_id,
                                        session_id,
                                        orchestrator,
                                    );
                                }
                                match self.advance_to_next_task_or_complete(
                                    original_message.channel_id,
                                    session_id,
                                    orchestrator,
                                ) {
                                    TaskAdvanceResult::AllTasksComplete => {
                                        orchestrator_complete = true;
                                        break;
                                    }
                                    _ => {
                                        no_tool_pending_retries = 0;
                                    }
                                }
                                continue;
                            }

                            log::info!(
                                "[TEXT_ORCHESTRATED] AI returned no tool calls but tasks are pending — forcing retry ({}/{})",
                                no_tool_pending_retries, MAX_NO_TOOL_PENDING_RETRIES
                            );
                            conversation.push(Message {
                                role: MessageRole::Assistant,
                                content: agent_response.body.clone(),
                            });
                            conversation.push(Message {
                                role: MessageRole::User,
                                content: "[SYSTEM] You have pending tasks to complete. Please call the appropriate tools to continue working on the current task. If a sub-agent failed or was cancelled, call `say_to_user` with `finished_task: true` to acknowledge and move on.".to_string(),
                            });
                            continue;
                        }

                        final_response = agent_response.body;
                        break;
                    }
                }
                None => {
                    // Broadcast that parsing failed - show the raw AI content for debugging
                    log::warn!("[TEXT_ORCHESTRATED] Failed to parse AI response, using raw content");
                    self.broadcaster.broadcast(GatewayEvent::agent_thinking(
                        original_message.channel_id,
                        Some(session_id),
                        &format!("Parse failed, raw AI response:\n{}", &ai_content[..ai_content.len().min(500)]),
                    ));

                    final_response = ai_content;
                    break;
                }
            }
        }

        self.finalize_tool_loop(
            original_message,
            session_id,
            is_safe_mode,
            orchestrator,
            orchestrator_complete,
            was_cancelled,
            waiting_for_user_response,
            memory_suppressed,
            &last_say_to_user_content,
            &tool_call_log,
            &final_response,
            &user_question_content,
            max_tool_iterations,
            iterations,
            watchdog,
        )
    }
}
