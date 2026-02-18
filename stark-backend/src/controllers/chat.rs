use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::channels::NormalizedMessage;
use crate::models::SessionScope;
use crate::AppState;

/// Web channel ID - a reserved ID for web-based chat
/// This is used to identify messages from the web frontend
const WEB_CHANNEL_ID: i64 = 0;
const WEB_CHANNEL_TYPE: &str = "web";

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    /// Optional user identifier for the web session
    #[serde(default)]
    pub user_id: Option<String>,
    /// Currently selected network from the UI (e.g., "base", "polygon", "mainnet")
    #[serde(default)]
    pub network: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Session ID for persistent conversations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
}

#[derive(Serialize)]
pub struct StopResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ExecutionStatusResponse {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
}

/// Request to cancel a specific subagent
#[derive(Debug, Deserialize)]
pub struct CancelSubagentRequest {
    pub subagent_id: String,
}

/// Response for subagent operations
#[derive(Serialize)]
pub struct SubagentResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Info about a running subagent
#[derive(Serialize)]
pub struct SubagentInfo {
    pub id: String,
    pub label: String,
    pub task: String,
    pub status: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    pub parent_session_id: i64,
}

/// Query parameters for listing subagents
#[derive(Debug, Deserialize)]
pub struct ListSubagentsQuery {
    pub session_id: Option<i64>,
}

/// Response listing subagents
#[derive(Serialize)]
pub struct SubagentListResponse {
    pub success: bool,
    pub subagents: Vec<SubagentInfo>,
}

/// Response for task deletion
#[derive(Serialize)]
pub struct DeleteTaskResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether the deleted task was the currently active one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_current_task: Option<bool>,
}

/// Task info for API response
#[derive(Serialize)]
pub struct PlannerTaskInfo {
    pub id: u32,
    pub description: String,
    pub status: String,
}

/// Response for getting planner tasks
#[derive(Serialize)]
pub struct GetPlannerTasksResponse {
    pub success: bool,
    pub tasks: Vec<PlannerTaskInfo>,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/api/chat").route(web::post().to(chat)))
        .service(web::resource("/api/chat/stop").route(web::post().to(stop_execution)))
        .service(web::resource("/api/chat/execution-status").route(web::get().to(get_execution_status)))
        .service(web::resource("/api/chat/subagents").route(web::get().to(list_subagents)))
        .service(web::resource("/api/chat/subagents/cancel").route(web::post().to(cancel_subagent)))
        // Task management for planner tasks
        .service(web::resource("/api/chat/tasks").route(web::get().to(get_planner_tasks)))
        .service(web::resource("/api/chat/tasks/{task_id}").route(web::delete().to(delete_task)))
        // Session management for web channel
        .service(web::resource("/api/chat/session").route(web::get().to(get_web_session)))
        .service(web::resource("/api/chat/session/new").route(web::post().to(new_web_session)));
}

