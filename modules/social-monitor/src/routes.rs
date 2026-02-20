//! Axum route handlers for the social monitor RPC API.

use crate::db::Db;
use crate::forensics;
use crate::twitter_api;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use social_monitor_types::*;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

pub struct AppState {
    pub db: Arc<Db>,
    pub start_time: Instant,
    pub last_tick_at: Arc<Mutex<Option<String>>>,
    pub poll_interval_secs: u64,
}

// =====================================================
// Account Endpoints
// =====================================================

// POST /rpc/accounts/add
pub async fn accounts_add(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddAccountRequest>,
) -> (StatusCode, Json<RpcResponse<MonitoredAccount>>) {
    let username = req.username.trim_start_matches('@').to_string();

    // Look up the user via Twitter API to get their ID
    let credentials = match twitter_api::TwitterCredentials::from_env() {
        Some(c) => c,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(RpcResponse::err(
                    "Twitter credentials not configured",
                )),
            )
        }
    };

    let client = reqwest::Client::new();
    let user = match twitter_api::lookup_user_by_username(&client, &credentials, &username).await {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RpcResponse::err(format!(
                    "Failed to look up @{}: {}",
                    username, e
                ))),
            )
        }
    };

    match state.db.add_account(
        &user.id,
        &user.username,
        Some(&user.name),
        req.notes.as_deref(),
        req.custom_keywords.as_deref(),
    ) {
        Ok(entry) => (StatusCode::OK, Json(RpcResponse::ok(entry))),
        Err(e) => {
            let msg = if e.to_string().contains("UNIQUE constraint") {
                format!("@{} is already being monitored", username)
            } else {
                format!("Failed to add account: {}", e)
            };
            (StatusCode::BAD_REQUEST, Json(RpcResponse::err(msg)))
        }
    }
}

// POST /rpc/accounts/remove
pub async fn accounts_remove(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoveAccountRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.remove_account(req.id) {
        Ok(true) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(RpcResponse::err(format!("Account #{} not found", req.id))),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to remove: {}", e))),
        ),
    }
}

// GET /rpc/accounts/list
pub async fn accounts_list(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<MonitoredAccount>>>) {
    match state.db.list_accounts() {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to list: {}", e))),
        ),
    }
}

// POST /rpc/accounts/update
pub async fn accounts_update(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateAccountRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.update_account(
        req.id,
        req.monitor_enabled,
        req.custom_keywords.as_deref(),
        req.notes.as_deref(),
    ) {
        Ok(true) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(RpcResponse::err(format!("Account #{} not found", req.id))),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to update: {}", e))),
        ),
    }
}

// =====================================================
// Keyword Endpoints
// =====================================================

// POST /rpc/keywords/add
pub async fn keywords_add(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddKeywordRequest>,
) -> (StatusCode, Json<RpcResponse<TrackedKeyword>>) {
    let aliases_json = req
        .aliases
        .as_ref()
        .and_then(|a| serde_json::to_string(a).ok());

    match state
        .db
        .add_keyword(&req.keyword, req.category.as_deref(), aliases_json.as_deref())
    {
        Ok(entry) => (StatusCode::OK, Json(RpcResponse::ok(entry))),
        Err(e) => {
            let msg = if e.to_string().contains("UNIQUE constraint") {
                format!("Keyword '{}' already tracked", req.keyword)
            } else {
                format!("Failed to add keyword: {}", e)
            };
            (StatusCode::BAD_REQUEST, Json(RpcResponse::err(msg)))
        }
    }
}

// POST /rpc/keywords/remove
pub async fn keywords_remove(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoveKeywordRequest>,
) -> (StatusCode, Json<RpcResponse<bool>>) {
    match state.db.remove_keyword(req.id) {
        Ok(true) => (StatusCode::OK, Json(RpcResponse::ok(true))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(RpcResponse::err(format!("Keyword #{} not found", req.id))),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to remove: {}", e))),
        ),
    }
}

