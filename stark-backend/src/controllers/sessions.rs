use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::models::{
    ChatSessionResponse, GetOrCreateSessionRequest, SessionScope, SessionTranscriptResponse,
    UpdateResetPolicyRequest,
};
use crate::AppState;

/// Get or create a chat session
async fn get_or_create_session(
    data: web::Data<AppState>,
    body: web::Json<GetOrCreateSessionRequest>,
) -> impl Responder {
    let scope = body.scope.unwrap_or(SessionScope::Dm);

    match data.db.get_or_create_chat_session(
        &body.channel_type,
        body.channel_id,
        &body.platform_chat_id,
        scope,
        body.agent_id.as_deref(),
    ) {
        Ok(session) => {
            let mut response: ChatSessionResponse = session.into();
            // Get message count
            if let Ok(count) = data.db.count_session_messages(response.id) {
                response.message_count = Some(count);
            }
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            log::error!("Failed to get or create session: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get a session by ID
async fn get_session(
    data: web::Data<AppState>,
    path: web::Path<i64>,
) -> impl Responder {
    let session_id = path.into_inner();

    match data.db.get_chat_session(session_id) {
        Ok(Some(session)) => {
            let mut response: ChatSessionResponse = session.into();
            if let Ok(count) = data.db.count_session_messages(response.id) {
                response.message_count = Some(count);
            }
            HttpResponse::Ok().json(response)
        }
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Session not found"
        })),
        Err(e) => {
            log::error!("Failed to get session: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Reset a session
async fn reset_session(
    data: web::Data<AppState>,
    path: web::Path<i64>,
) -> impl Responder {
    let session_id = path.into_inner();

    match data.db.reset_chat_session(session_id) {
        Ok(session) => {
            let response: ChatSessionResponse = session.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            log::error!("Failed to reset session: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Update session reset policy
async fn update_reset_policy(
    data: web::Data<AppState>,
    path: web::Path<i64>,
    body: web::Json<UpdateResetPolicyRequest>,
) -> impl Responder {
    let session_id = path.into_inner();

    match data.db.update_session_reset_policy(
        session_id,
        body.reset_policy,
        body.idle_timeout_minutes,
        body.daily_reset_hour,
    ) {
        Ok(Some(session)) => {
            let response: ChatSessionResponse = session.into();
            HttpResponse::Ok().json(response)
        }
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Session not found"
        })),
        Err(e) => {
            log::error!("Failed to update session reset policy: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get session transcript (message history)
#[derive(Deserialize)]
struct TranscriptQuery {
    limit: Option<i32>,
}

async fn get_transcript(
    data: web::Data<AppState>,
    path: web::Path<i64>,
    query: web::Query<TranscriptQuery>,
) -> impl Responder {
    let session_id = path.into_inner();

    let messages = if let Some(limit) = query.limit {
        data.db.get_recent_session_messages(session_id, limit)
    } else {
        data.db.get_session_messages(session_id)
    };

    match messages {
        Ok(msgs) => {
            let total = data.db.count_session_messages(session_id).unwrap_or(msgs.len() as i64);
            HttpResponse::Ok().json(SessionTranscriptResponse {
                session_id,
                messages: msgs,
                total_count: total,
            })
        }
        Err(e) => {
            log::error!("Failed to get session transcript: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/sessions")
            .route("", web::post().to(get_or_create_session))
            .route("/{id}", web::get().to(get_session))
            .route("/{id}/reset", web::post().to(reset_session))
            .route("/{id}/policy", web::put().to(update_reset_policy))
            .route("/{id}/transcript", web::get().to(get_transcript)),
    );
}
