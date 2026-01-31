//! Sub-agent tools for spawning and monitoring background agent instances
//!
//! This module provides two tools:
//! - `subagent`: Spawn a new sub-agent to work on a task
//! - `subagent_status`: Check the status of sub-agents

use crate::ai::multi_agent::{SubAgentContext, SubAgentManager, SubAgentStatus};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Counter for generating unique subagent IDs (fallback when no manager)
static SUBAGENT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Legacy status of a running subagent (for backwards compatibility)
#[derive(Debug, Clone)]
pub struct SubagentStatus {
    pub id: String,
    pub label: String,
    pub task: String,
    pub status: String, // "running", "completed", "failed"
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Global registry of running subagents (fallback when no manager)
lazy_static::lazy_static! {
    static ref SUBAGENT_REGISTRY: Arc<RwLock<HashMap<String, SubagentStatus>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

/// Tool for spawning background agent instances (subagents)
pub struct SubagentTool {
    definition: ToolDefinition,
}

impl SubagentTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "task".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The task or prompt for the subagent to work on. Be specific and detailed.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "label".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "A short label to identify this subagent (e.g., 'research', 'code-review'). Used for tracking and referencing.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "model".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional model override (e.g., 'claude-3-5-sonnet', 'gpt-4'). Uses default model if not specified.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "thinking".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional thinking level for Claude models (off, minimal, low, medium, high, xhigh).".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "off".to_string(),
                    "minimal".to_string(),
                    "low".to_string(),
                    "medium".to_string(),
                    "high".to_string(),
                    "xhigh".to_string(),
                ]),
            },
        );

        properties.insert(
            "timeout".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Timeout in seconds for the subagent task (default: 300, max: 3600).".to_string(),
                default: Some(json!(300)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "wait".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, wait for the subagent to complete before returning. If false (default), run in background.".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "context".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional additional context or data to pass to the subagent.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        SubagentTool {
            definition: ToolDefinition {
                name: "subagent".to_string(),
                description: "Spawn a background agent instance to work on a task autonomously. Useful for parallel task execution, long-running operations, or delegating subtasks. The subagent runs independently and can use tools.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["task".to_string()],
                },
                group: ToolGroup::System,
            },
        }
    }

    /// Get status of a subagent by ID (legacy method)
    pub async fn get_status(id: &str) -> Option<SubagentStatus> {
        SUBAGENT_REGISTRY.read().await.get(id).cloned()
    }

    /// List all subagents (legacy method)
    pub async fn list_all() -> Vec<SubagentStatus> {
        SUBAGENT_REGISTRY.read().await.values().cloned().collect()
    }

    /// Update subagent status (legacy method)
    async fn update_status(id: &str, status: &str, result: Option<String>, error: Option<String>) {
        let mut registry = SUBAGENT_REGISTRY.write().await;
        if let Some(entry) = registry.get_mut(id) {
            entry.status = status.to_string();
            entry.result = result;
            entry.error = error;
            if status == "completed" || status == "failed" {
                entry.completed_at = Some(chrono::Utc::now());
            }
        }
    }

    /// Get SubAgentManager from context if available
    /// Note: Currently returns None as we use the `subagent_manager_ptr` approach instead.
    #[allow(dead_code)]
    fn get_manager(_context: &ToolContext) -> Option<Arc<SubAgentManager>> {
        // The manager is stored as a pointer address in `subagent_manager_ptr`
        // This method is kept for potential future use with Arc-based storage
        None
    }
}

impl Default for SubagentTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SubagentParams {
    task: String,
    label: Option<String>,
    model: Option<String>,
    thinking: Option<String>,
    timeout: Option<u64>,
    wait: Option<bool>,
    context: Option<String>,
}

