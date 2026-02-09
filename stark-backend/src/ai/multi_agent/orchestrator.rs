//! Simplified orchestrator - manages agent context without mode transitions

use super::tools;
use super::types::{AgentContext, AgentMode};
use crate::tools::ToolDefinition;
use serde_json::Value;

/// Maximum iterations before forcing completion
const MAX_ITERATIONS: u32 = 100;

/// The orchestrator manages agent context and tool processing
pub struct Orchestrator {
    context: AgentContext,
}

impl Orchestrator {
    /// Create a new orchestrator for a request
    pub fn new(original_request: String) -> Self {
        Self {
            context: AgentContext {
                original_request,
                mode: AgentMode::TaskPlanner,
                planner_completed: false,
                ..Default::default()
            },
        }
    }

    /// Create from existing context (for resuming)
    pub fn from_context(context: AgentContext) -> Self {
        Self { context }
    }

    /// Get the current mode (always Assistant now)
    pub fn current_mode(&self) -> AgentMode {
        self.context.mode
    }

    /// Get the full context
    pub fn context(&self) -> &AgentContext {
        &self.context
    }

    /// Get mutable context
    pub fn context_mut(&mut self) -> &mut AgentContext {
        &mut self.context
    }

    /// Record that an actual tool was called
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.context.actual_tool_calls += 1;
        self.context.no_tool_warnings = 0;

        log::debug!(
            "[ORCHESTRATOR] Tool '{}' called (total: {})",
            tool_name,
            self.context.actual_tool_calls
        );

