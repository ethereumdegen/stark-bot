use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::channels::NormalizedMessage;
use crate::models::chat_session::SessionScope;
use crate::models::Channel;
use crate::AppState;

const CHANNEL_TYPE: &str = "external_channel";

// ── Request / Response types ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GatewayChatRequest {
    pub message: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub user_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GatewayChatResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GatewaySessionInfo {
    pub id: i64,
    pub session_key: String,
    pub created_at: String,
    pub last_activity_at: String,
    pub message_count: i64,
}

#[derive(Debug, Serialize)]
pub struct GatewaySessionsResponse {
    pub success: bool,
    pub sessions: Vec<GatewaySessionInfo>,
}

#[derive(Debug, Serialize)]
pub struct GatewayMessageInfo {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct GatewayMessagesResponse {
    pub success: bool,
    pub messages: Vec<GatewayMessageInfo>,
}

#[derive(Debug, Serialize)]
pub struct GatewayNewSessionResponse {
    pub success: bool,
    pub session_id: i64,
}

#[derive(Debug, Serialize)]
pub struct GatewayTokenResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
struct GatewayErrorResponse {
    success: bool,
    error: String,
}

#[derive(Debug, Serialize)]
pub struct GatewayModuleInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub has_tui: bool,
}

#[derive(Debug, Serialize)]
pub struct GatewayModulesResponse {
    pub success: bool,
    pub modules: Vec<GatewayModuleInfo>,
}

// ── SSE event types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_name: Option<String>,
}

// ── Route configuration ─────────────────────────────────────────────────

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/gateway")
            .route("/chat", web::post().to(gateway_chat))
            .route("/chat/stream", web::post().to(gateway_chat_stream))
            .route("/sessions", web::get().to(gateway_sessions))
            .route("/sessions/{id}/messages", web::get().to(gateway_session_messages))
            .route("/sessions/new", web::post().to(gateway_new_session))
            .route("/token/generate", web::post().to(gateway_generate_token))
            .route("/modules", web::get().to(gateway_modules_list))
            .route("/modules/{name}/tui/stream", web::get().to(gateway_tui_stream))
            .route("/modules/{name}/tui/action", web::post().to(gateway_tui_action)),
    );
}

// ── Auth helpers ────────────────────────────────────────────────────────

/// Constant-time byte comparison to prevent timing attacks
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Extract Bearer token from Authorization header
fn extract_bearer_token(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
}

/// Validate gateway token: find the matching external_channel channel.
/// Returns (channel_id, Channel) on success, or an HttpResponse error.
fn validate_gateway_token(
    state: &web::Data<AppState>,
    req: &HttpRequest,
) -> Result<(i64, Channel), HttpResponse> {
    let token = match extract_bearer_token(req) {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Err(HttpResponse::Unauthorized().json(GatewayErrorResponse {
                success: false,
                error: "Missing or empty Authorization: Bearer <token>".to_string(),
            }));
        }
    };

    // List all channels and filter to external_channel type
    let channels = state.db.list_channels().unwrap_or_default();
    let ext_channels: Vec<&Channel> = channels
        .iter()
        .filter(|ch| ch.channel_type == CHANNEL_TYPE)
        .collect();

    if ext_channels.is_empty() {
        return Err(HttpResponse::Unauthorized().json(GatewayErrorResponse {
            success: false,
            error: "No external channels configured".to_string(),
        }));
    }

    for ch in &ext_channels {
        if let Ok(Some(stored_token)) =
            state.db.get_channel_setting(ch.id, "external_channel_api_token")
        {
            if !stored_token.is_empty()
                && constant_time_eq(token.as_bytes(), stored_token.as_bytes())
            {
                // Token matches — verify channel is running
                let channel_manager = state.gateway.channel_manager();
                if !channel_manager.is_running(ch.id) {
                    return Err(HttpResponse::Forbidden().json(GatewayErrorResponse {
                        success: false,
                        error: "External channel is not running".to_string(),
                    }));
                }
                return Ok((ch.id, (*ch).clone()));
            }
        }
    }

    Err(HttpResponse::Unauthorized().json(GatewayErrorResponse {
        success: false,
        error: "Invalid gateway token".to_string(),
    }))
}