async fn chat(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<ChatRequest>,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(ChatResponse {
                success: false,
                message: None,
                error: Some("No authorization token provided".to_string()),
                session_id: None,
            });
        }
    };

    // Validate the session
    match state.db.validate_session(&token) {
        Ok(Some(_)) => {} // Session is valid
        Ok(None) => {
            return HttpResponse::Unauthorized().json(ChatResponse {
                success: false,
                message: None,
                error: Some("Invalid or expired session".to_string()),
                session_id: None,
            });
        }
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            return HttpResponse::InternalServerError().json(ChatResponse {
                success: false,
                message: None,
                error: Some("Internal server error".to_string()),
                session_id: None,
            });
        }
    };

    // Get the latest user message from the request
    let user_message = match body.messages.iter().rev().find(|m| m.role == "user") {
        Some(msg) => msg.content.clone(),
        None => {
            return HttpResponse::BadRequest().json(ChatResponse {
                success: false,
                message: None,
                error: Some("No user message provided".to_string()),
                session_id: None,
            });
        }
    };

    // Generate a user ID for the web session
    // Use the provided user_id, or derive from the session token
    let user_id = body.user_id.clone()
        .unwrap_or_else(|| format!("web-{}", &token[..8.min(token.len())]));

    // Create a normalized message for the dispatcher
    // This makes web chat go through the same pipeline as Telegram/Slack
    let normalized = NormalizedMessage {
        channel_id: WEB_CHANNEL_ID,
        channel_type: WEB_CHANNEL_TYPE.to_string(),
        chat_id: user_id.clone(),  // For web, chat_id == user_id (always DM-like)
        chat_name: None,
        user_id: user_id.clone(),
        user_name: format!("web-user-{}", &user_id[..8.min(user_id.len())]),
        text: user_message,
        message_id: None,
        session_mode: None,
        selected_network: body.network.clone(),
        force_safe_mode: false,
        preferred_subtype: None,
    };

    // Dispatch through the unified pipeline
    // This gives us: sessions, identities, memories, tool execution, gateway events
    let result = state.dispatcher.dispatch_safe(normalized).await;

    if let Some(error) = result.error {
        log::error!("Chat dispatch error: {}", error);
        return HttpResponse::InternalServerError().json(ChatResponse {
            success: false,
            message: None,
            error: Some(error),
            session_id: None,
        });
    }

    HttpResponse::Ok().json(ChatResponse {
        success: true,
        message: Some(ChatMessage {
            role: "assistant".to_string(),
            content: result.response,
        }),
        error: None,
        session_id: None, // Could return session ID if needed
    })
}

/// Stop the current agent execution for the web channel
async fn stop_execution(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    use std::time::Duration;

    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(StopResponse {
                success: false,
                message: None,
                error: Some("No authorization token provided".to_string()),
            });
        }
    };

    // Validate the session
    match state.db.validate_session(&token) {
        Ok(Some(_)) => {} // Session is valid
        Ok(None) => {
            return HttpResponse::Unauthorized().json(StopResponse {
                success: false,
                message: None,
                error: Some("Invalid or expired session".to_string()),
            });
        }
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            return HttpResponse::InternalServerError().json(StopResponse {
                success: false,
                message: None,
                error: Some("Internal server error".to_string()),
            });
        }
    };

    // Cancel the execution for the web channel
    // This will:
    // 1. Cancel via CancellationToken (immediate interruption of async ops)
    // 2. Set the cancelled flag (for checkpoint compatibility)
    // 3. Emit execution.stopped event for frontend confirmation
    // 4. Complete/abort the current execution
    log::info!("[CHAT_STOP] Stopping execution for web channel {}", WEB_CHANNEL_ID);
    state.execution_tracker.cancel_execution(WEB_CHANNEL_ID);

    // Also cancel any session-based executions running on this channel
    // This ensures cron jobs running in "main" mode on channel 0 are also stopped
    state.execution_tracker.cancel_all_sessions_for_channel(WEB_CHANNEL_ID);

    // Also cancel any running subagents for this channel and wait for acknowledgment
    let mut subagents_cancelled = 0;
    if let Some(subagent_manager) = state.dispatcher.subagent_manager() {
        subagents_cancelled = subagent_manager
            .cancel_all_for_channel_and_wait(WEB_CHANNEL_ID, Duration::from_millis(100))
            .await;
        log::info!("[CHAT_STOP] Cancelled {} subagents for web channel", subagents_cancelled);
    }

    let message = if subagents_cancelled > 0 {
        format!("Execution stopped. {} subagent(s) cancelled.", subagents_cancelled)
    } else {
        "Execution stopped".to_string()
    };

    HttpResponse::Ok().json(StopResponse {
        success: true,
        message: Some(message),
        error: None,
    })
}

