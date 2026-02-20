//! Shared types for the opencode module service and its RPC clients.

use serde::{Deserialize, Serialize};

// =====================================================
// RPC Request Types
// =====================================================

/// Send a coding task to the OpenCode agent
#[derive(Debug, Serialize, Deserialize)]
pub struct PromptRequest {
    /// The coding task to perform
    pub task: String,
    /// Optional project directory (defaults to service working dir)
    pub project_path: Option<String>,
}

// =====================================================
// RPC Response Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> RpcResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// =====================================================
// Domain Types
// =====================================================

/// Result of a coding prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResult {
    pub session_id: String,
    /// The agent's text response
    pub response: String,
}

/// An active OpenCode session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// Service health status
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub uptime_secs: u64,
    pub opencode_healthy: bool,
    pub opencode_port: u16,
    pub total_prompts: u64,
}
