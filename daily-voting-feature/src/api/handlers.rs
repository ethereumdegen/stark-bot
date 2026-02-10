use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use crate::models::*;
use crate::AppState;

// Helper for error responses
fn internal_error(err: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("Internal error: {}", err);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": err.to_string()
        }))
    )
}

fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": msg
        }))
    )
}

fn not_found(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": msg
        }))
    )
}

// ============ Health Check ============

pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let db_status = match sqlx::query("SELECT 1").execute(&state.db_pool).await {
        Ok(_) => "healthy",
        Err(_) => "unhealthy",
    };

    let blockchain_status = match state.blockchain_service.health_check().await {
        Ok(_) => "healthy",
        Err(_) => "unhealthy",
    };

    let current_cycle = state.voting_service.get_or_create_current_cycle().await
        .map(|c| c.id)
        .unwrap_or(0);

    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        current_cycle,
        database: db_status.to_string(),
        blockchain: blockchain_status.to_string(),
    })
}

// ============ Cycle Endpoints ============

pub async fn get_current_cycle(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CycleResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.get_current_cycle_response().await
        .map(Json)
        .map_err(internal_error)
}

pub async fn get_cycle(
    State(state): State<Arc<AppState>>,
    Path(cycle_id): Path<i64>,
) -> Result<Json<CycleResponse>, (StatusCode, Json<serde_json::Value>)> {
    match state.voting_service.get_cycle(cycle_id).await {
        Ok(Some(cycle)) => Ok(Json(cycle)),
        Ok(None) => Err(not_found("Cycle not found")),
        Err(e) => Err(internal_error(e)),
    }
}

pub async fn finalize_cycle(
    State(state): State<Arc<AppState>>,
    Path(cycle_id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.finalize_cycle(cycle_id).await
        .map(|_| Json(serde_json::json!({
            "success": true,
            "message": format!("Cycle {} finalized", cycle_id)
        })))
        .map_err(|e| bad_request(&e.to_string()))
}

// ============ Post Endpoints ============

pub async fn get_posts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Vec<PostResponse>>, (StatusCode, Json<serde_json::Value>)> {
    // For now, get current cycle posts
    state.voting_service.get_cycle_posts(None).await
        .map(Json)
        .map_err(internal_error)
}

pub async fn register_post(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RegisterPostRequest>,
) -> Result<Json<PostResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.register_post(request).await
        .map(Json)
        .map_err(|e| bad_request(&e.to_string()))
}

pub async fn get_post(
    State(state): State<Arc<AppState>>,
    Path(post_id): Path<String>,
) -> Result<Json<PostResponse>, (StatusCode, Json<serde_json::Value>)> {
    match state.voting_service.get_post(&post_id).await {
        Ok(Some(post)) => Ok(Json(post)),
        Ok(None) => Err(not_found("Post not found")),
        Err(e) => Err(internal_error(e)),
    }
}

// ============ Voting Endpoints ============

pub async fn cast_vote(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CastVoteRequest>,
) -> Result<Json<VoteResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.cast_vote(request).await
        .map(Json)
        .map_err(|e| bad_request(&e.to_string()))
}

pub async fn get_user_votes(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
    Query(params): Query<LeaderboardParams>,
) -> Result<Json<UserVotesResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.get_user_votes(&address, params.cycle_id).await
        .map(Json)
        .map_err(internal_error)
}

// ============ Leaderboard Endpoints ============

pub async fn get_leaderboard(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LeaderboardParams>,
) -> Result<Json<LeaderboardResponse>, (StatusCode, Json<serde_json::Value>)> {
    let limit = params.limit.unwrap_or(10);
    state.voting_service.get_leaderboard(params.cycle_id, limit).await
        .map(Json)
        .map_err(internal_error)
}

pub async fn get_cycle_leaderboard(
    State(state): State<Arc<AppState>>,
    Path(cycle_id): Path<i64>,
    Query(params): Query<LeaderboardParams>,
) -> Result<Json<LeaderboardResponse>, (StatusCode, Json<serde_json::Value>)> {
    let limit = params.limit.unwrap_or(10);
    state.voting_service.get_leaderboard(Some(cycle_id), limit).await
        .map(Json)
        .map_err(internal_error)
}

// ============ Rewards Endpoints ============

pub async fn get_user_rewards(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Result<Json<RewardsResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.get_user_rewards(&address).await
        .map(Json)
        .map_err(internal_error)
}

pub async fn claim_reward(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ClaimRewardRequest>,
) -> Result<Json<VoteResponse>, (StatusCode, Json<serde_json::Value>)> {
    state.voting_service.claim_reward(request).await
        .map(Json)
        .map_err(|e| bad_request(&e.to_string()))
}
