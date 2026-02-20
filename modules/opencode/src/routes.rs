//! Axum route handlers for the opencode module RPC API.

use crate::opencode_client::OpenCodeClient;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use opencode_types::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct AppState {
    pub oc_client: OpenCodeClient,
    pub start_time: Instant,
    pub opencode_port: u16,
    pub prompt_count: AtomicU64,
}

// POST /rpc/prompt
pub async fn prompt(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PromptRequest>,
) -> (StatusCode, Json<RpcResponse<PromptResult>>) {
    if req.task.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RpcResponse::err("task is empty")),
        );
    }

    // Create a fresh session for each prompt
    let session = match state.oc_client.create_session().await {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_GATEWAY, Json(RpcResponse::err(e))),
    };

    // Optionally prefix with project context
    let full_prompt = match &req.project_path {
        Some(path) => format!(
            "Working directory: {}\n\n{}",
            path, req.task
        ),
        None => req.task.clone(),
    };

    let response = match state.oc_client.prompt(&session.id, &full_prompt).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(RpcResponse::err(format!("OpenCode prompt failed: {}", e))),
            )
        }
    };

    state.prompt_count.fetch_add(1, Ordering::Relaxed);

    let result = PromptResult {
        session_id: session.id,
        response,
    };

    (StatusCode::OK, Json(RpcResponse::ok(result)))
}

// GET /rpc/sessions
pub async fn sessions(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<Vec<SessionInfo>>>) {
    match state.oc_client.list_sessions().await {
        Ok(sessions) => {
            let infos: Vec<SessionInfo> = sessions
                .into_iter()
                .map(|s| SessionInfo {
                    id: s.id,
                    title: s.title,
                })
                .collect();
            (StatusCode::OK, Json(RpcResponse::ok(infos)))
        }
        Err(e) => (StatusCode::BAD_GATEWAY, Json(RpcResponse::err(e))),
    }
}

// GET /rpc/status
pub async fn status(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<RpcResponse<ServiceStatus>>) {
    let opencode_healthy = state.oc_client.health().await.unwrap_or(false);

    let status = ServiceStatus {
        running: true,
        uptime_secs: state.start_time.elapsed().as_secs(),
        opencode_healthy,
        opencode_port: state.opencode_port,
        total_prompts: state.prompt_count.load(Ordering::Relaxed),
    };

    (StatusCode::OK, Json(RpcResponse::ok(status)))
}
