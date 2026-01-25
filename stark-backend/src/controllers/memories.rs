use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use serde::Deserialize;

use crate::models::{CreateMemoryRequest, MemoryResponse, MemoryType, SearchMemoriesRequest};
use crate::AppState;

/// Create a new memory
async fn create_memory(
    data: web::Data<AppState>,
    body: web::Json<CreateMemoryRequest>,
) -> impl Responder {
    // For daily logs, set log_date to today if not provided
    let log_date = if body.memory_type == MemoryType::DailyLog {
        body.log_date.or_else(|| Some(Utc::now().date_naive()))
    } else {
        body.log_date
    };

    match data.db.create_memory(
        body.memory_type,
        &body.content,
        body.category.as_deref(),
        body.tags.as_deref(),
        body.importance,
        body.identity_id.as_deref(),
        body.session_id,
        body.source_channel_type.as_deref(),
        body.source_message_id.as_deref(),
        log_date,
        body.expires_at,
    ) {
        Ok(memory) => {
            let response: MemoryResponse = memory.into();
            HttpResponse::Created().json(response)
        }
        Err(e) => {
            log::error!("Failed to create memory: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Search memories using FTS5
async fn search_memories(
    data: web::Data<AppState>,
    body: web::Json<SearchMemoriesRequest>,
) -> impl Responder {
    match data.db.search_memories(
        &body.query,
        body.memory_type,
        body.identity_id.as_deref(),
        body.category.as_deref(),
        body.min_importance,
        body.limit,
    ) {
        Ok(results) => HttpResponse::Ok().json(results),
        Err(e) => {
            log::error!("Failed to search memories: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get today's daily logs
#[derive(Deserialize)]
struct DailyLogsQuery {
    identity_id: Option<String>,
}

async fn get_daily_logs(
    data: web::Data<AppState>,
    query: web::Query<DailyLogsQuery>,
) -> impl Responder {
    match data.db.get_todays_daily_logs(query.identity_id.as_deref()) {
        Ok(memories) => {
            let responses: Vec<MemoryResponse> = memories.into_iter().map(|m| m.into()).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            log::error!("Failed to get daily logs: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get long-term memories
#[derive(Deserialize)]
struct LongTermQuery {
    identity_id: Option<String>,
    min_importance: Option<i32>,
    #[serde(default = "default_limit")]
    limit: i32,
}

fn default_limit() -> i32 {
    20
}

async fn get_long_term_memories(
    data: web::Data<AppState>,
    query: web::Query<LongTermQuery>,
) -> impl Responder {
    match data.db.get_long_term_memories(
        query.identity_id.as_deref(),
        query.min_importance,
        query.limit,
    ) {
        Ok(memories) => {
            let responses: Vec<MemoryResponse> = memories.into_iter().map(|m| m.into()).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            log::error!("Failed to get long-term memories: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Delete a memory
async fn delete_memory(
    data: web::Data<AppState>,
    path: web::Path<i64>,
) -> impl Responder {
    let memory_id = path.into_inner();

    match data.db.delete_memory(memory_id) {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "Memory deleted"
        })),
        Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Memory not found"
        })),
        Err(e) => {
            log::error!("Failed to delete memory: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Cleanup expired memories
async fn cleanup_expired(data: web::Data<AppState>) -> impl Responder {
    match data.db.cleanup_expired_memories() {
        Ok(count) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "deleted_count": count
        })),
        Err(e) => {
            log::error!("Failed to cleanup expired memories: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/memories")
            .route("", web::post().to(create_memory))
            .route("/search", web::post().to(search_memories))
            .route("/daily", web::get().to(get_daily_logs))
            .route("/long-term", web::get().to(get_long_term_memories))
            .route("/cleanup", web::post().to(cleanup_expired))
            .route("/{id}", web::delete().to(delete_memory)),
    );
}
