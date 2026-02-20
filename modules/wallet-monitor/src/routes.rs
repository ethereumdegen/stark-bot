//! Axum route handlers for the wallet monitor RPC API.

use crate::db::Db;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use wallet_monitor_types::*;

pub struct AppState {
    pub db: Arc<Db>,
    pub start_time: Instant,
    pub last_tick_at: Arc<Mutex<Option<String>>>,
    pub poll_interval_secs: u64,
    pub worker_enabled: bool,
    /// Masked API key for display (e.g. "abc...xyz"), None if not configured
    pub alchemy_key_preview: Option<String>,
}

// POST /rpc/watchlist/add
pub async fn watchlist_add(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddWalletRequest>,
) -> (StatusCode, Json<RpcResponse<WatchlistEntry>>) {
    let chain = req.chain.as_deref().unwrap_or("mainnet");
    let threshold = req.threshold_usd.unwrap_or(1000.0);

    if !is_valid_eth_address(&req.address) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RpcResponse::err("Invalid Ethereum address")),
        );
    }

    match state.db.add_to_watchlist(&req.address, req.label.as_deref(), chain, threshold) {
        Ok(entry) => (StatusCode::OK, Json(RpcResponse::ok(entry))),
        Err(e) => {
            let msg = if e.to_string().contains("UNIQUE constraint") {
                format!("Wallet {} already on watchlist for chain {}", req.address, chain)
            } else {
                format!("Failed to add wallet: {}", e)
            };
            (StatusCode::BAD_REQUEST, Json(RpcResponse::err(msg)))
        }
    }
}

// POST /rpc/watchlist/remove
pub async fn watchlist_remove(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoveWalletRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.remove_from_watchlist(req.id) {
        Ok(true) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(RpcResponse::err(format!("Entry #{} not found", req.id))),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to remove: {}", e))),
        ),
    }
}

// GET /rpc/watchlist/list
pub async fn watchlist_list(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<WatchlistEntry>>>) {
    match state.db.list_watchlist() {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to list: {}", e))),
        ),
    }
}

// POST /rpc/watchlist/update
pub async fn watchlist_update(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateWalletRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.update_watchlist_entry(
        req.id,
        req.label.as_deref(),
        req.threshold_usd,
        req.monitor_enabled,
        req.notes.as_deref(),
    ) {
        Ok(true) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(RpcResponse::err(format!("Entry #{} not found", req.id))),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to update: {}", e))),
        ),
    }
}

// POST /rpc/activity/query
pub async fn activity_query(
    State(state): State<Arc<AppState>>,
    Json(filter): Json<ActivityFilter>,
) -> (StatusCode, Json<RpcResponse<Vec<ActivityEntry>>>) {
    match state.db.query_activity(&filter) {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Query failed: {}", e))),
        ),
    }
}

// GET /rpc/activity/stats
pub async fn activity_stats(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<ActivityStats>>) {
    match state.db.get_activity_stats() {
        Ok(stats) => (StatusCode::OK, Json(RpcResponse::ok(stats))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Stats query failed: {}", e))),
        ),
    }
}

// GET /rpc/status
pub async fn status(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<ServiceStatus>>) {
    let stats = state.db.get_activity_stats().ok();
    let last_tick = state.last_tick_at.lock().await.clone();

    let status = ServiceStatus {
        running: true,
        uptime_secs: state.start_time.elapsed().as_secs(),
        watched_wallets: stats.as_ref().map(|s| s.watched_wallets).unwrap_or(0),
        active_wallets: stats.as_ref().map(|s| s.active_wallets).unwrap_or(0),
        total_transactions: stats.as_ref().map(|s| s.total_transactions).unwrap_or(0),
        large_trades: stats.as_ref().map(|s| s.large_trades).unwrap_or(0),
        last_tick_at: last_tick,
        poll_interval_secs: state.poll_interval_secs,
        worker_enabled: state.worker_enabled,
    };

    (StatusCode::OK, Json(RpcResponse::ok(status)))
}

// POST /rpc/backup/export
pub async fn backup_export(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<BackupEntry>>>) {
    match state.db.export_watchlist_for_backup() {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Backup export failed: {}", e))),
        ),
    }
}

// POST /rpc/backup/restore
pub async fn backup_restore(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackupRestoreRequest>,
) -> (StatusCode, Json<RpcResponse<usize>>) {
    match state.db.clear_and_restore_watchlist(&req.wallets) {
        Ok(count) => (StatusCode::OK, Json(RpcResponse::ok(count))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Backup restore failed: {}", e))),
        ),
    }
}

fn is_valid_eth_address(addr: &str) -> bool {
    addr.starts_with("0x") && addr.len() == 42 && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}
