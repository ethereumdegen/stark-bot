//! OpenCode Module Service — thin wrapper around `opencode serve`.
//!
//! Spawns `opencode serve` as a child process and exposes StarkBot-compatible
//! RPC endpoints that forward coding tasks to the OpenCode AI agent.
//!
//! Default: http://127.0.0.1:9103/

mod opencode_client;
mod routes;

use opencode_client::OpenCodeClient;
use routes::AppState;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    env_logger::init();

    let port: u16 = std::env::var("OPENCODE_MODULE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9103);

    let oc_port: u16 = std::env::var("OPENCODE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4096);

    let oc_host = std::env::var("OPENCODE_HOST")
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let project_dir = std::env::var("OPENCODE_PROJECT_DIR")
        .unwrap_or_else(|_| ".".to_string());

    let opencode_url = format!("http://{}:{}", oc_host, oc_port);

    // Spawn `opencode serve` as a child process
    log::info!(
        "Spawning opencode serve on {}:{} (project: {})",
        oc_host,
        oc_port,
        project_dir,
    );

    let mut cmd = tokio::process::Command::new("opencode");
    cmd.arg("serve")
        .arg("--port")
        .arg(oc_port.to_string())
        .arg("--hostname")
        .arg(&oc_host)
        .current_dir(&project_dir);

    if let Ok(pw) = std::env::var("OPENCODE_SERVER_PASSWORD") {
        cmd.env("OPENCODE_SERVER_PASSWORD", pw);
    }

    let child = cmd.spawn();

    match child {
        Ok(_child) => {
            log::info!("opencode serve spawned (pid managed by tokio)");
        }
        Err(e) => {
            log::error!("Failed to spawn opencode serve: {}", e);
            log::error!("Make sure `opencode` is installed and in PATH");
            std::process::exit(1);
        }
    }

    // Wait for opencode to become healthy
    let oc_client = OpenCodeClient::new(&opencode_url);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if oc_client.health().await.unwrap_or(false) {
            log::info!("opencode serve is healthy");
            break;
        }
        if Instant::now() > deadline {
            log::error!("opencode serve did not become healthy within 30s");
            std::process::exit(1);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let state = Arc::new(AppState {
        oc_client,
        start_time: Instant::now(),
        opencode_port: oc_port,
        prompt_count: AtomicU64::new(0),
    });

    let cors = tower_http::cors::CorsLayer::permissive();

    let app = axum::Router::new()
        .route("/rpc/prompt", axum::routing::post(routes::prompt))
        .route("/rpc/sessions", axum::routing::get(routes::sessions))
        .route("/rpc/status", axum::routing::get(routes::status))
        .with_state(state)
        .layer(cors);

    let addr = format!("127.0.0.1:{}", port);
    log::info!("OpenCode Module Service listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}
