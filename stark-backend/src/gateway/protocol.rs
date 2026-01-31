use crate::models::{ExecutionTask, TaskMetrics};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event types for gateway broadcasts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    // Channel events
    ChannelStarted,
    ChannelStopped,
    ChannelError,
    ChannelMessage,
    // Agent events
    AgentResponse,
    AgentToolCall,     // Real-time tool call notification for chat display
    AgentModeChange,   // Multi-agent mode transition (Explore/Plan/Perform)
    AgentSubtypeChange, // Agent subtype change (Finance/CodeEngineer)
    AgentThinking,     // Progress update during long AI calls
    AgentError,        // Error notification (timeout, etc.)
    AgentWarning,      // Warning when agent tries to skip tool calls
    // Tool events
    ToolExecution,
    ToolResult,
    ToolWaiting,  // Tool is waiting for retry after transient error
    // Skill events
    SkillInvoked,
    // Execution progress events
    ExecutionStarted,
    ExecutionThinking,
    ExecutionTaskStarted,
    ExecutionTaskUpdated,
    ExecutionTaskCompleted,
    ExecutionCompleted,
    // Payment events
    X402Payment,
    // Confirmation events
    ConfirmationRequired,
    ConfirmationApproved,
    ConfirmationRejected,
    ConfirmationExpired,
    // Transaction events
    TxPending,
    TxConfirmed,
    // Register events
    RegisterUpdate,
    // Multi-agent task events
    AgentTasksUpdate,
    AgentToolsetUpdate,  // Current tools available to agent
    // Sub-agent events
    SubagentSpawned,
    SubagentCompleted,
    SubagentFailed,
    // Streaming events
    StreamStart,
    StreamContentDelta,
    StreamToolStart,
    StreamToolDelta,
    StreamToolComplete,
    StreamThinkingDelta,
    StreamEnd,
    StreamError,
    // Process execution events
    ExecOutput,        // Real-time stdout/stderr line from exec
    ProcessStarted,    // Background process started
    ProcessOutput,     // Background process output chunk
    ProcessCompleted,  // Background process finished
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChannelStarted => "channel.started",
            Self::ChannelStopped => "channel.stopped",
            Self::ChannelError => "channel.error",
            Self::ChannelMessage => "channel.message",
            Self::AgentResponse => "agent.response",
            Self::AgentToolCall => "agent.tool_call",
            Self::AgentModeChange => "agent.mode_change",
            Self::AgentSubtypeChange => "agent.subtype_change",
            Self::AgentThinking => "agent.thinking",
            Self::AgentError => "agent.error",
            Self::AgentWarning => "agent.warning",
            Self::ToolExecution => "tool.execution",
            Self::ToolResult => "tool.result",
            Self::ToolWaiting => "tool.waiting",
            Self::SkillInvoked => "skill.invoked",
            Self::ExecutionStarted => "execution.started",
            Self::ExecutionThinking => "execution.thinking",
            Self::ExecutionTaskStarted => "execution.task_started",
            Self::ExecutionTaskUpdated => "execution.task_updated",
            Self::ExecutionTaskCompleted => "execution.task_completed",
            Self::ExecutionCompleted => "execution.completed",
            Self::X402Payment => "x402.payment",
            Self::ConfirmationRequired => "confirmation.required",
            Self::ConfirmationApproved => "confirmation.approved",
            Self::ConfirmationRejected => "confirmation.rejected",
            Self::ConfirmationExpired => "confirmation.expired",
            Self::TxPending => "tx.pending",
            Self::TxConfirmed => "tx.confirmed",
            Self::RegisterUpdate => "register.update",
            Self::AgentTasksUpdate => "agent.tasks_update",
            Self::AgentToolsetUpdate => "agent.toolset_update",
            Self::SubagentSpawned => "subagent.spawned",
            Self::SubagentCompleted => "subagent.completed",
            Self::SubagentFailed => "subagent.failed",
            Self::StreamStart => "stream.start",
            Self::StreamContentDelta => "stream.content_delta",
            Self::StreamToolStart => "stream.tool_start",
            Self::StreamToolDelta => "stream.tool_delta",
            Self::StreamToolComplete => "stream.tool_complete",
            Self::StreamThinkingDelta => "stream.thinking_delta",
            Self::StreamEnd => "stream.end",
            Self::StreamError => "stream.error",
            Self::ExecOutput => "exec.output",
            Self::ProcessStarted => "process.started",
            Self::ProcessOutput => "process.output",
            Self::ProcessCompleted => "process.completed",
        }
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<EventType> for String {
    fn from(event_type: EventType) -> Self {
        event_type.as_str().to_string()
    }
}