// GET /rpc/keywords/list
pub async fn keywords_list(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<TrackedKeyword>>>) {
    match state.db.list_keywords() {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Failed to list: {}", e))),
        ),
    }
}

// =====================================================
// Tweet Endpoints
// =====================================================

// POST /rpc/tweets/query
pub async fn tweets_query(
    State(state): State<Arc<AppState>>,
    Json(filter): Json<TweetFilter>,
) -> (StatusCode, Json<RpcResponse<Vec<CapturedTweet>>>) {
    match state.db.query_tweets(&filter) {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Query failed: {}", e))),
        ),
    }
}

// GET /rpc/tweets/stats
pub async fn tweets_stats(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<TweetStats>>) {
    match state.db.get_tweet_stats() {
        Ok(stats) => (StatusCode::OK, Json(RpcResponse::ok(stats))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Stats query failed: {}", e))),
        ),
    }
}

// =====================================================
// Forensics Endpoints
// =====================================================

// POST /rpc/topics/query
pub async fn topics_query(
    State(state): State<Arc<AppState>>,
    Json(filter): Json<TopicFilter>,
) -> (StatusCode, Json<RpcResponse<Vec<TopicScore>>>) {
    match state.db.query_topic_scores(&filter) {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Query failed: {}", e))),
        ),
    }
}

// POST /rpc/sentiment/query
pub async fn sentiment_query(
    State(state): State<Arc<AppState>>,
    Json(filter): Json<SentimentFilter>,
) -> (StatusCode, Json<RpcResponse<Vec<SentimentSnapshot>>>) {
    match state.db.query_sentiment(&filter) {
        Ok(entries) => (StatusCode::OK, Json(RpcResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Query failed: {}", e))),
        ),
    }
}

// POST /rpc/forensics/report
pub async fn forensics_report(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ForensicsReportRequest>,
) -> (StatusCode, Json<RpcResponse<AccountForensicsReport>>) {
    let account = if let Some(id) = req.account_id {
        state.db.get_account_by_id(id).ok().flatten()
    } else if let Some(ref username) = req.username {
        state.db.get_account_by_username(username).ok().flatten()
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(RpcResponse::err(
                "Either account_id or username is required",
            )),
        );
    };

    let account = match account {
        Some(a) => a,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(RpcResponse::err("Account not found")),
            )
        }
    };

    let report = forensics::generate_report(&state.db, &account);
    (StatusCode::OK, Json(RpcResponse::ok(report)))
}

// =====================================================
// Service Endpoints
// =====================================================

// GET /rpc/status
pub async fn status(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<ServiceStatus>>) {
    let stats = state.db.get_tweet_stats().ok();
    let last_tick = state.last_tick_at.lock().await.clone();

    let status = ServiceStatus {
        running: true,
        uptime_secs: state.start_time.elapsed().as_secs(),
        monitored_accounts: stats.as_ref().map(|s| s.monitored_accounts).unwrap_or(0),
        active_accounts: stats.as_ref().map(|s| s.active_accounts).unwrap_or(0),
        total_tweets: stats.as_ref().map(|s| s.total_tweets).unwrap_or(0),
        unique_topics: stats.as_ref().map(|s| s.unique_topics).unwrap_or(0),
        last_tick_at: last_tick,
        poll_interval_secs: state.poll_interval_secs,
    };

    (StatusCode::OK, Json(RpcResponse::ok(status)))
}

// POST /rpc/backup/export
pub async fn backup_export(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<BackupData>>) {
    match state.db.export_for_backup() {
        Ok(data) => (StatusCode::OK, Json(RpcResponse::ok(data))),
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
    match state.db.clear_and_restore(&req.data) {
        Ok(count) => (StatusCode::OK, Json(RpcResponse::ok(count))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RpcResponse::err(format!("Backup restore failed: {}", e))),
        ),
    }
}
