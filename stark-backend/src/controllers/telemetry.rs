//! Telemetry API endpoints for execution traces, reward stats, and resource versioning.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::telemetry::{Resource, ResourceType};

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/telemetry")
            .route("/session/{id}/timeline", web::get().to(get_session_timeline))
            .route("/rollout/{id}/summary", web::get().to(get_rollout_summary))
            .route("/rollout/{id}/triplets", web::get().to(get_rollout_triplets))
            .route("/rewards/stats", web::get().to(get_reward_stats))
    );
    cfg.service(
        web::scope("/api/resources")
            .route("", web::get().to(list_resources))
            .route("", web::post().to(create_resource))
            .route("/rollback/{version}", web::post().to(rollback_resource))
    );
}

async fn get_session_timeline(
    state: web::Data<AppState>,
    path: web::Path<i64>,
    _req: HttpRequest,
) -> impl Responder {
    let session_id = path.into_inner();
    let timeline = state.telemetry_store.get_session_timeline(session_id);
    HttpResponse::Ok().json(timeline)
}

async fn get_rollout_summary(
    state: web::Data<AppState>,
    path: web::Path<String>,
    _req: HttpRequest,
) -> impl Responder {
    let rollout_id = path.into_inner();
    let summary = state.telemetry_store.get_execution_summary(&rollout_id);
    HttpResponse::Ok().json(summary)
}

async fn get_rollout_triplets(
    state: web::Data<AppState>,
    path: web::Path<String>,
    _req: HttpRequest,
) -> impl Responder {
    let rollout_id = path.into_inner();
    let triplets = state.telemetry_store.get_triplets(&rollout_id);
    HttpResponse::Ok().json(triplets)
}

#[derive(Deserialize)]
struct RewardStatsQuery {
    since_hours: Option<u64>,
}

async fn get_reward_stats(
    state: web::Data<AppState>,
    query: web::Query<RewardStatsQuery>,
    _req: HttpRequest,
) -> impl Responder {
    let since = query.since_hours.map(|hours| {
        chrono::Utc::now() - chrono::Duration::hours(hours as i64)
    });
    let stats = state.telemetry_store.get_reward_stats(since);
    HttpResponse::Ok().json(stats)
}

async fn list_resources(
    state: web::Data<AppState>,
    _req: HttpRequest,
) -> impl Responder {
    let versions = state.resource_manager.list_versions();
    HttpResponse::Ok().json(versions)
}

#[derive(Deserialize)]
struct CreateResourceRequest {
    label: String,
    description: Option<String>,
    resources: Vec<ResourceInput>,
}

#[derive(Deserialize)]
struct ResourceInput {
    name: String,
    resource_type: String,
    content: String,
}

async fn create_resource(
    state: web::Data<AppState>,
    body: web::Json<CreateResourceRequest>,
    _req: HttpRequest,
) -> impl Responder {
    let resources: Vec<Resource> = body.resources.iter().map(|r| {
        Resource {
            name: r.name.clone(),
            resource_type: ResourceType::from_str(&r.resource_type)
                .unwrap_or(ResourceType::PromptTemplate),
            content: r.content.clone(),
            metadata: serde_json::Value::Null,
        }
    }).collect();

    match state.resource_manager.create_version(
        body.label.clone(),
        resources,
        body.description.clone(),
    ) {
        Ok(bundle) => {
            // Activate the new version
            if let Err(e) = state.resource_manager.activate_version(&bundle.version_id) {
                return HttpResponse::InternalServerError().json(ErrorResponse {
                    error: format!("Created but failed to activate: {}", e),
                });
            }
            HttpResponse::Ok().json(bundle)
        }
        Err(e) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: e,
        }),
    }
}

async fn rollback_resource(
    state: web::Data<AppState>,
    path: web::Path<String>,
    _req: HttpRequest,
) -> impl Responder {
    let version_id = path.into_inner();
    match state.resource_manager.rollback(&version_id) {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({
            "status": "ok",
            "activated_version": version_id,
        })),
        Err(e) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: e,
        }),
    }
}