/// JSON-RPC request from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC response to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn success(id: String, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: String, error: RpcError) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid request")
    }

    pub fn method_not_found() -> Self {
        Self::new(-32601, "Method not found")
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(-32603, message)
    }
}

/// Server-push event to all connected clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub event: String,
    pub data: Value,
}

impl GatewayEvent {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            type_: "event".to_string(),
            event: event.into(),
            data,
        }
    }

    pub fn channel_started(channel_id: i64, channel_type: &str, name: &str) -> Self {
        Self::new(
            EventType::ChannelStarted,
            serde_json::json!({
                "channel_id": channel_id,
                "channel_type": channel_type,
                "name": name
            }),
        )
    }

    pub fn channel_stopped(channel_id: i64, channel_type: &str, name: &str) -> Self {
        Self::new(
            EventType::ChannelStopped,
            serde_json::json!({
                "channel_id": channel_id,
                "channel_type": channel_type,
                "name": name
            }),
        )
    }

    pub fn channel_error(channel_id: i64, error: &str) -> Self {
        Self::new(
            EventType::ChannelError,
            serde_json::json!({
                "channel_id": channel_id,
                "error": error
            }),
        )
    }

    pub fn channel_message(
        channel_id: i64,
        channel_type: &str,
        from: &str,
        text: &str,
    ) -> Self {
        Self::new(
            EventType::ChannelMessage,
            serde_json::json!({
                "channel_id": channel_id,
                "channel_type": channel_type,
                "from": from,
                "text": text
            }),
        )
    }

    pub fn agent_response(channel_id: i64, to: &str, text: &str) -> Self {
        Self::new(
            EventType::AgentResponse,
            serde_json::json!({
                "channel_id": channel_id,
                "to": to,
                "text": text
            }),
        )
    }

    /// Emit a tool call notification for real-time display in chat
    pub fn agent_tool_call(channel_id: i64, tool_name: &str, parameters: &Value) -> Self {
        Self::new(
            EventType::AgentToolCall,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_name": tool_name,
                "parameters": parameters
            }),
        )
    }

    /// Emit agent mode change for UI header display
    pub fn agent_mode_change(channel_id: i64, mode: &str, label: &str, reason: Option<&str>) -> Self {
        Self::new(
            EventType::AgentModeChange,
            serde_json::json!({
                "channel_id": channel_id,
                "mode": mode,
                "label": label,
                "reason": reason,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Emit agent subtype change for UI header display (Finance/CodeEngineer)
    pub fn agent_subtype_change(channel_id: i64, subtype: &str, label: &str) -> Self {
        Self::new(
            EventType::AgentSubtypeChange,
            serde_json::json!({
                "channel_id": channel_id,
                "subtype": subtype,
                "label": label,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Emit progress update during long AI calls
    pub fn agent_thinking(channel_id: i64, message: &str) -> Self {
        Self::new(
            EventType::AgentThinking,
            serde_json::json!({
                "channel_id": channel_id,
                "message": message,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Emit error notification (timeout, etc.)
    pub fn agent_error(channel_id: i64, error: &str) -> Self {
        Self::new(
            EventType::AgentError,
            serde_json::json!({
                "channel_id": channel_id,
                "error": error,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Emit warning when agent tries to respond without calling tools
    pub fn agent_warning(channel_id: i64, warning_type: &str, message: &str, attempt: u32) -> Self {
        Self::new(
            EventType::AgentWarning,
            serde_json::json!({
                "channel_id": channel_id,
                "warning_type": warning_type,
                "message": message,
                "attempt": attempt,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    pub fn tool_execution(channel_id: i64, tool_name: &str, parameters: &Value) -> Self {
        Self::new(
            EventType::ToolExecution,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_name": tool_name,
                "parameters": parameters
            }),
        )
    }

    pub fn tool_result(channel_id: i64, tool_name: &str, success: bool, duration_ms: i64, content: &str) -> Self {
        Self::new(
            EventType::ToolResult,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_name": tool_name,
                "success": success,
                "duration_ms": duration_ms,
                "content": content
            }),
        )
    }

    /// Tool is waiting for retry after transient network error (exponential backoff)
    pub fn tool_waiting(channel_id: i64, tool_name: &str, wait_seconds: u64) -> Self {
        Self::new(
            EventType::ToolWaiting,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_name": tool_name,
                "wait_seconds": wait_seconds,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    pub fn skill_invoked(channel_id: i64, skill_name: &str) -> Self {
        Self::new(
            EventType::SkillInvoked,
            serde_json::json!({
                "channel_id": channel_id,
                "skill_name": skill_name
            }),
        )
    }

    // =====================================================
    // Execution Progress Events
    // =====================================================

    /// Execution started (plan mode or direct execution)
    pub fn execution_started(
        channel_id: i64,
        execution_id: &str,
        mode: &str,
        description: &str,
        active_form: &str,
    ) -> Self {
        Self::new(
            EventType::ExecutionStarted,
            serde_json::json!({
                "channel_id": channel_id,
                "execution_id": execution_id,
                "mode": mode,  // "plan" or "execute"
                "description": description,
                "active_form": active_form
            }),
        )
    }

    /// AI is thinking/reasoning
    pub fn execution_thinking(channel_id: i64, execution_id: &str, text: &str) -> Self {
        Self::new(
            EventType::ExecutionThinking,
            serde_json::json!({
                "channel_id": channel_id,
                "execution_id": execution_id,
                "text": text
            }),
        )
    }

    /// Task started (tool, sub-agent, etc.)
    pub fn task_started(task: &ExecutionTask, execution_id: &str) -> Self {
        Self::new(
            EventType::ExecutionTaskStarted,
            serde_json::json!({
                "id": task.id,
                "execution_id": execution_id,
                "parent_id": task.parent_id,
                "parent_task_id": task.parent_id,  // Alias for frontend compatibility
                "channel_id": task.channel_id,
                "type": task.task_type.to_string(),
                "name": task.description,  // Frontend expects 'name' field
                "description": task.description,
                "active_form": task.active_form,
                "status": task.status.to_string()
            }),
        )
    }

    /// Task metrics updated
    pub fn task_updated(task_id: &str, channel_id: i64, metrics: &TaskMetrics) -> Self {
        Self::new(
            EventType::ExecutionTaskUpdated,
            serde_json::json!({
                "task_id": task_id,
                "channel_id": channel_id,
                "metrics": {
                    "tool_uses": metrics.tool_uses,
                    "tokens_used": metrics.tokens_used,
                    "lines_read": metrics.lines_read,
                    "duration_ms": metrics.duration_ms
                }
            }),
        )
    }

    /// Task metrics and active form updated
    pub fn task_updated_with_active_form(task_id: &str, channel_id: i64, metrics: &TaskMetrics, active_form: &str) -> Self {
        Self::new(
            EventType::ExecutionTaskUpdated,
            serde_json::json!({
                "task_id": task_id,
                "channel_id": channel_id,
                "active_form": active_form,
                "metrics": {
                    "tool_uses": metrics.tool_uses,
                    "tokens_used": metrics.tokens_used,
                    "lines_read": metrics.lines_read,
                    "duration_ms": metrics.duration_ms
                }
            }),
        )
    }

    /// Task completed
    pub fn task_completed(task_id: &str, channel_id: i64, status: &str, metrics: &TaskMetrics) -> Self {
        Self::new(
            EventType::ExecutionTaskCompleted,
            serde_json::json!({
                "task_id": task_id,
                "channel_id": channel_id,
                "status": status,
                "metrics": {
                    "tool_uses": metrics.tool_uses,
                    "tokens_used": metrics.tokens_used,
                    "lines_read": metrics.lines_read,
                    "duration_ms": metrics.duration_ms
                }
            }),
        )
    }

    /// Execution completed
    pub fn execution_completed(channel_id: i64, execution_id: &str, total_metrics: &TaskMetrics) -> Self {
        Self::new(
            EventType::ExecutionCompleted,
            serde_json::json!({
                "channel_id": channel_id,
                "execution_id": execution_id,
                "metrics": {
                    "tool_uses": total_metrics.tool_uses,
                    "tokens_used": total_metrics.tokens_used,
                    "duration_ms": total_metrics.duration_ms
                }
            }),
        )
    }

    // =====================================================
    // Confirmation Events
    // =====================================================

    /// Confirmation required for a tool execution
    pub fn confirmation_required(
        channel_id: i64,
        confirmation_id: &str,
        tool_name: &str,
        description: &str,
        parameters: &Value,
    ) -> Self {
        Self::new(
            EventType::ConfirmationRequired,
            serde_json::json!({
                "channel_id": channel_id,
                "confirmation_id": confirmation_id,
                "tool_name": tool_name,
                "description": description,
                "parameters": parameters,
                "instructions": "Type /confirm to execute or /cancel to abort",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Confirmation approved and tool executing
    pub fn confirmation_approved(channel_id: i64, confirmation_id: &str, tool_name: &str) -> Self {
        Self::new(
            EventType::ConfirmationApproved,
            serde_json::json!({
                "channel_id": channel_id,
                "confirmation_id": confirmation_id,
                "tool_name": tool_name,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Confirmation rejected by user
    pub fn confirmation_rejected(channel_id: i64, confirmation_id: &str, tool_name: &str) -> Self {
        Self::new(
            EventType::ConfirmationRejected,
            serde_json::json!({
                "channel_id": channel_id,
                "confirmation_id": confirmation_id,
                "tool_name": tool_name,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Confirmation expired
    pub fn confirmation_expired(channel_id: i64, confirmation_id: &str, tool_name: &str) -> Self {
        Self::new(
            EventType::ConfirmationExpired,
            serde_json::json!({
                "channel_id": channel_id,
                "confirmation_id": confirmation_id,
                "tool_name": tool_name,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Custom event with arbitrary event name and data
    pub fn custom(event: &str, data: Value) -> Self {
        Self::new(event, data)
    }

    /// Transaction pending - broadcast when tx is sent but not yet mined
    pub fn tx_pending(
        channel_id: i64,
        tx_hash: &str,
        network: &str,
        explorer_url: &str,
    ) -> Self {
        Self::new(
            EventType::TxPending,
            serde_json::json!({
                "channel_id": channel_id,
                "tx_hash": tx_hash,
                "network": network,
                "explorer_url": explorer_url,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Transaction confirmed - broadcast when tx is mined
    pub fn tx_confirmed(
        channel_id: i64,
        tx_hash: &str,
        network: &str,
        status: &str,
    ) -> Self {
        Self::new(
            EventType::TxConfirmed,
            serde_json::json!({
                "channel_id": channel_id,
                "tx_hash": tx_hash,
                "network": network,
                "status": status,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// x402 payment made
    pub fn x402_payment(
        channel_id: i64,
        amount: &str,
        amount_formatted: &str,
        asset: &str,
        pay_to: &str,
        resource: Option<&str>,
    ) -> Self {
        Self::new(
            EventType::X402Payment,
            serde_json::json!({
                "channel_id": channel_id,
                "amount": amount,
                "amount_formatted": amount_formatted,
                "asset": asset,
                "pay_to": pay_to,
                "resource": resource,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Register updated - broadcast full registry state
    pub fn register_update(
        channel_id: i64,
        registers: Value,
    ) -> Self {
        Self::new(
            EventType::RegisterUpdate,
            serde_json::json!({
                "channel_id": channel_id,
                "registers": registers,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Multi-agent task list updated
    pub fn agent_tasks_update(
        channel_id: i64,
        mode: &str,
        mode_label: &str,
        tasks: Value,
        stats: Value,
    ) -> Self {
        Self::new(
            EventType::AgentTasksUpdate,
            serde_json::json!({
                "channel_id": channel_id,
                "mode": mode,
                "mode_label": mode_label,
                "tasks": tasks,
                "stats": stats,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Agent toolset updated - broadcast current tools available to the agent
    pub fn agent_toolset_update(
        channel_id: i64,
        mode: &str,
        subtype: &str,
        tools: Vec<Value>,
    ) -> Self {
        Self::new(
            EventType::AgentToolsetUpdate,
            serde_json::json!({
                "channel_id": channel_id,
                "mode": mode,
                "subtype": subtype,
                "tools": tools,
                "count": tools.len(),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    // =====================================================
    // Streaming Events
    // =====================================================

    /// Stream started - broadcast when streaming response begins
    pub fn stream_start(channel_id: i64, session_id: Option<i64>) -> Self {
        Self::new(
            EventType::StreamStart,
            serde_json::json!({
                "channel_id": channel_id,
                "session_id": session_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Content delta - incremental text content
    pub fn stream_content_delta(channel_id: i64, content: &str, index: usize) -> Self {
        Self::new(
            EventType::StreamContentDelta,
            serde_json::json!({
                "channel_id": channel_id,
                "content": content,
                "index": index
            }),
        )
    }

    /// Tool call started - broadcast when a tool call begins streaming
    pub fn stream_tool_start(channel_id: i64, tool_id: &str, tool_name: &str, index: usize) -> Self {
        Self::new(
            EventType::StreamToolStart,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_id": tool_id,
                "tool_name": tool_name,
                "index": index,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Tool call arguments delta - incremental arguments JSON
    pub fn stream_tool_delta(channel_id: i64, tool_id: &str, arguments_delta: &str, index: usize) -> Self {
        Self::new(
            EventType::StreamToolDelta,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_id": tool_id,
                "arguments_delta": arguments_delta,
                "index": index
            }),
        )
    }

    /// Tool call complete - broadcast when tool call arguments are fully streamed
    pub fn stream_tool_complete(
        channel_id: i64,
        tool_id: &str,
        tool_name: &str,
        arguments: &Value,
        index: usize,
    ) -> Self {
        Self::new(
            EventType::StreamToolComplete,
            serde_json::json!({
                "channel_id": channel_id,
                "tool_id": tool_id,
                "tool_name": tool_name,
                "arguments": arguments,
                "index": index,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Thinking delta - incremental thinking/reasoning content (Claude)
    pub fn stream_thinking_delta(channel_id: i64, content: &str) -> Self {
        Self::new(
            EventType::StreamThinkingDelta,
            serde_json::json!({
                "channel_id": channel_id,
                "content": content
            }),
        )
    }

    /// Stream ended - broadcast when streaming completes
    pub fn stream_end(
        channel_id: i64,
        stop_reason: Option<&str>,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    ) -> Self {
        Self::new(
            EventType::StreamEnd,
            serde_json::json!({
                "channel_id": channel_id,
                "stop_reason": stop_reason,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens
                },
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Stream error - broadcast when an error occurs during streaming
    pub fn stream_error(channel_id: i64, error: &str, code: Option<&str>) -> Self {
        Self::new(
            EventType::StreamError,
            serde_json::json!({
                "channel_id": channel_id,
                "error": error,
                "code": code,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    // =====================================================
    // Process Execution Events
    // =====================================================

    /// Real-time output line from exec command
    pub fn exec_output(channel_id: i64, line: &str, stream: &str) -> Self {
        Self::new(
            EventType::ExecOutput,
            serde_json::json!({
                "channel_id": channel_id,
                "line": line,
                "stream": stream,  // "stdout" or "stderr"
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Background process started
    pub fn process_started(channel_id: i64, process_id: &str, command: &str, pid: u32) -> Self {
        Self::new(
            EventType::ProcessStarted,
            serde_json::json!({
                "channel_id": channel_id,
                "process_id": process_id,
                "command": command,
                "pid": pid,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Background process output chunk
    pub fn process_output(channel_id: i64, process_id: &str, lines: &[String], stream: &str) -> Self {
        Self::new(
            EventType::ProcessOutput,
            serde_json::json!({
                "channel_id": channel_id,
                "process_id": process_id,
                "lines": lines,
                "stream": stream,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }

    /// Background process completed
    pub fn process_completed(channel_id: i64, process_id: &str, exit_code: Option<i32>, duration_ms: i64) -> Self {
        Self::new(
            EventType::ProcessCompleted,
            serde_json::json!({
                "channel_id": channel_id,
                "process_id": process_id,
                "exit_code": exit_code,
                "duration_ms": duration_ms,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
    }
}

/// Params for channel operations
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelIdParams {
    pub id: i64,
}