        // Track skill-specific tool calls if a skill is active
        if let Some(ref mut skill) = self.context.active_skill {
            skill.tool_calls_made += 1;
            log::debug!(
                "[ORCHESTRATOR] Tool '{}' called with active skill '{}' (skill total: {})",
                tool_name,
                skill.name,
                skill.tool_calls_made
            );
        }
    }

    /// Clear the active skill
    pub fn clear_active_skill(&mut self) {
        if let Some(ref skill) = self.context.active_skill {
            log::debug!(
                "[ORCHESTRATOR] Clearing active skill '{}' (made {} tool calls)",
                skill.name,
                skill.tool_calls_made
            );
        }
        self.context.active_skill = None;
    }

    /// Check if the agent should have called tools but didn't
    /// Returns (error_message, warning_count) if tool calls were required but skipped
    pub fn check_tool_call_required(&mut self) -> Option<(String, u32)> {
        // If we've already warned too many times (5), let it through to avoid infinite loop
        // But this is a serious failure - the agent is not following instructions
        if self.context.no_tool_warnings >= 5 {
            log::error!(
                "[ORCHESTRATOR] CRITICAL: Agent skipped tool calls {} times! Allowing response to prevent infinite loop, but this indicates a serious problem.",
                self.context.no_tool_warnings
            );
            return None;
        }

        // If no actual tools were called yet, the agent is probably hallucinating
        if self.context.actual_tool_calls == 0 && self.context.mode_iterations > 0 {
            self.context.no_tool_warnings += 1;
            log::warn!(
                "[ORCHESTRATOR] Agent tried to respond without calling any tools (warning {}/5)",
                self.context.no_tool_warnings
            );

            let message = format!(
                "⚠️ WARNING {}/5: You MUST call actual tools before responding!\n\n\
                You should:\n\
                1. Use `use_skill` to load relevant skill instructions (e.g., 'local_wallet' for balance queries)\n\
                2. Use lookup tools like `token_lookup` for token info\n\
                3. Use `x402_rpc` or `web3_preset_function_call` for blockchain data\n\
                4. Use `read_file` or `list_files` to explore files\n\n\
                ❌ Do NOT fabricate, guess, or assume data.\n\
                ❌ Do NOT respond with made-up balances or addresses.\n\
                ✅ Call the appropriate tools to get REAL information.\n\n\
                Original request: {}",
                self.context.no_tool_warnings,
                self.context.original_request
            );

            return Some((message, self.context.no_tool_warnings));
        }

        None
    }

    /// Get the current agent subtype
    pub fn current_subtype(&self) -> super::types::AgentSubtype {
        self.context.subtype
    }

    /// Set the agent subtype
    pub fn set_subtype(&mut self, subtype: super::types::AgentSubtype) {
        self.context.subtype = subtype;
    }

    /// Get the system prompt for task planner mode
    pub fn get_planner_prompt(&self) -> String {
        self.get_planner_prompt_with_skills("No skills available.")
    }

    /// Get the system prompt for task planner mode with available skills
    pub fn get_planner_prompt_with_skills(&self, skills_text: &str) -> String {
        include_str!("prompts/task_planner.md")
            .replace("{original_request}", &self.context.original_request)
            .replace("{available_skills}", skills_text)
    }

    /// Get the system prompt
    pub fn get_system_prompt(&self) -> String {
        // If in task planner mode, return the planner prompt
        if self.context.mode == AgentMode::TaskPlanner && !self.context.planner_completed {
            return self.get_planner_prompt();
        }

        let base_prompt = include_str!("prompts/assistant.md");

        let mut prompt = String::new();

        // CURRENT TASK goes FIRST — highest priority for the AI to see
        if let Some(task) = self.context.task_queue.current_task() {
            let total = self.context.task_queue.total();
            let completed = self.context.task_queue.completed_count();

            // Detect "Use skill: X" in task description and inject explicit use_skill instruction
            let skill_instruction = if let Some(caps) = task.description
                .find("Use skill: ")
                .and_then(|start| {
                    let rest = &task.description[start + 11..];
                    // Extract skill name (up to whitespace or end)
                    let skill_name = rest.split_whitespace().next();
                    skill_name.map(|s| s.to_string())
                })
            {
                format!(
                    "\n\n**⚡ ACTION REQUIRED:** Call `use_skill(skill_name=\"{}\")` to load this skill's instructions, then follow them step by step.",
                    caps
                )
            } else {
                String::new()
            };

            let auto_complete_hint = if let Some(ref tool_name) = task.auto_complete_tool {
                format!(
                    "\n\n**Note:** This task will auto-complete when `{}` succeeds. \
                     You do NOT need to call `task_fully_completed` for this task.",
                    tool_name
                )
            } else {
                String::new()
            };

            prompt.push_str(&format!(
                "# >>> CURRENT TASK ({}/{}) <<<\n\n{}{}{}\n\n\
                 **YOU MUST**: Complete ONLY this task. Do NOT skip ahead. \
                 When done, call `say_to_user` with `finished_task: true` or `task_fully_completed` with a summary.\n\n---\n\n",
                completed + 1,
                total,
                task.description,
                skill_instruction,
                auto_complete_hint,
            ));
        }

        prompt.push_str(base_prompt);
        prompt.push_str("\n\n---\n\n");
        prompt.push_str(&self.format_context_summary());

        prompt
    }

    /// Format a summary of the current context for the prompt
    fn format_context_summary(&self) -> String {
        const MAX_NOTES: usize = 10;
        const MAX_SCRATCHPAD_LEN: usize = 1000;

        let mut summary = String::new();

        summary.push_str("## Current Context\n\n");
        summary.push_str(&format!("**Request**: {}\n\n", self.context.original_request));
        summary.push_str(&format!("**Subtype**: {} {}\n\n",
            self.context.subtype.emoji(),
            self.context.subtype.label()
        ));

        // Add selected network - this is the network the user has selected in the UI
        // The agent should use this for all web3 operations unless the user explicitly specifies otherwise
        if let Some(ref network) = self.context.selected_network {
            summary.push_str(&format!("**Selected Network**: {} (use this for web3 tool calls unless user explicitly specifies a different network)\n\n", network));
        }

        // Add notes (capped, most recent)
        if !self.context.exploration_notes.is_empty() {
            summary.push_str("### Notes\n\n");
            let notes_len = self.context.exploration_notes.len();
            let skip = notes_len.saturating_sub(MAX_NOTES);
            if skip > 0 {
                summary.push_str(&format!("_(showing last {} of {} notes)_\n", MAX_NOTES, notes_len));
            }
            for note in self.context.exploration_notes.iter().skip(skip) {
                summary.push_str(&format!("- {}\n", note));
            }
            summary.push('\n');
        }

        // Add active skill context
        if let Some(ref skill) = self.context.active_skill {
            summary.push_str("### Active Skill (FOLLOW THESE INSTRUCTIONS)\n\n");
            summary.push_str(&format!("**Skill**: {}\n\n", skill.name));
            summary.push_str("**Instructions**:\n");
            summary.push_str(&skill.instructions);
            summary.push_str("\n\n");
            summary.push_str("**IMPORTANT**: Call the actual tools mentioned above. Do NOT call `use_skill` again.\n\n");
        }

        // Add scratchpad if not empty (truncated)
        if !self.context.scratchpad.is_empty() {
            summary.push_str("### Scratchpad\n\n");
            if self.context.scratchpad.len() > MAX_SCRATCHPAD_LEN {
                summary.push_str(&self.context.scratchpad[..MAX_SCRATCHPAD_LEN]);
                summary.push_str("\n_(truncated)_\n\n");
            } else {
                summary.push_str(&self.context.scratchpad);
                summary.push_str("\n\n");
            }
        }

        // Add waiting for user context (if any) - this shows what tools were called before asking user
        if let Some(ref waiting_context) = self.context.waiting_for_user_context {
            summary.push_str("### Actions Completed Before User Question\n\n");
            summary.push_str("**IMPORTANT**: The following actions were ALREADY completed in a previous turn. Do NOT repeat them.\n\n");
            summary.push_str(waiting_context);
            summary.push_str("\n\n");
        }

        summary
    }

    /// Clear the waiting_for_user_context after it's been consumed
    pub fn clear_waiting_for_user_context(&mut self) {
        self.context.waiting_for_user_context = None;
    }

    /// Get the tools available
    pub fn get_mode_tools(&self) -> Vec<ToolDefinition> {
        tools::get_tools_for_mode(self.context.mode)
    }

    /// Process a tool call result
    pub fn process_tool_result(&mut self, tool_name: &str, params: &Value) -> ProcessResult {
        self.context.mode_iterations += 1;
        self.context.total_iterations += 1;

        log::debug!(
            "[ORCHESTRATOR] Processing tool '{}' (iteration {})",
            tool_name, self.context.mode_iterations
        );

        // define_tasks is now a registered tool handled via metadata interception
        // in the dispatcher (same pattern as add_task)
        ProcessResult::Continue
    }

    /// Check if we should force completion due to hitting max iterations
    pub fn check_forced_transition(&mut self) -> Option<super::types::ModeTransition> {
        if self.context.mode_iterations >= MAX_ITERATIONS {
            log::warn!(
                "[ORCHESTRATOR] Forced completion after {} iterations",
                MAX_ITERATIONS
            );
            // Return None - we don't do mode transitions anymore
            // The dispatcher should handle this case
            None
        } else {
            None
        }
    }

    // =========================================================================
    /// Transition to assistant mode after planner completes
    pub fn transition_to_assistant(&mut self) {
        self.context.mode = AgentMode::Assistant;
        self.context.planner_completed = true;
    }

    /// Pop the next task from the queue
    pub fn pop_next_task(&mut self) -> Option<&super::types::PlannerTask> {
        self.context.task_queue.pop_next()
    }

    /// Complete the current task
    pub fn complete_current_task(&mut self) -> Option<u32> {
        self.context.task_queue.complete_current()
    }

    /// Check if all tasks are complete
    pub fn all_tasks_complete(&self) -> bool {
        self.context.task_queue.all_complete()
    }

    /// Check if task queue is empty (no tasks defined)
    pub fn task_queue_is_empty(&self) -> bool {
        self.context.task_queue.is_empty()
    }

    /// Get the task queue for broadcasting
    pub fn task_queue(&self) -> &super::types::TaskQueue {
        &self.context.task_queue
    }

    /// Delete a task by ID. Returns true if the task was found and deleted.
    /// Also returns whether the deleted task was the current one.
    pub fn delete_task(&mut self, task_id: u32) -> (bool, bool) {
        let was_current = self.context.task_queue
            .current_task()
            .map(|t| t.id == task_id)
            .unwrap_or(false);
        let deleted = self.context.task_queue.delete_task(task_id);
        (deleted, was_current)
    }

    /// Get a task by ID
    pub fn get_task(&self, task_id: u32) -> Option<&super::types::PlannerTask> {
        self.context.task_queue.get_task(task_id)
    }

    /// Insert a task right after the current task (will be executed next)
    pub fn insert_task_front(&mut self, description: String) -> Vec<u32> {
        self.context.task_queue.insert_after_current(vec![description])
    }

    /// Append a task at the end of the queue
    pub fn append_task(&mut self, description: String) -> Vec<u32> {
        self.context.task_queue.append_tasks(vec![description])
    }
}

/// Result of processing a tool call
#[derive(Debug)]
pub enum ProcessResult {
    /// Continue processing
    Continue,
    /// Tool executed successfully with result
    ToolResult(String),
    /// Task is complete with summary
    Complete(String),
    /// Error occurred
    Error(String),
}
