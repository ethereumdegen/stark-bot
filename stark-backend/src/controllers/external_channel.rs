use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
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

// ── SSE event types ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
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
            .route("/token/generate", web::post().to(gateway_generate_token)),
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
        .unwrap_or_else(|| "default".to_string());
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
        preferred_subtype: None,
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
        .unwrap_or_else(|| "default".to_string());
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
            preferred_subtype: None,
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
                    Some(SseEvent {
                        event_type: "tool_call".to_string(),
                        content: None,
                        tool_name: Some(tool_name.to_string()),
                    })
                }
                "tool.result" => {
                    let tool_name = event.data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                    let success = event.data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");

                    if success && !content.is_empty() && (tool_name == "say_to_user" || tool_name == "task_fully_completed") {
                        Some(SseEvent {
                            event_type: "text".to_string(),
                            content: Some(content.to_string()),
                            tool_name: None,
                        })
                    } else {
                        Some(SseEvent {
                            event_type: "tool_result".to_string(),
                            content: None,
                            tool_name: Some(tool_name.to_string()),
                        })
                    }
                }
                "agent.response" => {
                    let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if !content.is_empty() {
                        Some(SseEvent {
                            event_type: "text".to_string(),
                            content: Some(content.to_string()),
                            tool_name: None,
                        })
                    } else {
                        None
                    }
                }
                "dispatch.complete" => {
                    let done = SseEvent {
                        event_type: "done".to_string(),
                        content: None,
                        tool_name: None,
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