#[async_trait]
impl Tool for SubagentTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SubagentParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Generate unique subagent ID
        let counter = SUBAGENT_COUNTER.fetch_add(1, Ordering::SeqCst);
        let label = params
            .label
            .clone()
            .unwrap_or_else(|| format!("task-{}", counter));
        let subagent_id = SubAgentManager::generate_id(&label);

        let timeout_secs = params.timeout.unwrap_or(300).min(3600);
        let wait = params.wait.unwrap_or(false);

        log::info!(
            "[SUBAGENT] Spawning subagent '{}' with task: {}",
            subagent_id,
            if params.task.len() > 100 {
                &params.task[..100]
            } else {
                &params.task
            }
        );

        // Check if we have access to the SubAgentManager via the context
        // Also need valid session and channel IDs for real execution
        let has_valid_context = context.session_id.map(|id| id > 0).unwrap_or(false)
            && context.channel_id.map(|id| id > 0).unwrap_or(false);

        if let Some(manager) = &context.subagent_manager {
            if has_valid_context {
                // Real execution via SubAgentManager - this will actually spawn an AI agent
                log::info!("[SUBAGENT] ✓ Using SubAgentManager for real AI execution");

                let session_id = context.session_id.unwrap();
                let channel_id = context.channel_id.unwrap();

                let subagent_context = SubAgentContext::new(
                    subagent_id.clone(),
                    session_id,
                    channel_id,
                    label.clone(),
                    params.task.clone(),
                    timeout_secs,
                )
                .with_model(params.model.clone())
                .with_context(params.context.clone())
                .with_thinking(params.thinking.clone());

                // Spawn the sub-agent
                match manager.spawn(subagent_context).await {
                    Ok(id) => {
                        if wait {
                            // Poll for completion
                            let start = std::time::Instant::now();
                            let timeout_duration = std::time::Duration::from_secs(timeout_secs);

                            loop {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                                match manager.get_status(&id) {
                                    Ok(Some(status)) => {
                                        if status.status.is_terminal() {
                                            match status.status {
                                                SubAgentStatus::Completed => {
                                                    return ToolResult::success(
                                                        status
                                                            .result
                                                            .unwrap_or_else(|| "Task completed".to_string()),
                                                    )
                                                    .with_metadata(json!({
                                                        "subagent_id": id,
                                                        "label": label,
                                                        "status": "completed",
                                                        "waited": true
                                                    }));
                                                }
                                                SubAgentStatus::Failed => {
                                                    return ToolResult::error(format!(
                                                        "Subagent failed: {}",
                                                        status.error.unwrap_or_else(|| "Unknown error".to_string())
                                                    ));
                                                }
                                                SubAgentStatus::TimedOut => {
                                                    return ToolResult::error(format!(
                                                        "Subagent '{}' timed out after {} seconds",
                                                        id, timeout_secs
                                                    ));
                                                }
                                                SubAgentStatus::Cancelled => {
                                                    return ToolResult::error(format!(
                                                        "Subagent '{}' was cancelled",
                                                        id
                                                    ));
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        return ToolResult::error(format!(
                                            "Subagent '{}' not found",
                                            id
                                        ));
                                    }
                                    Err(e) => {
                                        return ToolResult::error(format!(
                                            "Failed to get subagent status: {}",
                                            e
                                        ));
                                    }
                                }

                                if start.elapsed() > timeout_duration {
                                    return ToolResult::error(format!(
                                        "Subagent '{}' timed out after {} seconds",
                                        id, timeout_secs
                                    ));
                                }
                            }
                        } else {
                            // Return immediately
                            return ToolResult::success(format!(
                                "Subagent '{}' spawned successfully and running in background.\n\
                                 Label: {}\n\
                                 Task: {}\n\
                                 Timeout: {}s\n\
                                 \n\
                                 Use `subagent_status` with id '{}' to check progress.",
                                id,
                                label,
                                if params.task.len() > 100 {
                                    format!("{}...", &params.task[..100])
                                } else {
                                    params.task.clone()
                                },
                                timeout_secs,
                                id
                            ))
                            .with_metadata(json!({
                                "subagent_id": id,
                                "label": label,
                                "status": "running",
                                "waited": false,
                                "timeout": timeout_secs
                            }));
                        }
                    }
                    Err(e) => {
                        return ToolResult::error(format!("Failed to spawn subagent: {}", e));
                    }
                }
            } else {
                // Have manager but no valid channel context - log and fall through to legacy mode
                log::warn!(
                    "[SUBAGENT] SubAgentManager available but no valid channel context (session_id: {:?}, channel_id: {:?}). \
                     Falling back to legacy mode.",
                    context.session_id,
                    context.channel_id
                );
            }
        }

        // Fallback: Legacy in-memory approach when no manager is available
        // or when channel context is missing
        // WARNING: This path does NOT actually execute AI or make API calls!
        log::warn!(
            "[SUBAGENT] Using legacy placeholder mode (NO REAL AI EXECUTION). \
             For real subagent support, ensure dispatcher is configured with SubAgentManager and valid channel context."
        );

        // Build the full task prompt
        let full_task = if let Some(ref ctx) = params.context {
            format!("{}\n\n## Additional Context:\n{}", params.task, ctx)
        } else {
            params.task.clone()
        };

        // Register the subagent in legacy registry
        {
            let mut registry = SUBAGENT_REGISTRY.write().await;
            registry.insert(
                subagent_id.clone(),
                SubagentStatus {
                    id: subagent_id.clone(),
                    label: label.clone(),
                    task: params.task.clone(),
                    status: "running".to_string(),
                    started_at: chrono::Utc::now(),
                    completed_at: None,
                    result: None,
                    error: None,
                },
            );
        }

        // Clone values for the async task
        let subagent_id_clone = subagent_id.clone();
        let model_override = params.model.clone();
        let thinking_level = params.thinking.clone();
        let channel_id = context.channel_id;
        let channel_type = context.channel_type.clone();

        // Spawn the subagent task (legacy simulation)
        let task_handle = tokio::spawn(async move {
            log::info!(
                "[SUBAGENT] Legacy execution for '{}'",
                subagent_id_clone
            );

            // Simulate work
            let start = std::time::Instant::now();

            // Placeholder result
            let result = format!(
                "Subagent '{}' processed task.\n\
                 Model: {}\n\
                 Thinking: {}\n\
                 Channel: {:?} ({:?})\n\
                 \n\
                 Task summary: {}\n\
                 \n\
                 [Note: This is a placeholder response. For full AI execution, \
                 ensure SubAgentManager is properly configured.]",
                subagent_id_clone,
                model_override.as_deref().unwrap_or("default"),
                thinking_level.as_deref().unwrap_or("default"),
                channel_id,
                channel_type,
                if full_task.len() > 200 {
                    &full_task[..200]
                } else {
                    &full_task
                }
            );

            let duration = start.elapsed();
            log::info!(
                "[SUBAGENT] Legacy execution '{}' completed in {:?}",
                subagent_id_clone,
                duration
            );

            // Update status to completed
            Self::update_status(&subagent_id_clone, "completed", Some(result.clone()), None).await;

            result
        });

        if wait {
            // Wait for completion with timeout
            match tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                task_handle,
            )
            .await
            {
                Ok(Ok(result)) => ToolResult::success(result).with_metadata(json!({
                    "subagent_id": subagent_id,
                    "label": label,
                    "status": "completed",
                    "waited": true,
                    "legacy": true
                })),
                Ok(Err(e)) => {
                    Self::update_status(&subagent_id, "failed", None, Some(e.to_string())).await;
                    ToolResult::error(format!("Subagent task failed: {}", e))
                }
                Err(_) => {
                    Self::update_status(
                        &subagent_id,
                        "failed",
                        None,
                        Some(format!("Timeout after {}s", timeout_secs)),
                    )
                    .await;
                    ToolResult::error(format!(
                        "Subagent '{}' timed out after {} seconds",
                        subagent_id, timeout_secs
                    ))
                }
            }
        } else {
            // Return immediately, task runs in background
            ToolResult::success(format!(
                "Subagent '{}' spawned successfully and running in background.\n\
                 Label: {}\n\
                 Task: {}\n\
                 Timeout: {}s\n\
                 \n\
                 Use `subagent_status` with id '{}' to check progress.",
                subagent_id,
                label,
                if params.task.len() > 100 {
                    format!("{}...", &params.task[..100])
                } else {
                    params.task.clone()
                },
                timeout_secs,
                subagent_id
            ))
            .with_metadata(json!({
                "subagent_id": subagent_id,
                "label": label,
                "status": "running",
                "waited": false,
                "timeout": timeout_secs,
                "legacy": true
            }))
        }
    }
}

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
                group: ToolGroup::System,
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

        // Check if we have SubAgentManager
        if let Some(manager) = &context.subagent_manager {
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

                            return ToolResult::success(result).with_metadata(json!({
                                "id": status.id,
                                "status": status.status.to_string(),
                                "label": status.label
                            }));
                        }
                        Ok(None) => {
                            return ToolResult::error(format!("Subagent '{}' not found", id));
                        }
                        Err(e) => {
                            return ToolResult::error(format!(
                                "Failed to get subagent status: {}",
                                e
                            ));
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

                            return ToolResult::success(result).with_metadata(json!({
                                "count": agents.len(),
                                "subagents": agents.iter().map(|s| json!({
                                    "id": s.id,
                                    "label": s.label,
                                    "status": s.status.to_string()
                                })).collect::<Vec<_>>()
                            }));
                        }
                        Err(e) => {
                            return ToolResult::error(format!(
                                "Failed to list subagents: {}",
                                e
                            ));
                        }
                    }
                }
        }

        // Fallback: Legacy in-memory approach
        if let Some(id) = params.id {
            // Get specific subagent status
            match SubagentTool::get_status(&id).await {
                Some(status) => {
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
                            "Completed: {}\n",
                            completed.format("%Y-%m-%d %H:%M:%S UTC")
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
                        "status": status.status,
                        "label": status.label,
                        "legacy": true
                    }))
                }
                None => ToolResult::error(format!("Subagent '{}' not found", id)),
            }
        } else {
            // List all subagents
            let all = SubagentTool::list_all().await;

            if all.is_empty() {
                return ToolResult::success("No subagents found.");
            }

            let mut result = format!("## Subagents ({} total)\n\n", all.len());

            for status in &all {
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
                "count": all.len(),
                "subagents": all.iter().map(|s| json!({
                    "id": s.id,
                    "label": s.label,
                    "status": s.status
                })).collect::<Vec<_>>(),
                "legacy": true
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_definition() {
        let tool = SubagentTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "subagent");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"task".to_string()));
    }

    #[test]
    fn test_subagent_status_definition() {
        let tool = SubagentStatusTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "subagent_status");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_subagent_legacy() {
        let tool = SubagentTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "task": "Test task",
                    "label": "test",
                    "wait": true
                }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(result.content.contains("subagent"));
    }
}
