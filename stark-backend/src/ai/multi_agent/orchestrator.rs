//! Simplified orchestrator - manages agent context without mode transitions

use super::tools;
use super::types::{AgentContext, AgentMode};
use crate::tools::ToolDefinition;
use serde_json::Value;

/// Maximum iterations before forcing completion
const MAX_ITERATIONS: u32 = 50;

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
                mode: AgentMode::Assistant,
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
                3. Use `x402_rpc` or `web3_function_call` for blockchain data\n\
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

    /// Get the system prompt
    pub fn get_system_prompt(&self) -> String {
        let base_prompt = include_str!("prompts/assistant.md");

        let mut prompt = base_prompt.to_string();
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

        match tool_name {
            "add_note" => self.handle_add_note(params),
            _ => ProcessResult::Continue,
        }
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
    // Tool handlers
    // =========================================================================

    fn handle_add_note(&mut self, params: &Value) -> ProcessResult {
        if let Some(note) = params.get("note").and_then(|v| v.as_str()) {
            self.context.exploration_notes.push(note.to_string());
            ProcessResult::ToolResult(format!("Note added: {}", note))
        } else {
            ProcessResult::Error("Missing 'note' parameter".to_string())
        }
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
