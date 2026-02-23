use crate::ai::multi_agent::Orchestrator;
use crate::channels::types::NormalizedMessage;
use crate::models::session_message::MessageRole as DbMessageRole;
use crate::models::CompletionStatus;
use crate::telemetry::Watchdog;
use std::sync::Arc;

use super::MessageDispatcher;

/// Result of attempting to advance to the next task in the queue
pub(super) enum TaskAdvanceResult {
    /// Started working on the next task
    NextTaskStarted,
    /// No more tasks remain, session should complete
    AllTasksComplete,
    /// No pending tasks but queue is in inconsistent state (has non-completed tasks)
    /// This shouldn't happen in normal operation
    InconsistentState,
}

impl MessageDispatcher {
    /// Save a memory entry when a chat session completes successfully.
    /// Extracts a meaningful summary rather than dumping raw I/O.
    pub(super) fn save_session_completion_memory(
        &self,
        user_input: &str,
        bot_response: &str,
        is_safe_mode: bool,
        agent_subtype: Option<&str>,
    ) {
        let enabled = self.db.get_bot_settings()
            .map(|s| s.chat_session_memory_generation)
            .unwrap_or(true);
        if !enabled { return; }

        if bot_response.is_empty() { return; }

        let identity_id: Option<&str> = if is_safe_mode { Some("safemode") } else { None };

        // Build a concise, useful summary instead of raw I/O dump
        let user_summary: String = user_input.chars().take(200).collect();
        let response_summary: String = bot_response.chars().take(400).collect();

        // Extract the first line/sentence of the response as a topic indicator
        let topic = bot_response
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(100)
            .collect::<String>();

        let entry = format!(
            "### Session: {}\n- **Asked:** {}\n- **Result:** {}",
            topic.trim(),
            user_summary.trim(),
            response_summary.trim(),
        );
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if let Err(e) = self.db.insert_memory(
            "daily_log",
            &entry,
            None,
            None,
            5,
            identity_id,
            None,
            None,
            None,
            Some("session_completion"),
            Some(&today),
            agent_subtype,
        ) {
            log::error!("[SESSION_MEMORY] Failed to insert daily log memory: {}", e);
        }
    }

    /// Try to advance to the next task in the queue.
    /// If a next task exists, marks it as in_progress and broadcasts updates.
    /// If no tasks remain, marks the session as complete in the database and broadcasts completion.
    /// Returns TaskAdvanceResult indicating what happened.
    pub(super) fn advance_to_next_task_or_complete(
        &self,
        channel_id: i64,
        session_id: i64,
        orchestrator: &mut Orchestrator,
    ) -> TaskAdvanceResult {
        if let Some(next_task) = orchestrator.pop_next_task() {
            log::info!(
                "[ORCHESTRATED_LOOP] Starting next task: {} - {}",
                next_task.id,
                next_task.description
            );
            self.broadcast_task_status_change(
                channel_id,
                session_id,
                next_task.id,
                "in_progress",
                &next_task.description,
            );
            self.broadcast_task_queue_update(channel_id, session_id, orchestrator);
            TaskAdvanceResult::NextTaskStarted
        } else if orchestrator.task_queue_is_empty() || orchestrator.all_tasks_complete() {
            // Queue is empty or all tasks completed - end the session
            log::info!("[ORCHESTRATED_LOOP] All tasks completed, stopping loop");
            self.active_cache.update_completion_status(session_id, CompletionStatus::Complete);
            self.broadcast_session_complete(channel_id, session_id);
            TaskAdvanceResult::AllTasksComplete
        } else {
            // No pending tasks but queue has non-completed tasks (inconsistent state)
            log::warn!(
                "[ORCHESTRATED_LOOP] No pending tasks but queue in inconsistent state (not empty, not all complete)"
            );
            TaskAdvanceResult::InconsistentState
        }
    }