/// Validate web SIWE session (for admin actions like token generation)
fn validate_web_session(
    state: &web::Data<AppState>,
    req: &HttpRequest,
) -> Result<(), HttpResponse> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(HttpResponse::Unauthorized().json(GatewayErrorResponse {
                success: false,
                error: "No authorization token provided".to_string(),
            }));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(GatewayErrorResponse {
            success: false,
            error: "Invalid or expired session".to_string(),
        })),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(GatewayErrorResponse {
                success: false,
                error: "Internal server error".to_string(),
            }))
        }
    }
}

// ── Endpoint handlers ───────────────────────────────────────────────────

/// POST /api/gateway/chat — send message, get full response
async fn gateway_chat(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<GatewayChatRequest>,
) -> impl Responder {
    let (channel_id, channel) = match validate_gateway_token(&state, &req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    log::info!(
        "[EXT_CHANNEL] Chat on '{}' (id={}): {} chars",
        channel.name,
        channel_id,
        body.message.len()
    );

    let chat_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let user_name = body
        .user_name
        .clone()
        .unwrap_or_else(|| "gateway-user".to_string());

    // Subscribe to events so we can capture say_to_user / agent.response
    let broadcaster = &state.broadcaster;
    let (client_id, mut rx) = broadcaster.subscribe();

    let collected: Arc<tokio::sync::Mutex<Vec<String>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let collected_clone = collected.clone();
    let cid = channel_id;
    let listener = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if event.event == "tool.result" {
                let tool_name = event
                    .data
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let ev_channel = event.data.get("channel_id").and_then(|v| v.as_i64());
                let success = event
                    .data
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let content = event
                    .data
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if ev_channel == Some(cid) && success && !content.is_empty() {
                    if tool_name == "say_to_user" || tool_name == "task_fully_completed" {
                        collected_clone.lock().await.push(content.to_string());
                    }
                }
            }
            if event.event == "agent.response" {
                let ev_channel = event.data.get("channel_id").and_then(|v| v.as_i64());
                let content = event
                    .data
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if ev_channel == Some(cid) && !content.is_empty() {
                    collected_clone.lock().await.push(content.to_string());
                }
            }
        }
    });

    let safe_mode = state
        .db
        .get_channel_setting(channel_id, "external_channel_safe_mode")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);

    let normalized = NormalizedMessage {
        channel_id,
        channel_type: CHANNEL_TYPE.to_string(),
        chat_id: chat_id.clone(),
        chat_name: None,
        user_id: "gateway-user".to_string(),
        user_name,
        text: body.message.clone(),
        message_id: None,
        session_mode: None,
        selected_network: None,
        force_safe_mode: safe_mode,
        platform_role_ids: vec![],
        chat_context: None,
    };

    let result = state.dispatcher.dispatch_safe(normalized).await;

    broadcaster.unsubscribe(&client_id);
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    listener.abort();

    // Look up session
    let session_id = state
        .db
        .get_or_create_chat_session(CHANNEL_TYPE, channel_id, &chat_id, SessionScope::Api, None)
        .ok()
        .map(|s| s.id);

    let say_messages = collected.lock().await;
    let response_text = if !say_messages.is_empty() {
        say_messages.join("\n\n")
    } else {
        result.response.clone()
    };

    if let Some(error) = result.error {
        log::error!("[EXT_CHANNEL] Dispatch error: {}", error);
        return HttpResponse::InternalServerError().json(GatewayChatResponse {
            success: false,
            response: None,
            session_id,
            error: Some(error),
        });
    }

    HttpResponse::Ok().json(GatewayChatResponse {
        success: true,
        response: Some(response_text),
        session_id,
        error: None,
    })
}

