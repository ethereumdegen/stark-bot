use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use std::sync::Arc;

use crate::channels::NormalizedMessage;
use crate::controllers::chat::{ChatMessage, ChatResponse};
use crate::models::chat_session::SessionScope;
use crate::AppState;

const DEV_CHANNEL_ID: i64 = -1;
const DEV_CHANNEL_TYPE: &str = "dev_chat";

#[derive(Debug, Deserialize)]
pub struct DevChatRequest {
    pub message: String,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/api/dev/chat").route(web::post().to(dev_chat)));
}

async fn dev_chat(
    state: web::Data<AppState>,
    body: web::Json<DevChatRequest>,
) -> impl Responder {
    log::info!("[DEV_CHAT] Received: {}", body.message);

    // Subscribe to gateway events so we can capture say_to_user messages
    // (the dispatcher suppresses them from DispatchResult.response since
    // they're normally delivered via WebSocket)
    let broadcaster = &state.broadcaster;
    let (client_id, mut rx) = broadcaster.subscribe();

    // Collect say_to_user messages and other relevant events in a background task
    let collected: Arc<tokio::sync::Mutex<Vec<String>>> = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let collected_clone = collected.clone();
    let listener = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            // Capture say_to_user and task_fully_completed tool results
            if event.event == "tool.result" {
                let tool_name = event.data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let channel_id = event.data.get("channel_id").and_then(|v| v.as_i64());
                let success = event.data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");

                if channel_id == Some(DEV_CHANNEL_ID) && success && !content.is_empty() {
                    if tool_name == "say_to_user" || tool_name == "task_fully_completed" {
                        collected_clone.lock().await.push(content.to_string());
                    }
                }
            }
            // Also capture agent.response events (final AI text)
            if event.event == "agent.response" {
                let channel_id = event.data.get("channel_id").and_then(|v| v.as_i64());
                let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");

                if channel_id == Some(DEV_CHANNEL_ID) && !content.is_empty() {
                    collected_clone.lock().await.push(content.to_string());
                }
            }
        }
    });

    let normalized = NormalizedMessage {
        channel_id: DEV_CHANNEL_ID,
        channel_type: DEV_CHANNEL_TYPE.to_string(),
        chat_id: "dev-test".to_string(),
        chat_name: None,
        user_id: "dev-user".to_string(),
        user_name: "dev-user".to_string(),
        text: body.message.clone(),
        message_id: None,
        session_mode: None,
        selected_network: None,
        force_safe_mode: false,
        preferred_subtype: None,
    };

    let result = state.dispatcher.dispatch_safe(normalized).await;

    // Clean up: unsubscribe (stops new events) then give listener a moment to drain
    broadcaster.unsubscribe(&client_id);
    // Small yield to let the listener process any buffered events
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    listener.abort();

    // Look up the session that was created/used by the dispatcher
    let session_id = state.db.get_or_create_chat_session(
        DEV_CHANNEL_TYPE,
        DEV_CHANNEL_ID,
        "dev-test",
        SessionScope::Api,
        None,
    ).ok().map(|s| s.id);

    // Build response: prefer say_to_user messages, fall back to DispatchResult.response
    let say_messages = collected.lock().await;
    let response_text = if !say_messages.is_empty() {
        say_messages.join("\n\n")
    } else {
        result.response.clone()
    };

    if let Some(error) = result.error {
        log::error!("[DEV_CHAT] Dispatch error: {}", error);
        return HttpResponse::InternalServerError().json(ChatResponse {
            success: false,
            message: None,
            error: Some(error),
            session_id,
        });
    }

    HttpResponse::Ok().json(ChatResponse {
        success: true,
        message: Some(ChatMessage {
            role: "assistant".to_string(),
            content: response_text,
        }),
        error: None,
        session_id,
    })
}
