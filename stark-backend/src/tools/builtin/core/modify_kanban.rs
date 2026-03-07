//! Workstream tool — unified kanban + scheduling for the AI agent
//!
//! Allows the agent to manage kanban board items AND create scheduled jobs:
//! - List items (optionally filtered by status)
//! - Pick the next ready task (atomically moves to in_progress)
//! - Update item status
//! - Add notes to items
//! - Create new items (auto-executed by scheduler when "ready")
//! - Schedule one-time or recurring cron jobs

use crate::db::tables::kanban::{CreateKanbanItemRequest, UpdateKanbanItemRequest};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct WorkstreamTool {
    definition: ToolDefinition,
}

impl WorkstreamTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The action to perform: 'list' (show kanban items), 'pick_task' (grab highest-priority ready task), 'update_status' (move item between columns), 'add_note' (append to result), 'create' (new kanban item), 'schedule' (create a cron job), 'list_schedules' (show all cron jobs), 'get_schedule' (details + run history), 'update_schedule' (modify a cron job), 'pause_schedule', 'resume_schedule', 'delete_schedule'".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list".to_string(),
                    "pick_task".to_string(),
                    "update_status".to_string(),
                    "add_note".to_string(),
                    "create".to_string(),
                    "schedule".to_string(),
                    "list_schedules".to_string(),
                    "get_schedule".to_string(),
                    "update_schedule".to_string(),
                    "pause_schedule".to_string(),
                    "resume_schedule".to_string(),
                    "delete_schedule".to_string(),
                ]),
            },
        );

        properties.insert(
            "status".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Filter for 'list' action, or new status for 'update_status'. Values: 'ready', 'in_progress', 'complete'".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "ready".to_string(),
                    "in_progress".to_string(),
                    "complete".to_string(),
                ]),
            },
        );

        properties.insert(
            "item_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Kanban item ID (required for 'update_status' and 'add_note')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "title".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Title for 'create' (kanban item) or 'schedule' (job name)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "description".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Description for 'create', note text for 'add_note', or job description for 'schedule'".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "priority".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Priority for 'create': 0=normal, 1=high, 2=urgent".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "message".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The prompt/instruction the agent should execute when the scheduled job runs (required for 'schedule')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "schedule_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Schedule type (required for 'schedule'): 'at' (one-time at ISO datetime), 'every' (recurring interval in ms), 'cron' (cron expression)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "at".to_string(),
                    "every".to_string(),
                    "cron".to_string(),
                ]),
            },
        );

        properties.insert(
            "schedule_value".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Schedule value (required for 'schedule'): ISO datetime for 'at', milliseconds for 'every', cron expression for 'cron'".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "job_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Cron job UUID (required for get_schedule, update_schedule, pause_schedule, resume_schedule, delete_schedule)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "delete_after_run".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Auto-delete the job after it runs once (optional for 'schedule'; defaults to true for 'at', false for 'every'/'cron')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        WorkstreamTool {
            definition: ToolDefinition {
                name: "workstream".to_string(),
                description: "Manage your workstream: list/create/pick kanban tasks (auto-executed by scheduler), and fully manage cron schedules (create, list, update, pause, resume, delete). Use this to work through tasks autonomously and schedule future work.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }
}

impl Default for WorkstreamTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct WorkstreamParams {
    action: String,
    status: Option<String>,
    item_id: Option<i64>,
    title: Option<String>,
    description: Option<String>,
    priority: Option<i32>,
    message: Option<String>,
    schedule_type: Option<String>,
    schedule_value: Option<String>,
    delete_after_run: Option<bool>,
    job_id: Option<String>,
}

#[async_trait]
impl Tool for WorkstreamTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: WorkstreamParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        match params.action.as_str() {
            "list" => {
                let items = if let Some(ref status) = params.status {
                    match db.list_kanban_items_by_status(status) {
                        Ok(items) => items,
                        Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                    }
                } else {
                    match db.list_kanban_items() {
                        Ok(items) => items,
                        Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                    }
                };

                if items.is_empty() {
                    let filter_msg = params.status.as_deref().map(|s| format!(" with status '{}'", s)).unwrap_or_default();
                    return ToolResult::success(format!("No kanban items found{}.", filter_msg));
                }

                let mut output = String::new();
                for item in &items {
                    let priority_label = match item.priority {
                        2 => " [URGENT]",
                        1 => " [HIGH]",
                        _ => "",
                    };
                    output.push_str(&format!(
                        "#{} [{}]{} — {}\n",
                        item.id, item.status, priority_label, item.title
                    ));
                    if !item.description.is_empty() {
                        output.push_str(&format!("  Description: {}\n", item.description));
                    }
                    if let Some(ref result) = item.result {
                        output.push_str(&format!("  Notes: {}\n", result));
                    }
                }

                ToolResult::success(output)
                    .with_metadata(json!({ "count": items.len() }))
            }

            "pick_task" => {
                match db.pick_next_kanban_task() {
                    Ok(Some(item)) => {
                        // Set session_id if available
                        if let Some(session_id) = context.session_id {
                            let _ = db.update_kanban_item(item.id, &UpdateKanbanItemRequest {
                                session_id: Some(session_id),
                                ..Default::default()
                            });
                        }

                        let priority_label = match item.priority {
                            2 => "URGENT",
                            1 => "HIGH",
                            _ => "NORMAL",
                        };

                        ToolResult::success(format!(
                            "Picked task #{} (priority: {}): {}\n\nDescription: {}\n\nThis task is now IN PROGRESS. Work on it and update status to 'complete' when done.",
                            item.id, priority_label, item.title, item.description
                        )).with_metadata(json!({
                            "item_id": item.id,
                            "title": item.title,
                            "priority": item.priority,
                        }))
                    }
                    Ok(None) => ToolResult::success("No ready tasks on the kanban board. All tasks are either in progress or complete."),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "update_status" => {
                let item_id = match params.item_id {
                    Some(id) => id,
                    None => return ToolResult::error("'item_id' is required for 'update_status' action"),
                };
                let status = match params.status {
                    Some(s) => s,
                    None => return ToolResult::error("'status' is required for 'update_status' action"),
                };

                if !["ready", "in_progress", "complete"].contains(&status.as_str()) {
                    return ToolResult::error("Invalid status. Must be 'ready', 'in_progress', or 'complete'");
                }

                match db.update_kanban_item(item_id, &UpdateKanbanItemRequest {
                    status: Some(status.clone()),
                    ..Default::default()
                }) {
                    Ok(Some(item)) => {
                        ToolResult::success(format!(
                            "Task #{} '{}' moved to status: {}", item.id, item.title, status
                        ))
                    }
                    Ok(None) => ToolResult::error(format!("Kanban item #{} not found", item_id)),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "add_note" => {
                let item_id = match params.item_id {
                    Some(id) => id,
                    None => return ToolResult::error("'item_id' is required for 'add_note' action"),
                };
                let note = match params.description {
                    Some(d) => d,
                    None => return ToolResult::error("'description' is required for 'add_note' action (the note text)"),
                };

                // Get existing item to append note
                let existing = match db.get_kanban_item(item_id) {
                    Ok(Some(item)) => item,
                    Ok(None) => return ToolResult::error(format!("Kanban item #{} not found", item_id)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                let new_result = match existing.result {
                    Some(ref existing_notes) => format!("{}\n---\n{}", existing_notes, note),
                    None => note.clone(),
                };

                match db.update_kanban_item(item_id, &UpdateKanbanItemRequest {
                    result: Some(new_result),
                    ..Default::default()
                }) {
                    Ok(Some(_)) => ToolResult::success(format!("Note added to task #{}", item_id)),
                    Ok(None) => ToolResult::error(format!("Kanban item #{} not found", item_id)),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "create" => {
                let title = match params.title {
                    Some(t) => t,
                    None => return ToolResult::error("'title' is required for 'create' action"),
                };

                let request = CreateKanbanItemRequest {
                    title,
                    description: params.description,
                    priority: params.priority,
                };

                match db.create_kanban_item(&request) {
                    Ok(item) => {
                        ToolResult::success(format!(
                            "Created kanban item #{}: '{}'", item.id, item.title
                        )).with_metadata(json!({
                            "item_id": item.id,
                            "title": item.title,
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "schedule" => {
                let title = match params.title {
                    Some(t) => t,
                    None => return ToolResult::error("'title' is required for 'schedule' action"),
                };
                let message = match params.message {
                    Some(m) => m,
                    None => return ToolResult::error("'message' is required for 'schedule' action (the agent instruction)"),
                };
                let schedule_type = match params.schedule_type {
                    Some(st) => st,
                    None => return ToolResult::error("'schedule_type' is required for 'schedule' action ('at', 'every', or 'cron')"),
                };
                let schedule_value = match params.schedule_value {
                    Some(sv) => sv,
                    None => return ToolResult::error("'schedule_value' is required for 'schedule' action"),
                };

                if !["at", "every", "cron"].contains(&schedule_type.as_str()) {
                    return ToolResult::error("Invalid schedule_type. Must be 'at', 'every', or 'cron'");
                }

                let delete_after_run = params.delete_after_run.unwrap_or(schedule_type == "at");

                match db.create_cron_job(
                    &title,
                    params.description.as_deref(),
                    &schedule_type,
                    &schedule_value,
                    None,           // timezone
                    "isolated",     // session_mode
                    Some(&message),
                    None,           // system_event
                    context.channel_id, // channel_id
                    None,           // deliver_to
                    false,          // deliver
                    None,           // model_override
                    None,           // thinking_level
                    None,           // timeout_seconds
                    delete_after_run,
                ) {
                    Ok(job) => {
                        let type_label = match schedule_type.as_str() {
                            "at" => format!("one-time at {}", schedule_value),
                            "every" => format!("every {}ms", schedule_value),
                            "cron" => format!("cron: {}", schedule_value),
                            _ => schedule_value.clone(),
                        };
                        ToolResult::success(format!(
                            "Scheduled job '{}' (id: {}, {}). delete_after_run={}",
                            job.name, job.job_id, type_label, delete_after_run
                        )).with_metadata(json!({
                            "job_id": job.job_id,
                            "name": job.name,
                            "schedule_type": schedule_type,
                            "schedule_value": schedule_value,
                            "delete_after_run": delete_after_run,
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "list_schedules" => {
                let jobs = match db.list_cron_jobs() {
                    Ok(jobs) => jobs,
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                let jobs: Vec<_> = if let Some(ref status) = params.status {
                    jobs.into_iter().filter(|j| j.status == *status).collect()
                } else {
                    jobs
                };

                if jobs.is_empty() {
                    let filter_msg = params.status.as_deref().map(|s| format!(" with status '{}'", s)).unwrap_or_default();
                    return ToolResult::success(format!("No cron jobs found{}.", filter_msg));
                }

                let mut output = format!("{} cron job(s):\n\n", jobs.len());
                for job in &jobs {
                    let schedule_label = match job.schedule_type.as_str() {
                        "at" => format!("once at {}", job.schedule_value),
                        "every" => format!("every {}ms", job.schedule_value),
                        "cron" => format!("cron: {}", job.schedule_value),
                        _ => job.schedule_value.clone(),
                    };
                    output.push_str(&format!(
                        "- [{}] {} — \"{}\" ({})\n  runs: {}, errors: {}, next: {}\n",
                        job.status,
                        job.job_id,
                        job.name,
                        schedule_label,
                        job.run_count,
                        job.error_count,
                        job.next_run_at.as_deref().unwrap_or("none"),
                    ));
                }

                ToolResult::success(output)
                    .with_metadata(json!({ "count": jobs.len() }))
            }

            "get_schedule" => {
                let job_id = match params.job_id {
                    Some(id) => id,
                    None => return ToolResult::error("'job_id' (UUID) is required for 'get_schedule' action"),
                };

                let job = match db.get_cron_job_by_job_id(&job_id) {
                    Ok(Some(job)) => job,
                    Ok(None) => return ToolResult::error(format!("Cron job '{}' not found", job_id)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                let schedule_label = match job.schedule_type.as_str() {
                    "at" => format!("once at {}", job.schedule_value),
                    "every" => format!("every {}ms", job.schedule_value),
                    "cron" => format!("cron: {}", job.schedule_value),
                    _ => job.schedule_value.clone(),
                };

                let mut output = format!(
                    "Job: {} ({})\nStatus: {}\nSchedule: {}\nMessage: {}\nSession mode: {}\nRuns: {}, Errors: {}\nLast run: {}\nNext run: {}\nDelete after run: {}\nCreated: {}\n",
                    job.name,
                    job.job_id,
                    job.status,
                    schedule_label,
                    job.message.as_deref().unwrap_or("(none)"),
                    job.session_mode,
                    job.run_count,
                    job.error_count,
                    job.last_run_at.as_deref().unwrap_or("never"),
                    job.next_run_at.as_deref().unwrap_or("none"),
                    job.delete_after_run,
                    job.created_at,
                );

                if let Some(ref err) = job.last_error {
                    output.push_str(&format!("Last error: {}\n", err));
                }

                // Fetch recent runs
                let runs = db.get_cron_job_runs(job.id, 5).unwrap_or_default();
                if !runs.is_empty() {
                    output.push_str("\nRecent runs:\n");
                    for run in &runs {
                        let status = if run.success { "OK" } else { "FAIL" };
                        let duration = run.duration_ms.map(|d| format!("{}ms", d)).unwrap_or_default();
                        output.push_str(&format!(
                            "  [{}] {} {} {}\n",
                            status,
                            run.started_at,
                            duration,
                            run.error.as_deref().unwrap_or(""),
                        ));
                    }
                }

                ToolResult::success(output)
                    .with_metadata(json!({
                        "job_id": job.job_id,
                        "status": job.status,
                        "run_count": job.run_count,
                        "error_count": job.error_count,
                    }))
            }

            "update_schedule" => {
                let job_id_str = match params.job_id {
                    Some(id) => id,
                    None => return ToolResult::error("'job_id' (UUID) is required for 'update_schedule' action"),
                };

                let job = match db.get_cron_job_by_job_id(&job_id_str) {
                    Ok(Some(job)) => job,
                    Ok(None) => return ToolResult::error(format!("Cron job '{}' not found", job_id_str)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                if params.title.is_none() && params.description.is_none() && params.message.is_none()
                    && params.schedule_type.is_none() && params.schedule_value.is_none()
                    && params.delete_after_run.is_none()
                {
                    return ToolResult::error("At least one field to update is required: title, description, message, schedule_type, schedule_value, delete_after_run");
                }

                match db.update_cron_job(
                    job.id,
                    params.title.as_deref(),
                    params.description.as_deref(),
                    params.schedule_type.as_deref(),
                    params.schedule_value.as_deref(),
                    None, // timezone
                    None, // session_mode
                    params.message.as_deref(),
                    None, // system_event
                    None, // channel_id
                    None, // deliver_to
                    None, // deliver
                    None, // model_override
                    None, // thinking_level
                    None, // timeout_seconds
                    params.delete_after_run,
                    None, // status
                ) {
                    Ok(updated) => {
                        ToolResult::success(format!(
                            "Updated cron job '{}' ({})", updated.name, updated.job_id
                        )).with_metadata(json!({
                            "job_id": updated.job_id,
                            "name": updated.name,
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "pause_schedule" => {
                let job_id_str = match params.job_id {
                    Some(id) => id,
                    None => return ToolResult::error("'job_id' (UUID) is required for 'pause_schedule' action"),
                };

                let job = match db.get_cron_job_by_job_id(&job_id_str) {
                    Ok(Some(job)) => job,
                    Ok(None) => return ToolResult::error(format!("Cron job '{}' not found", job_id_str)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                if job.status == "paused" {
                    return ToolResult::success(format!("Job '{}' is already paused", job.name));
                }

                match db.update_cron_job(
                    job.id,
                    None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                    Some("paused"),
                ) {
                    Ok(updated) => {
                        ToolResult::success(format!(
                            "Paused cron job '{}' ({})", updated.name, updated.job_id
                        ))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "resume_schedule" => {
                let job_id_str = match params.job_id {
                    Some(id) => id,
                    None => return ToolResult::error("'job_id' (UUID) is required for 'resume_schedule' action"),
                };

                let job = match db.get_cron_job_by_job_id(&job_id_str) {
                    Ok(Some(job)) => job,
                    Ok(None) => return ToolResult::error(format!("Cron job '{}' not found", job_id_str)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                if job.status == "active" {
                    return ToolResult::success(format!("Job '{}' is already active", job.name));
                }

                match db.update_cron_job(
                    job.id,
                    None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                    Some("active"),
                ) {
                    Ok(updated) => {
                        ToolResult::success(format!(
                            "Resumed cron job '{}' ({})", updated.name, updated.job_id
                        ))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "delete_schedule" => {
                let job_id_str = match params.job_id {
                    Some(id) => id,
                    None => return ToolResult::error("'job_id' (UUID) is required for 'delete_schedule' action"),
                };

                let job = match db.get_cron_job_by_job_id(&job_id_str) {
                    Ok(Some(job)) => job,
                    Ok(None) => return ToolResult::error(format!("Cron job '{}' not found", job_id_str)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                };

                match db.delete_cron_job(job.id) {
                    Ok(true) => {
                        ToolResult::success(format!(
                            "Deleted cron job '{}' ({})", job.name, job.job_id
                        ))
                    }
                    Ok(false) => ToolResult::error(format!("Failed to delete cron job '{}'", job_id_str)),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: list, pick_task, update_status, add_note, create, schedule, list_schedules, get_schedule, update_schedule, pause_schedule, resume_schedule, delete_schedule",
                params.action
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = WorkstreamTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "workstream");
        assert_eq!(def.group, ToolGroup::Development);
    }
}
