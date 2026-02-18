//! Sub-agent tools for spawning and monitoring background agent instances
//!
//! This module provides two tools:
//! - `spawn_subagents`: Spawn multiple sub-agents in parallel and wait for all results
//! - `subagent_status`: Check the status of sub-agents or cancel them

use crate::ai::archetypes::minimax::strip_think_blocks;
use crate::ai::multi_agent::{SubAgentContext, SubAgentManager, SubAgentStatus};
use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Counter for generating unique subagent IDs
static SUBAGENT_COUNTER: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// SpawnSubagentsTool — spawns multiple sub-agents in parallel, awaits all
// ---------------------------------------------------------------------------

/// Tool for spawning multiple background agent instances and awaiting their results.
///
/// Takes an array of agent specs, spawns all in parallel, polls until all
/// reach a terminal state (or overall timeout), and returns consolidated results.
pub struct SpawnSubagentsTool {
    definition: ToolDefinition,
}

impl SpawnSubagentsTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "agents".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Array of sub-agent specifications to spawn in parallel. Each element is an object with: \
                    task (string, required) — the task prompt; \
                    label (string) — short identifier like 'research' or 'analysis'; \
                    agent_subtype (string) — agent subtype key (e.g. 'superouter', 'finance', 'code_engineer'). \
                    REQUIRED — determines which tools and skills the sub-agent can use; \
                    model (string) — optional model override; \
                    thinking (string) — thinking level (off/minimal/low/medium/high/xhigh); \
                    timeout (integer) — per-agent timeout in seconds (default 300, max 3600); \
                    read_only (boolean) — restrict to read-only tools (default false); \
                    context (string) — additional context to pass.".to_string(),
                default: None,
                items: Some(Box::new(PropertySchema {
                    schema_type: "object".to_string(),
                    description: "Sub-agent specification with task, label, agent_subtype, model, thinking, timeout, read_only, and context fields".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "timeout".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Overall timeout in seconds to wait for all sub-agents (default: 600, max: 3600). \
                    If reached, returns partial results for completed agents and marks others as still running.".to_string(),
                default: Some(json!(600)),
                items: None,
                enum_values: None,
            },
        );

        SpawnSubagentsTool {
            definition: ToolDefinition {
                name: "spawn_subagents".to_string(),
                description: "Spawn multiple sub-agents in parallel and wait for all results. \
                    Each sub-agent runs autonomously with its own tools. All agents execute concurrently \
                    and the tool returns a consolidated report once all complete (or timeout is reached). \
                    Use this for parallel task execution, multi-domain work, or delegating subtasks.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["agents".to_string()],
                },
                group: ToolGroup::SubAgent,
                hidden: false,
            },
        }
    }
}

impl Default for SpawnSubagentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SpawnSubagentsParams {
    agents: Vec<AgentSpec>,
    timeout: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct AgentSpec {
    task: String,
    label: Option<String>,
    /// Agent subtype key — determines which tools/skills the sub-agent gets
    agent_subtype: Option<String>,
    model: Option<String>,
    thinking: Option<String>,
    timeout: Option<u64>,
    context: Option<String>,
    #[serde(default)]
    read_only: Option<bool>,
}

/// Progress interval for broadcasting await progress events (seconds)
const PROGRESS_INTERVAL_SECS: u64 = 15;
/// Poll interval for checking subagent statuses (seconds)
const POLL_INTERVAL_SECS: u64 = 2;
/// Idle threshold: warn if a subagent has no tool activity for this many seconds
const IDLE_WARN_SECS: i64 = 120;

#[async_trait]
impl Tool for SpawnSubagentsTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SpawnSubagentsParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        if params.agents.is_empty() {
            return ToolResult::success("No agents to spawn.").with_metadata(json!({
                "count": 0,
                "results": []
            }));
        }

        let overall_timeout = params.timeout.unwrap_or(600).min(3600);

        // Check if we have a real SubAgentManager with valid context
        // Note: channel_id 0 is valid (web channel), so we just check is_some()
        let has_valid_context = context.session_id.map(|id| id > 0).unwrap_or(false)
            && context.channel_id.is_some();

        if let Some(manager) = &context.subagent_manager {
            if has_valid_context {
                return self.execute_real(
                    &params.agents,
                    overall_timeout,
                    manager,
                    context,
                ).await;
            }
        }

        // No valid SubAgentManager context — return error
        ToolResult::error(
            "SubAgentManager not available or missing valid session/channel context. \
             Sub-agents require an active session with a configured SubAgentManager."
        )
    }
}