/// Get the current execution status for the web channel
async fn get_execution_status(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(ExecutionStatusResponse {
                running: false,
                execution_id: None,
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(ExecutionStatusResponse {
            running: false,
            execution_id: None,
        });
    }

    // Get execution ID for the web channel
    let execution_id = state.execution_tracker.get_execution_id(WEB_CHANNEL_ID);

    HttpResponse::Ok().json(ExecutionStatusResponse {
        running: execution_id.is_some(),
        execution_id,
    })
}

/// List all subagents for the web channel
async fn list_subagents(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ListSubagentsQuery>,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(SubagentListResponse {
                success: false,
                subagents: vec![],
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(SubagentListResponse {
            success: false,
            subagents: vec![],
        });
    }

    // Get subagents for the web channel, optionally filtered by session
    let filter_session_id = query.session_id;
    let subagents = if let Some(subagent_manager) = state.dispatcher.subagent_manager() {
        match subagent_manager.list_by_channel(WEB_CHANNEL_ID) {
            Ok(agents) => agents
                .into_iter()
                .filter(|ctx| {
                    if let Some(sid) = filter_session_id {
                        ctx.parent_session_id == sid
                    } else {
                        true
                    }
                })
                .map(|ctx| SubagentInfo {
                    id: ctx.id,
                    label: ctx.label,
                    task: if ctx.task.len() > 100 {
                        format!("{}...", &ctx.task[..97])
                    } else {
                        ctx.task
                    },
                    status: format!("{:?}", ctx.status),
                    started_at: ctx.started_at.to_rfc3339(),
                    session_id: ctx.session_id,
                    parent_session_id: ctx.parent_session_id,
                })
                .collect(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    HttpResponse::Ok().json(SubagentListResponse {
        success: true,
        subagents,
    })
}

/// Cancel a specific subagent
async fn cancel_subagent(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CancelSubagentRequest>,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(SubagentResponse {
                success: false,
                message: None,
                error: Some("No authorization token provided".to_string()),
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(SubagentResponse {
            success: false,
            message: None,
            error: Some("Invalid or expired session".to_string()),
        });
    }

    // Cancel the subagent
    if let Some(subagent_manager) = state.dispatcher.subagent_manager() {
        log::info!("[CHAT] Cancelling subagent {}", body.subagent_id);
        match subagent_manager.cancel(&body.subagent_id) {
            Ok(true) => {
                HttpResponse::Ok().json(SubagentResponse {
                    success: true,
                    message: Some(format!("Subagent {} cancelled", body.subagent_id)),
                    error: None,
                })
            }
            Ok(false) => {
                HttpResponse::Ok().json(SubagentResponse {
                    success: false,
                    message: None,
                    error: Some(format!("Subagent {} not found or already completed", body.subagent_id)),
                })
            }
            Err(e) => {
                HttpResponse::Ok().json(SubagentResponse {
                    success: false,
                    message: None,
                    error: Some(format!("Failed to cancel subagent: {}", e)),
                })
            }
        }
    } else {
        HttpResponse::Ok().json(SubagentResponse {
            success: false,
            message: None,
            error: Some("Subagent manager not available".to_string()),
        })
    }
}

/// Get current planner tasks for the web channel
async fn get_planner_tasks(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(GetPlannerTasksResponse {
                success: false,
                tasks: vec![],
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(GetPlannerTasksResponse {
            success: false,
            tasks: vec![],
        });
    }

    // Get tasks from execution tracker
    let tasks = state.execution_tracker.get_planner_tasks(WEB_CHANNEL_ID);
    let task_infos: Vec<PlannerTaskInfo> = tasks
        .into_iter()
        .map(|t| PlannerTaskInfo {
            id: t.id,
            description: t.description,
            status: t.status.to_string(),
        })
        .collect();

    HttpResponse::Ok().json(GetPlannerTasksResponse {
        success: true,
        tasks: task_infos,
    })
}

/// Delete a planner task by ID
async fn delete_task(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<u32>,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(DeleteTaskResponse {
                success: false,
                message: None,
                error: Some("No authorization token provided".to_string()),
                was_current_task: None,
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(DeleteTaskResponse {
            success: false,
            message: None,
            error: Some("Invalid or expired session".to_string()),
            was_current_task: None,
        });
    }

    let task_id = path.into_inner();
    log::info!("[CHAT] Deleting planner task {} for web channel", task_id);

    // Queue the task deletion - the dispatcher will handle it during the next checkpoint
    state.execution_tracker.queue_task_deletion(WEB_CHANNEL_ID, task_id);

    // If there's an active execution, we need to signal the dispatcher to check for deletions
    // The execution loop will pick up the deletion on its next iteration
    // If the deleted task is the current one, we should also stop the execution
    // For now, we'll just queue the deletion and let the frontend show it as deleted

    HttpResponse::Ok().json(DeleteTaskResponse {
        success: true,
        message: Some(format!("Task {} queued for deletion", task_id)),
        error: None,
        was_current_task: None, // We don't know until dispatcher processes it
    })
}

/// Response for web session endpoints
#[derive(Serialize)]
pub struct WebSessionResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Get the current active web session (or create one if none exists)
async fn get_web_session(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(WebSessionResponse {
                success: false,
                session_id: None,
                completion_status: None,
                message_count: None,
                created_at: None,
                error: Some("No authorization token provided".to_string()),
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(WebSessionResponse {
            success: false,
            session_id: None,
            completion_status: None,
            message_count: None,
            created_at: None,
            error: Some("Invalid or expired session".to_string()),
        });
    }

    // Get or create the web session
    // Use token prefix as the platform_chat_id to tie session to the auth token
    let chat_id = format!("web-{}", &token[..8.min(token.len())]);

    match state.db.get_or_create_chat_session(
        WEB_CHANNEL_TYPE,
        WEB_CHANNEL_ID,
        &chat_id,
        SessionScope::Dm,
        None,
    ) {
        Ok(session) => {
            // Get message count
            let message_count = state.db.count_session_messages(session.id).ok();

            HttpResponse::Ok().json(WebSessionResponse {
                success: true,
                session_id: Some(session.id),
                completion_status: Some(session.completion_status.as_str().to_string()),
                message_count,
                created_at: Some(session.created_at.to_rfc3339()),
                error: None,
            })
        }
        Err(e) => {
            log::error!("Failed to get or create web session: {}", e);
            HttpResponse::InternalServerError().json(WebSessionResponse {
                success: false,
                session_id: None,
                completion_status: None,
                message_count: None,
                created_at: None,
                error: Some(format!("Database error: {}", e)),
            })
        }
    }
}

/// Create a new web session (resets the current one)
async fn new_web_session(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    // Validate session token
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return HttpResponse::Unauthorized().json(WebSessionResponse {
                success: false,
                session_id: None,
                completion_status: None,
                message_count: None,
                created_at: None,
                error: Some("No authorization token provided".to_string()),
            });
        }
    };

    // Validate the session
    if state.db.validate_session(&token).ok().flatten().is_none() {
        return HttpResponse::Unauthorized().json(WebSessionResponse {
            success: false,
            session_id: None,
            completion_status: None,
            message_count: None,
            created_at: None,
            error: Some("Invalid or expired session".to_string()),
        });
    }

    // First get the current session
    let chat_id = format!("web-{}", &token[..8.min(token.len())]);

    let current_session = state.db.get_or_create_chat_session(
        WEB_CHANNEL_TYPE,
        WEB_CHANNEL_ID,
        &chat_id,
        SessionScope::Dm,
        None,
    );

    match current_session {
        Ok(session) => {
            // Reset the session (marks old as inactive, creates new)
            match state.db.reset_chat_session(session.id) {
                Ok(new_session) => {
                    log::info!("[CHAT] Created new web session {} (replaced {})", new_session.id, session.id);

                    HttpResponse::Ok().json(WebSessionResponse {
                        success: true,
                        session_id: Some(new_session.id),
                        completion_status: Some(new_session.completion_status.as_str().to_string()),
                        message_count: Some(0),
                        created_at: Some(new_session.created_at.to_rfc3339()),
                        error: None,
                    })
                }
                Err(e) => {
                    log::error!("Failed to reset web session: {}", e);
                    HttpResponse::InternalServerError().json(WebSessionResponse {
                        success: false,
                        session_id: None,
                        completion_status: None,
                        message_count: None,
                        created_at: None,
                        error: Some(format!("Failed to create new session: {}", e)),
                    })
                }
            }
        }
        Err(e) => {
            log::error!("Failed to get current web session: {}", e);
            HttpResponse::InternalServerError().json(WebSessionResponse {
                success: false,
                session_id: None,
                completion_status: None,
                message_count: None,
                created_at: None,
                error: Some(format!("Database error: {}", e)),
            })
        }
    }
}
