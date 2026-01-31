use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::channels::NormalizedMessage;
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
}

/// Response listing subagents
#[derive(Serialize)]
pub struct SubagentListResponse {
    pub success: bool,
    pub subagents: Vec<SubagentInfo>,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/api/chat").route(web::post().to(chat)))
        .service(web::resource("/api/chat/stop").route(web::post().to(stop_execution)))
        .service(web::resource("/api/chat/subagents").route(web::get().to(list_subagents)))
        .service(web::resource("/api/chat/subagents/cancel").route(web::post().to(cancel_subagent)));
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
        user_id: user_id.clone(),
        user_name: format!("web-user-{}", &user_id[..8.min(user_id.len())]),
        text: user_message,
        message_id: None,
    };

    // Dispatch through the unified pipeline
    // This gives us: sessions, identities, memories, tool execution, gateway events
    let result = state.dispatcher.dispatch(normalized).await;

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
    log::info!("[CHAT_STOP] Stopping execution for web channel {}", WEB_CHANNEL_ID);
    state.execution_tracker.cancel_execution(WEB_CHANNEL_ID);

    // Also cancel any running subagents for this channel
    let mut subagents_cancelled = 0;
    if let Some(subagent_manager) = state.dispatcher.subagent_manager() {
        subagents_cancelled = subagent_manager.cancel_all_for_channel(WEB_CHANNEL_ID);
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

/// List all subagents for the web channel
async fn list_subagents(
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

    // Get subagents for the web channel
    let subagents = if let Some(subagent_manager) = state.dispatcher.subagent_manager() {
        match subagent_manager.list_by_channel(WEB_CHANNEL_ID) {
            Ok(agents) => agents
                .into_iter()
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