impl SpawnSubagentsTool {
    /// Real execution path: spawn all agents via SubAgentManager, poll until done
    async fn execute_real(
        &self,
        agents: &[AgentSpec],
        overall_timeout: u64,
        manager: &Arc<SubAgentManager>,
        context: &ToolContext,
    ) -> ToolResult {
        let session_id = context.session_id.unwrap();
        let channel_id = context.channel_id.unwrap();

        log::info!(
            "[SUBAGENTS] Spawning {} sub-agents in parallel (timeout: {}s)",
            agents.len(),
            overall_timeout
        );

        // Phase 1: Spawn all agents
        let mut spawned_ids: Vec<String> = Vec::with_capacity(agents.len());
        let mut spawned_labels: Vec<String> = Vec::with_capacity(agents.len());

        for (i, spec) in agents.iter().enumerate() {
            let counter = SUBAGENT_COUNTER.fetch_add(1, Ordering::SeqCst);
            let label = spec.label.clone().unwrap_or_else(|| format!("task-{}", counter));
            let subagent_id = SubAgentManager::generate_id(&label);
            let agent_timeout = spec.timeout.unwrap_or(300).min(3600);
            let read_only = spec.read_only.unwrap_or(false);

            let mut subagent_context = SubAgentContext::new(
                subagent_id.clone(),
                session_id,
                channel_id,
                label.clone(),
                spec.task.clone(),
                agent_timeout,
            )
            .with_model(spec.model.clone())
            .with_context(spec.context.clone())
            .with_thinking(spec.thinking.clone())
            .with_read_only(read_only)
            .with_agent_subtype(spec.agent_subtype.clone());

            // Propagate parent identity for depth tracking
            if let (Some(parent_id), Some(parent_depth)) =
                (&context.current_subagent_id, context.current_subagent_depth)
            {
                subagent_context =
                    subagent_context.with_parent_subagent(parent_id.clone(), parent_depth);
            }

            match manager.spawn(subagent_context).await {
                Ok(id) => {
                    log::info!(
                        "[SUBAGENTS] [{}/{}] Spawned '{}' (label: {})",
                        i + 1,
                        agents.len(),
                        id,
                        label
                    );
                    spawned_ids.push(id);
                    spawned_labels.push(label);
                }
                Err(e) => {
                    log::error!("[SUBAGENTS] Failed to spawn agent {}: {}", i, e);
                    // Continue spawning the rest, report this failure in results
                    spawned_ids.push(format!("FAILED_TO_SPAWN_{}", i));
                    spawned_labels.push(label);
                }
            }
        }

        // Phase 2: Poll all until terminal or overall timeout
        let start = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(overall_timeout);
        let mut last_progress = std::time::Instant::now();

        // Get broadcaster for progress events
        let broadcaster = context.broadcaster.as_ref();

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

            // Check all statuses
            let mut all_terminal = true;
            let mut status_summary: Vec<(String, String, String)> = Vec::new(); // (id, label, status)

            for (id, label) in spawned_ids.iter().zip(spawned_labels.iter()) {
                if id.starts_with("FAILED_TO_SPAWN_") {
                    status_summary.push((id.clone(), label.clone(), "spawn_failed".to_string()));
                    continue;
                }

                match manager.get_status(id) {
                    Ok(Some(status)) => {
                        let status_str = status.status.to_string();
                        if !status.status.is_terminal() {
                            all_terminal = false;
                        }
                        status_summary.push((id.clone(), label.clone(), status_str));
                    }
                    Ok(None) => {
                        status_summary.push((id.clone(), label.clone(), "not_found".to_string()));
                    }
                    Err(_) => {
                        all_terminal = false;
                        status_summary.push((id.clone(), label.clone(), "unknown".to_string()));
                    }
                }
            }

            // Broadcast progress every PROGRESS_INTERVAL_SECS
            if last_progress.elapsed() >= std::time::Duration::from_secs(PROGRESS_INTERVAL_SECS) {
                last_progress = std::time::Instant::now();
                let elapsed = start.elapsed().as_secs();

                // Build heartbeat info for each running agent
                let mut progress_details = Vec::new();
                for (id, label, status) in &status_summary {
                    let mut detail = json!({
                        "id": id,
                        "label": label,
                        "status": status,
                    });
                    // Add idle warning for running agents
                    if status == "running" {
                        if let Some(last_act) = manager.get_last_activity(id) {
                            let idle_secs = (chrono::Utc::now() - last_act).num_seconds();
                            detail["idle_secs"] = json!(idle_secs);
                            if idle_secs > IDLE_WARN_SECS {
                                detail["warning"] = json!(format!("idle for {}s", idle_secs));
                            }
                        }
                    }
                    progress_details.push(detail);
                }

                if let Some(bc) = broadcaster {
                    bc.broadcast(GatewayEvent::new(
                        "subagent.await_progress",
                        json!({
                            "channel_id": channel_id,
                            "elapsed_secs": elapsed,
                            "overall_timeout": overall_timeout,
                            "agents": progress_details,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                        }),
                    ));
                }

                log::debug!(
                    "[SUBAGENTS] Progress: {}/{}s elapsed, statuses: {:?}",
                    elapsed,
                    overall_timeout,
                    status_summary.iter().map(|(_, l, s)| format!("{}:{}", l, s)).collect::<Vec<_>>()
                );
            }

            if all_terminal {
                break;
            }

            if start.elapsed() > timeout_duration {
                log::warn!(
                    "[SUBAGENTS] Overall timeout reached ({}s), returning partial results",
                    overall_timeout
                );
                break;
            }
        }