    /// Finalization logic shared by both native and text tool loop paths:
    /// clearing active skill, saving orchestrator context, updating completion status,
    /// saving cancellation/max-iteration summaries, building final return value.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_tool_loop(
        &self,
        original_message: &NormalizedMessage,
        session_id: i64,
        is_safe_mode: bool,
        orchestrator: &mut Orchestrator,
        orchestrator_complete: bool,
        was_cancelled: bool,
        waiting_for_user_response: bool,
        memory_suppressed: bool,
        last_say_to_user_content: &str,
        last_say_to_user_id: Option<&str>,
        tool_call_log: &[String],
        final_summary: &str,
        user_question_content: &str,
        max_tool_iterations: usize,
        iterations: usize,
        watchdog: &Arc<Watchdog>,
    ) -> Result<(String, bool, Option<String>), String> {
        // Returns (response_text, already_delivered_via_say_to_user, message_id)
        // Clear active skill when the orchestrator loop completes
        if orchestrator_complete || was_cancelled {
            if orchestrator.context().active_skill.is_some() {
                log::info!("[ORCHESTRATED_LOOP] Clearing active skill on session completion");
                orchestrator.context_mut().active_skill = None;
            }
        }

        // Save orchestrator context for next turn (in-memory cache; flushed on evict)
        self.active_cache.save_agent_context(session_id, orchestrator.context());

        // Update completion status
        if was_cancelled {
            log::info!("[ORCHESTRATED_LOOP] Marking session {} as Cancelled", session_id);
            self.active_cache.update_completion_status(session_id, CompletionStatus::Cancelled);
            self.broadcast_session_complete(original_message.channel_id, session_id);
        } else if orchestrator_complete && !waiting_for_user_response {
            log::info!("[ORCHESTRATED_LOOP] Marking session {} as Complete", session_id);
            self.active_cache.update_completion_status(session_id, CompletionStatus::Complete);
            self.broadcast_session_complete(original_message.channel_id, session_id);
            if memory_suppressed {
                log::info!("[ORCHESTRATED_LOOP] Skipping session memory — memory-excluded tool was called");
            } else {
                // Prefer say_to_user content for memory, fall back to task_fully_completed summary
                let memory_content = if !last_say_to_user_content.is_empty() {
                    last_say_to_user_content
                } else {
                    final_summary
                };
                let subtype = orchestrator.current_subtype_key();
                let subtype_opt = if subtype.is_empty() { None } else { Some(subtype) };
                self.save_session_completion_memory(
                    &original_message.text,
                    memory_content,
                    is_safe_mode,
                    subtype_opt,
                );
            }
        }

        // Save cancellation summary
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

        // Emit session_completed reward with real iteration/tool counts
        // via RewardEmitter for richer scoring (efficiency bonus, iteration penalty).
        let success = orchestrator_complete && !was_cancelled;
        watchdog.reward_emitter().session_completed(
            success,
            iterations as u32,
            tool_call_log.len() as u32,
            max_tool_iterations as u32,
        );

        // Build final return: (response, already_delivered_via_say_to_user, message_id)
        if waiting_for_user_response {
            // Save the tool call log to the orchestrator context
            if !tool_call_log.is_empty() {
                let context_summary = format!(
                    "Before asking the user, I already completed these actions:\n{}",
                    tool_call_log.join("\n")
                );
                orchestrator.context_mut().waiting_for_user_context = Some(context_summary);
                self.active_cache.save_agent_context(session_id, orchestrator.context());
            }
            Ok((user_question_content.to_string(), false, None))
        } else if !last_say_to_user_content.is_empty() {
            // say_to_user content IS the final result — already broadcast via tool.result event.
            // dispatch() will store it as assistant message but should NOT re-broadcast.
            log::info!("[ORCHESTRATED_LOOP] Returning say_to_user content as final result ({} chars)", last_say_to_user_content.len());
            Ok((last_say_to_user_content.to_string(), true, last_say_to_user_id.map(|s| s.to_string())))
        } else if orchestrator_complete {
            Ok((final_summary.to_string(), false, None))
        } else if tool_call_log.is_empty() {
            // Mark session as Failed — hit max iterations with no work done
            self.active_cache.update_completion_status(session_id, CompletionStatus::Failed);
            self.broadcast_session_complete(original_message.channel_id, session_id);
            Err(format!(
                "Tool loop hit max iterations ({}) without completion",
                max_tool_iterations
            ))
        } else {
            // Max iterations with work done — mark as Failed (didn't complete normally)
            self.active_cache.update_completion_status(session_id, CompletionStatus::Failed);
            self.broadcast_session_complete(original_message.channel_id, session_id);
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
}
