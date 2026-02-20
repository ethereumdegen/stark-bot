//! Axum route handlers for the discord tipping RPC API.

use crate::db::Db;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use discord_tipping_types::*;
use std::sync::Arc;
use std::time::Instant;

pub struct AppState {
    pub db: Arc<Db>,
    pub start_time: Instant,
}

// POST /rpc/profile/get_or_create
pub async fn get_or_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GetOrCreateProfileRequest>,
) -> (StatusCode, Json<RpcResponse<DiscordUserProfile>>) {
    match state.db.get_or_create_profile(&req.discord_user_id, &req.username) {
        Ok(p) => (StatusCode::OK, Json(RpcResponse::ok(p))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// POST /rpc/profile/get
pub async fn get_profile(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GetProfileRequest>,
) -> (StatusCode, Json<RpcResponse<Option<DiscordUserProfile>>>) {
    match state.db.get_profile(&req.discord_user_id) {
        Ok(p) => (StatusCode::OK, Json(RpcResponse::ok(p))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// POST /rpc/profile/get_by_address
pub async fn get_by_address(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GetProfileByAddressRequest>,
) -> (StatusCode, Json<RpcResponse<Option<DiscordUserProfile>>>) {
    match state.db.get_profile_by_address(&req.address) {
        Ok(p) => (StatusCode::OK, Json(RpcResponse::ok(p))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// POST /rpc/profile/register
pub async fn register_address(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterAddressRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.register_address(&req.discord_user_id, &req.address) {
        Ok(()) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// POST /rpc/profile/unregister
pub async fn unregister_address(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UnregisterAddressRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.unregister_address(&req.discord_user_id) {
        Ok(()) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// GET /rpc/profiles/all
pub async fn list_all(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<DiscordUserProfile>>>) {
    match state.db.list_all_profiles() {
        Ok(p) => (StatusCode::OK, Json(RpcResponse::ok(p))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// GET /rpc/profiles/registered
pub async fn list_registered(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<DiscordUserProfile>>>) {
    match state.db.list_registered_profiles() {
        Ok(p) => (StatusCode::OK, Json(RpcResponse::ok(p))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// GET /rpc/stats
pub async fn stats(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<ProfileStats>>) {
    match state.db.get_stats() {
        Ok(s) => (StatusCode::OK, Json(RpcResponse::ok(s))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// GET /rpc/status
pub async fn status(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<ServiceStatus>>) {
    let stats = state.db.get_stats().ok();
    (
        StatusCode::OK,
        Json(RpcResponse::ok(ServiceStatus {
            running: true,
            uptime_secs: state.start_time.elapsed().as_secs(),
            total_profiles: stats.as_ref().map(|s| s.total_profiles).unwrap_or(0),
            registered_count: stats.as_ref().map(|s| s.registered_count).unwrap_or(0),
        })),
    )
}

// POST /rpc/backup/export
pub async fn backup_export(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<BackupEntry>>>) {
    match state.db.list_registered_profiles() {
        Ok(profiles) => {
            let entries: Vec<BackupEntry> = profiles
                .into_iter()
                .filter_map(|p| {
                    p.public_address.map(|addr| BackupEntry {
                        discord_user_id: p.discord_user_id,
                        discord_username: p.discord_username,
                        public_address: addr,
                        registered_at: p.registered_at,
                    })
                })
                .collect();
            (StatusCode::OK, Json(RpcResponse::ok(entries)))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}

// POST /rpc/backup/restore
pub async fn backup_restore(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackupRestoreRequest>,
) -> (StatusCode, Json<RpcResponse<usize>>) {
    match state.db.clear_and_restore(&req.profiles) {
        Ok(count) => (StatusCode::OK, Json(RpcResponse::ok(count))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(RpcResponse::err(e))),
    }
}