        // Phase 3: Collect and return consolidated results
        self.build_consolidated_result(&spawned_ids, &spawned_labels, manager, start.elapsed())
    }

    /// Build the consolidated result report from all subagent outcomes
    fn build_consolidated_result(
        &self,
        ids: &[String],
        labels: &[String],
        manager: &Arc<SubAgentManager>,
        elapsed: std::time::Duration,
    ) -> ToolResult {
        let mut report = format!(
            "## Sub-agent Results ({} agents, {:.1}s elapsed)\n\n",
            ids.len(),
            elapsed.as_secs_f64()
        );

        let mut results_metadata = Vec::new();
        let mut all_succeeded = true;

        for (id, label) in ids.iter().zip(labels.iter()) {
            if id.starts_with("FAILED_TO_SPAWN_") {
                report.push_str(&format!("### {} — SPAWN FAILED\nFailed to spawn this sub-agent.\n\n", label));
                results_metadata.push(json!({
                    "id": id,
                    "label": label,
                    "status": "spawn_failed",
                }));
                all_succeeded = false;
                continue;
            }

            match manager.get_status(id) {
                Ok(Some(status)) => {
                    let status_str = status.status.to_string();
                    let status_emoji = match status.status {
                        SubAgentStatus::Completed => "OK",
                        SubAgentStatus::Failed => "FAILED",
                        SubAgentStatus::TimedOut => "TIMED OUT",
                        SubAgentStatus::Cancelled => "CANCELLED",
                        SubAgentStatus::Running => "STILL RUNNING",
                        SubAgentStatus::Pending => "PENDING",
                    };

                    report.push_str(&format!("### {} — {}\n", label, status_emoji));

                    if let Some(ref duration_end) = status.completed_at {
                        let dur = (*duration_end - status.started_at).num_seconds();
                        report.push_str(&format!("Duration: {}s\n", dur));
                    }

                    if let Some(ref result) = status.result {
                        let cleaned = strip_think_blocks(result);
                        let truncated = if cleaned.len() > 2000 {
                            format!("{}...\n[truncated, {} chars total]", &cleaned[..2000], cleaned.len())
                        } else {
                            cleaned
                        };
                        report.push_str(&format!("\n{}\n\n", truncated));
                    }

                    if let Some(ref error) = status.error {
                        report.push_str(&format!("\nError: {}\n\n", error));
                        all_succeeded = false;
                    }

                    if !status.status.is_terminal() {
                        all_succeeded = false;
                    }
                    if status.status == SubAgentStatus::Failed
                        || status.status == SubAgentStatus::TimedOut
                        || status.status == SubAgentStatus::Cancelled
                    {
                        all_succeeded = false;
                    }

                    results_metadata.push(json!({
                        "id": id,
                        "label": label,
                        "status": status_str,
                    }));
                }
                Ok(None) => {
                    report.push_str(&format!("### {} — NOT FOUND\nSub-agent '{}' not found in database.\n\n", label, id));
                    results_metadata.push(json!({
                        "id": id,
                        "label": label,
                        "status": "not_found",
                    }));
                    all_succeeded = false;
                }
                Err(e) => {
                    report.push_str(&format!("### {} — ERROR\nFailed to get status: {}\n\n", label, e));
                    results_metadata.push(json!({
                        "id": id,
                        "label": label,
                        "status": "error",
                        "error": e.to_string(),
                    }));
                    all_succeeded = false;
                }
            }
        }

        // Do NOT set task_fully_completed here — let the parent AI get one more turn
        // to read the subagent results and format a proper user-facing response.
        let metadata = json!({
            "count": ids.len(),
            "all_succeeded": all_succeeded,
            "elapsed_secs": elapsed.as_secs_f64(),
            "results": results_metadata,
        });

        if !all_succeeded {
            // Still return success (not error) — we have partial results
            // The report clearly indicates which agents failed
        }
        ToolResult::success(report).with_metadata(metadata)
    }
}

// ---------------------------------------------------------------------------
// SubagentStatusTool — check status / cancel running subagents
// ---------------------------------------------------------------------------

/// Tool for checking subagent status
pub struct SubagentStatusTool {
    definition: ToolDefinition,
}