/// POST /api/gateway/chat/stream — send message, get SSE stream
async fn gateway_chat_stream(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<GatewayChatRequest>,
) -> impl Responder {
    let (channel_id, channel) = match validate_gateway_token(&state, &req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    log::info!(
        "[EXT_CHANNEL] Stream on '{}' (id={}): {} chars",
        channel.name,
        channel_id,
        body.message.len()
    );

    let chat_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let user_name = body
        .user_name
        .clone()
        .unwrap_or_else(|| "gateway-user".to_string());

    let safe_mode = state
        .db
        .get_channel_setting(channel_id, "external_channel_safe_mode")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);

    let broadcaster = state.broadcaster.clone();
    let (client_id, mut rx) = broadcaster.subscribe();
    let cid = channel_id;

    // Dispatch in a background task
    let dispatcher = state.dispatcher.clone();
    let msg_text = body.message.clone();
    let broadcaster_bg = broadcaster.clone();
    let client_id_bg = client_id.clone();

    tokio::spawn(async move {
        let normalized = NormalizedMessage {
            channel_id: cid,
            channel_type: CHANNEL_TYPE.to_string(),
            chat_id,
            chat_name: None,
            user_id: "gateway-user".to_string(),
            user_name,
            text: msg_text,
            message_id: None,
            session_mode: None,
            selected_network: None,
            force_safe_mode: safe_mode,
            platform_role_ids: vec![],
        chat_context: None,
        };
        let _ = dispatcher.dispatch_safe(normalized).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        broadcaster_bg.unsubscribe(&client_id_bg);
    });

    // Use a channel to bridge events into the SSE stream
    let (tx, mut sse_rx) = tokio::sync::mpsc::channel::<web::Bytes>(64);

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let ev_channel = event.data.get("channel_id").and_then(|v| v.as_i64());
            if ev_channel != Some(cid) {
                continue;
            }

            let sse = match event.event.as_str() {
                "tool.call" => {
                    let tool_name = event.data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let parameters = event.data.get("parameters").cloned();
                    Some(SseEvent {
                        event_type: "tool_call".to_string(),
                        content: None,
                        tool_name: Some(tool_name.to_string()),
                        parameters,
                        success: None, duration_ms: None,
                        label: None, agent_subtype: None, error: None, task_name: None,
                    })
                }
                "tool.result" => {
                    let tool_name = event.data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                    let success = event.data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let duration_ms = event.data.get("duration_ms").and_then(|v| v.as_i64());

                    // For say_to_user / task_fully_completed, also emit text so the
                    // main response body is visible inline
                    if success && !content.is_empty() && (tool_name == "say_to_user" || tool_name == "task_fully_completed") {
                        // Send the text event first
                        let text_event = SseEvent {
                            event_type: "text".to_string(),
                            content: Some(content.to_string()),
                            tool_name: None, parameters: None,
                            success: None, duration_ms: None,
                            label: None, agent_subtype: None, error: None, task_name: None,
                        };
                        if let Ok(json) = serde_json::to_string(&text_event) {
                            let _ = tx.send(web::Bytes::from(format!("data: {}\n\n", json))).await;
                        }
                    }

                    // Always emit the tool_result so the CLI can show tool activity
                    let content_val = if !content.is_empty() { Some(content.to_string()) } else { None };
                    Some(SseEvent {
                        event_type: "tool_result".to_string(),
                        content: content_val,
                        tool_name: Some(tool_name.to_string()),
                        parameters: None,
                        success: Some(success),
                        duration_ms,
                        label: None, agent_subtype: None, error: None, task_name: None,
                    })
                }
                "agent.response" => {
                    let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if !content.is_empty() {
                        Some(SseEvent {
                            event_type: "text".to_string(),
                            content: Some(content.to_string()),
                            tool_name: None, parameters: None,
                            success: None, duration_ms: None,
                            label: None, agent_subtype: None, error: None, task_name: None,
                        })
                    } else {
                        None
                    }
                }
                "subagent.spawned" => {
                    let label = event.data.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let agent_subtype = event.data.get("agent_subtype").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let task = event.data.get("task").and_then(|v| v.as_str()).map(|s| s.to_string());
                    Some(SseEvent {
                        event_type: "subagent_spawned".to_string(),
                        content: task,
                        tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: Some(label), agent_subtype, error: None, task_name: None,
                    })
                }
                "subagent.completed" => {
                    let label = event.data.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some(SseEvent {
                        event_type: "subagent_completed".to_string(),
                        content: None, tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: Some(label), agent_subtype: None, error: None, task_name: None,
                    })
                }
                "subagent.failed" => {
                    let label = event.data.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let error = event.data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error").to_string();
                    Some(SseEvent {
                        event_type: "subagent_failed".to_string(),
                        content: None, tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: Some(label), agent_subtype: None, error: Some(error), task_name: None,
                    })
                }
                "agent.subtype_change" => {
                    let subtype = event.data.get("subtype").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let label = event.data.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some(SseEvent {
                        event_type: "subtype_change".to_string(),
                        content: None, tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: Some(label), agent_subtype: Some(subtype), error: None, task_name: None,
                    })
                }
                "agent.thinking" => {
                    let message = event.data.get("message").and_then(|v| v.as_str()).unwrap_or("thinking...").to_string();
                    Some(SseEvent {
                        event_type: "thinking".to_string(),
                        content: Some(message),
                        tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: None, agent_subtype: None, error: None, task_name: None,
                    })
                }
                "execution.task_started" => {
                    let name = event.data.get("name")
                        .or_else(|| event.data.get("description"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("").to_string();
                    if !name.is_empty() {
                        Some(SseEvent {
                            event_type: "task_started".to_string(),
                            content: None, tool_name: None, parameters: None,
                            success: None, duration_ms: None,
                            label: None, agent_subtype: None, error: None, task_name: Some(name),
                        })
                    } else {
                        None
                    }
                }
                "execution.task_completed" => {
                    let status = event.data.get("status").and_then(|v| v.as_str()).unwrap_or("completed").to_string();
                    Some(SseEvent {
                        event_type: "task_completed".to_string(),
                        content: Some(status),
                        tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: None, agent_subtype: None, error: None, task_name: None,
                    })
                }
                "dispatch.complete" => {
                    let done = SseEvent {
                        event_type: "done".to_string(),
                        content: None, tool_name: None, parameters: None,
                        success: None, duration_ms: None,
                        label: None, agent_subtype: None, error: None, task_name: None,
                    };
                    if let Ok(json) = serde_json::to_string(&done) {
                        let _ = tx.send(web::Bytes::from(format!("data: {}\n\n", json))).await;
                    }
                    break;
                }
                _ => None,
            };

            if let Some(sse) = sse {
                if let Ok(json) = serde_json::to_string(&sse) {
                    if tx.send(web::Bytes::from(format!("data: {}\n\n", json))).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let stream = futures_util::stream::unfold(sse_rx, |mut rx| async move {
        rx.recv().await.map(|bytes| (Ok::<_, actix_web::Error>(bytes), rx))
    });

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(stream)
}

/// GET /api/gateway/sessions — list sessions for this channel
async fn gateway_sessions(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    let (channel_id, _channel) = match validate_gateway_token(&state, &req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let sessions = state.db.list_chat_sessions().unwrap_or_default();
    let filtered: Vec<GatewaySessionInfo> = sessions
        .into_iter()
        .filter(|s| s.channel_type == CHANNEL_TYPE && s.channel_id == channel_id)
        .map(|s| {
            let msg_count = state.db.count_session_messages(s.id).unwrap_or(0);
            GatewaySessionInfo {
                id: s.id,
                session_key: s.session_key,
                created_at: s.created_at.to_rfc3339(),
                last_activity_at: s.last_activity_at.to_rfc3339(),
                message_count: msg_count,
            }
        })
        .collect();

    HttpResponse::Ok().json(GatewaySessionsResponse {
        success: true,
        sessions: filtered,
    })
}

/// GET /api/gateway/sessions/{id}/messages — get message history
async fn gateway_session_messages(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> impl Responder {
    let (channel_id, _channel) = match validate_gateway_token(&state, &req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let session_id = path.into_inner();

    // Verify session belongs to this channel
    if let Ok(Some(session)) = state.db.get_chat_session(session_id) {
        if session.channel_type != CHANNEL_TYPE || session.channel_id != channel_id {
            return HttpResponse::Forbidden().json(GatewayErrorResponse {
                success: false,
                error: "Session does not belong to this channel".to_string(),
            });
        }
    } else {
        return HttpResponse::NotFound().json(GatewayErrorResponse {
            success: false,
            error: "Session not found".to_string(),
        });
    }

    let messages = state.db.get_session_messages(session_id).unwrap_or_default();
    let response: Vec<GatewayMessageInfo> = messages
        .into_iter()
        .map(|m| GatewayMessageInfo {
            role: m.role.as_str().to_string(),
            content: m.content,
            user_name: m.user_name,
            created_at: m.created_at.to_rfc3339(),
        })
        .collect();

    HttpResponse::Ok().json(GatewayMessagesResponse {
        success: true,
        messages: response,
    })
}

/// POST /api/gateway/sessions/new — create a new session (reset)
async fn gateway_new_session(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    let (channel_id, _channel) = match validate_gateway_token(&state, &req) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    match state.db.create_gateway_session(
        CHANNEL_TYPE,
        channel_id,
        SessionScope::Api,
        None,
    ) {
        Ok(session) => HttpResponse::Ok().json(GatewayNewSessionResponse {
            success: true,
            session_id: session.id,
        }),
        Err(e) => {
            log::error!("[EXT_CHANNEL] Failed to create session: {}", e);
            HttpResponse::InternalServerError().json(GatewayErrorResponse {
                success: false,
                error: "Failed to create session".to_string(),
            })
        }
    }
}

/// POST /api/gateway/token/generate — generate a new API token (admin action)
async fn gateway_generate_token(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    // This endpoint uses web SIWE auth, not gateway token
    if let Err(resp) = validate_web_session(&state, &req) {
        return resp;
    }

    let channel_id = match body.get("channel_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => {
            return HttpResponse::BadRequest().json(GatewayTokenResponse {
                success: false,
                token: None,
                error: Some("channel_id is required".to_string()),
            });
        }
    };

    // Verify it's an external_channel
    match state.db.get_channel(channel_id) {
        Ok(Some(ch)) if ch.channel_type == CHANNEL_TYPE => {}
        _ => {
            return HttpResponse::BadRequest().json(GatewayTokenResponse {
                success: false,
                token: None,
                error: Some("Channel not found or not an external channel".to_string()),
            });
        }
    }

    // Generate 256-bit random token (64 hex chars)
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    let token = hex::encode(bytes);

    if let Err(e) = state
        .db
        .set_channel_setting(channel_id, "external_channel_api_token", &token)
    {
        log::error!("[EXT_CHANNEL] Failed to save token: {}", e);
        return HttpResponse::InternalServerError().json(GatewayTokenResponse {
            success: false,
            token: None,
            error: Some("Failed to save token".to_string()),
        });
    }

    log::info!(
        "[EXT_CHANNEL] Generated new API token for channel {}",
        channel_id
    );

    HttpResponse::Ok().json(GatewayTokenResponse {
        success: true,
        token: Some(token),
        error: None,
    })
}

// ── Module listing endpoint ──────────────────────────────────────────────

/// GET /api/gateway/modules — list installed modules
async fn gateway_modules_list(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_gateway_token(&state, &req) {
        return resp;
    }

    let registry = crate::modules::ModuleRegistry::new();
    let modules: Vec<GatewayModuleInfo> = registry
        .available_modules()
        .into_iter()
        .map(|m| GatewayModuleInfo {
            name: m.name().to_string(),
            description: m.description().to_string(),
            version: m.version().to_string(),
            has_tui: m.dashboard_styles().iter().any(|s| s == "tui"),
        })
        .collect();

    HttpResponse::Ok().json(GatewayModulesResponse {
        success: true,
        modules,
    })
}

// ── TUI streaming endpoints ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TuiStreamQuery {
    #[serde(default = "default_tui_width")]
    width: u32,
    #[serde(default = "default_tui_height")]
    height: u32,
}

fn default_tui_width() -> u32 { 120 }
fn default_tui_height() -> u32 { 40 }

/// Fetch a TUI ANSI frame from a module's service endpoint
async fn fetch_tui_frame(
    module_name: &str,
    width: u32,
    height: u32,
    selected: Option<i64>,
    scroll: Option<i64>,
) -> Result<(String, Option<Value>), String> {
    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(module_name) {
        Some(m) => m,
        None => return Err(format!("Unknown module: '{}'", module_name)),
    };

    let mut url = format!(
        "{}/rpc/dashboard/tui?width={}&height={}",
        module.service_url(),
        width,
        height
    );
    if let Some(s) = selected {
        url.push_str(&format!("&selected={}", s));
    }
    if let Some(s) = scroll {
        url.push_str(&format!("&scroll={}", s));
    }

    let client = reqwest::Client::new();
    let ansi = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch TUI frame: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read TUI frame: {}", e))?;

    // Optionally fetch actions metadata
    let actions_url = format!("{}/rpc/dashboard/tui/actions", module.service_url());
    let actions = client
        .get(&actions_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()
        .and_then(|r| {
            if r.status().is_success() {
                Some(r)
            } else {
                None
            }
        });
    let actions_json = match actions {
        Some(resp) => resp.json::<Value>().await.ok(),
        None => None,
    };

    Ok((ansi, actions_json))
}

/// GET /api/gateway/modules/{name}/tui/stream — SSE stream of TUI frames
async fn gateway_tui_stream(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    query: web::Query<TuiStreamQuery>,
) -> impl Responder {
    if let Err(resp) = validate_gateway_token(&state, &req) {
        return resp;
    }

    let module_name = path.into_inner();
    let width = query.width;
    let height = query.height;

    let broadcaster = state.broadcaster.clone();
    let (_client_id, mut rx) = broadcaster.subscribe();

    let (tx, mut sse_rx) = tokio::sync::mpsc::channel::<web::Bytes>(64);

    let mod_name = module_name.clone();
    tokio::spawn(async move {
        // Send initial frame
        match fetch_tui_frame(&mod_name, width, height, None, None).await {
            Ok((ansi, actions)) => {
                let mut frame = serde_json::json!({ "ansi": ansi });
                if let Some(a) = actions {
                    frame["actions"] = a;
                }
                let sse = format!("event: tui_frame\ndata: {}\n\n", frame);
                let _ = tx.send(web::Bytes::from(sse)).await;
            }
            Err(e) => {
                let sse = format!("event: error\ndata: {}\n\n", serde_json::json!({ "error": e }));
                let _ = tx.send(web::Bytes::from(sse)).await;
                return;
            }
        }

        // Listen for invalidation events
        loop {
            match rx.recv().await {
                Some(event) => {
                    if event.event != "module.tui_invalidate" {
                        continue;
                    }
                    let event_module = event.data.get("module").and_then(|v| v.as_str()).unwrap_or("");
                    if event_module != mod_name {
                        continue;
                    }

                    match fetch_tui_frame(&mod_name, width, height, None, None).await {
                        Ok((ansi, _)) => {
                            let frame = serde_json::json!({ "ansi": ansi });
                            let sse = format!("event: tui_frame\ndata: {}\n\n", frame);
                            if tx.send(web::Bytes::from(sse)).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => {} // skip failed frame fetches
                    }
                }
                None => break, // broadcaster shut down
            }
        }
    });

    let stream = futures_util::stream::unfold(sse_rx, |mut rx| async move {
        rx.recv().await.map(|bytes| (Ok::<_, actix_web::Error>(bytes), rx))
    });

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(stream)
}

/// POST /api/gateway/modules/{name}/tui/action — forward TUI action to module
async fn gateway_tui_action(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<Value>,
) -> impl Responder {
    if let Err(resp) = validate_gateway_token(&state, &req) {
        return resp;
    }

    let module_name = path.into_inner();

    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(&module_name) {
        Some(m) => m,
        None => {
            return HttpResponse::NotFound().json(GatewayErrorResponse {
                success: false,
                error: format!("Unknown module: '{}'", module_name),
            });
        }
    };

    let url = format!("{}/rpc/dashboard/tui/action", module.service_url());

    let client = reqwest::Client::new();
    match client
        .post(&url)
        .json(&body.into_inner())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            HttpResponse::build(actix_web::http::StatusCode::from_u16(status.as_u16()).unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR))
                .content_type("application/json")
                .body(body)
        }
        Err(e) => {
            HttpResponse::BadGateway().json(GatewayErrorResponse {
                success: false,
                error: format!("Failed to proxy action: {}", e),
            })
        }
    }
}
