//! Types for the tool validator subsystem

use crate::tools::types::ToolContext;
use serde_json::Value;
use std::sync::Arc;

/// Priority levels for validator execution order
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValidatorPriority {
    /// Execute first (security checks)
    Critical = 0,
    /// Execute early
    High = 100,
    /// Normal execution order
    Normal = 500,
    /// Execute later
    Low = 900,
}

impl Default for ValidatorPriority {
    fn default() -> Self {
        ValidatorPriority::Normal
    }
}

/// Context passed to validators during execution
#[derive(Clone)]
pub struct ValidationContext {
    /// Name of the tool being called
    pub tool_name: String,
    /// Arguments passed to the tool
    pub tool_args: Value,
    /// Channel ID (if available)
    pub channel_id: Option<i64>,
    /// Session ID (if available)
    pub session_id: Option<i64>,
    /// Full tool context with access to credentials, DB, etc.
    pub tool_context: Arc<ToolContext>,
}

impl ValidationContext {
    /// Create a new validation context
    pub fn new(
        tool_name: String,
        tool_args: Value,
        tool_context: Arc<ToolContext>,
    ) -> Self {
        Self {
            tool_name,
            tool_args,
            channel_id: tool_context.channel_id,
            session_id: tool_context.session_id,
            tool_context,
        }
    }

    /// Set channel context
    pub fn with_channel(mut self, channel_id: i64) -> Self {
        self.channel_id = Some(channel_id);
        self
    }

    /// Set session context
    pub fn with_session(mut self, session_id: i64) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

/// Result of a validation check
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// Allow the tool call to proceed
    Allow,
    /// Block the tool call with a reason
    Block(String),
    /// Block with a reason and a suggestion for what to do instead
    BlockWithSuggestion {
        reason: String,
        suggestion: String,
    },
}

impl ValidationResult {
    /// Check if the validation allows the tool call
    pub fn is_allowed(&self) -> bool {
        matches!(self, ValidationResult::Allow)
    }

    /// Check if the validation blocks the tool call
    pub fn is_blocked(&self) -> bool {
        !self.is_allowed()
    }

    /// Get the block reason if blocked
    pub fn block_reason(&self) -> Option<&str> {
        match self {
            ValidationResult::Allow => None,
            ValidationResult::Block(reason) => Some(reason),
            ValidationResult::BlockWithSuggestion { reason, .. } => Some(reason),
        }
    }

    /// Get the suggestion if available
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            ValidationResult::BlockWithSuggestion { suggestion, .. } => Some(suggestion),
            _ => None,
        }
    }

    /// Format the full error message for display to the agent
    pub fn to_error_message(&self) -> Option<String> {
        match self {
            ValidationResult::Allow => None,
            ValidationResult::Block(reason) => Some(format!("Blocked: {}", reason)),
            ValidationResult::BlockWithSuggestion { reason, suggestion } => {
                Some(format!("Blocked: {} Suggestion: {}", reason, suggestion))
            }
        }
    }
}