impl SubagentStatusTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "The subagent ID to check status for. Omit to list all subagents."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "cancel".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description:
                    "If true and id is provided, cancel the running subagent."
                        .to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        SubagentStatusTool {
            definition: ToolDefinition {
                name: "subagent_status".to_string(),
                description:
                    "Check the status of a running or completed subagent, or list all subagents. Can also cancel running subagents."
                        .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::SubAgent,
                hidden: false,
            },
        }
    }
}

impl Default for SubagentStatusTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SubagentStatusParams {
    id: Option<String>,
    cancel: Option<bool>,
}

#[async_trait]
impl Tool for SubagentStatusTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SubagentStatusParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Require SubAgentManager
        let manager = match &context.subagent_manager {
            Some(m) => m,
            None => {
                return ToolResult::error(
                    "SubAgentManager not available. Sub-agent status requires an active session with a configured SubAgentManager."
                );
            }
        };

        if let Some(id) = params.id {
            // Check if cancel requested
            if params.cancel.unwrap_or(false) {
                match manager.cancel(&id) {
                    Ok(true) => {
                        return ToolResult::success(format!(
                            "Subagent '{}' cancellation requested.",
                            id
                        ));
                    }
                    Ok(false) => {
                        return ToolResult::error(format!(
                            "Subagent '{}' is not running or not found.",
                            id
                        ));
                    }
                    Err(e) => {
                        return ToolResult::error(format!(
                            "Failed to cancel subagent: {}",
                            e
                        ));
                    }
                }
            }

            // Get specific subagent status
            match manager.get_status(&id) {
                Ok(Some(status)) => {
                    let mut result = format!(
                        "## Subagent: {}\n\
                         Label: {}\n\
                         Status: {}\n\
                         Started: {}\n",
                        status.id,
                        status.label,
                        status.status,
                        status.started_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );

                    if let Some(completed) = status.completed_at {
                        result.push_str(&format!(
                            "Completed: {}\n\
                             Duration: {}s\n",
                            completed.format("%Y-%m-%d %H:%M:%S UTC"),
                            (completed - status.started_at).num_seconds()
                        ));
                    }

                    result.push_str(&format!("\nTask: {}\n", status.task));

                    if let Some(ref res) = status.result {
                        result.push_str(&format!("\n## Result:\n{}\n", res));
                    }

                    if let Some(ref err) = status.error {
                        result.push_str(&format!("\n## Error:\n{}\n", err));
                    }

                    ToolResult::success(result).with_metadata(json!({
                        "id": status.id,
                        "status": status.status.to_string(),
                        "label": status.label
                    }))
                }
                Ok(None) => {
                    ToolResult::error(format!("Subagent '{}' not found", id))
                }
                Err(e) => {
                    ToolResult::error(format!(
                        "Failed to get subagent status: {}",
                        e
                    ))
                }
            }
        } else {
            // List all subagents for this channel
            let channel_id = context.channel_id.unwrap_or(0);
            match manager.list_by_channel(channel_id) {
                Ok(agents) => {
                    if agents.is_empty() {
                        return ToolResult::success("No subagents found.");
                    }

                    let mut result = format!("## Subagents ({} total)\n\n", agents.len());

                    for status in &agents {
                        result.push_str(&format!(
                            "- **{}** ({}): {} - {}\n",
                            status.id,
                            status.label,
                            status.status,
                            if status.task.len() > 50 {
                                format!("{}...", &status.task[..50])
                            } else {
                                status.task.clone()
                            }
                        ));
                    }

                    ToolResult::success(result).with_metadata(json!({
                        "count": agents.len(),
                        "subagents": agents.iter().map(|s| json!({
                            "id": s.id,
                            "label": s.label,
                            "status": s.status.to_string()
                        })).collect::<Vec<_>>()
                    }))
                }
                Err(e) => {
                    ToolResult::error(format!(
                        "Failed to list subagents: {}",
                        e
                    ))
                }
            }
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_subagents_definition() {
        let tool = SpawnSubagentsTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "spawn_subagents");
        assert_eq!(def.group, ToolGroup::SubAgent);
        assert!(def.input_schema.required.contains(&"agents".to_string()));
    }

    #[test]
    fn test_subagent_status_definition() {
        let tool = SubagentStatusTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "subagent_status");
        assert_eq!(def.group, ToolGroup::SubAgent);
        assert!(def.input_schema.required.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_subagents_empty() {
        let tool = SpawnSubagentsTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "agents": []
                }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(result.content.contains("No agents"));
    }

    #[tokio::test]
    async fn test_spawn_subagents_no_manager_returns_error() {
        let tool = SpawnSubagentsTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "agents": [
                        { "task": "Test task 1", "label": "test1" },
                        { "task": "Test task 2", "label": "test2" }
                    ]
                }),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.content.contains("SubAgentManager not available"));
    }
}
