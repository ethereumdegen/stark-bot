//! Discord Tipping Service — standalone binary for managing user profiles and tipping.
//!
//! Hosts both an RPC API and a dashboard UI.
//! Default: http://127.0.0.1:9101/

mod db;
mod dashboard;
mod routes;

use routes::AppState;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    let port: u16 = std::env::var("DISCORD_TIPPING_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9101);

    let db_path = std::env::var("DISCORD_TIPPING_DB_PATH")
        .unwrap_or_else(|_| "./discord_tipping.db".to_string());

    log::info!("Opening database at: {}", db_path);
    let database = Arc::new(
        db::Db::open(&db_path).expect("Failed to open database"),
    );

    let state = Arc::new(AppState {
        db: database,
        start_time: Instant::now(),
    });

    let cors = tower_http::cors::CorsLayer::permissive();

    let app = axum::Router::new()
        .route("/", axum::routing::get(dashboard::dashboard))
        .route("/rpc/profile/get_or_create", axum::routing::post(routes::get_or_create))
        .route("/rpc/profile/get", axum::routing::post(routes::get_profile))
        .route("/rpc/profile/get_by_address", axum::routing::post(routes::get_by_address))
        .route("/rpc/profile/register", axum::routing::post(routes::register_address))
        .route("/rpc/profile/unregister", axum::routing::post(routes::unregister_address))
        .route("/rpc/profiles/all", axum::routing::get(routes::list_all))
        .route("/rpc/profiles/registered", axum::routing::get(routes::list_registered))
        .route("/rpc/stats", axum::routing::get(routes::stats))
        .route("/rpc/status", axum::routing::get(routes::status))
        .route("/rpc/backup/export", axum::routing::post(routes::backup_export))
        .route("/rpc/backup/restore", axum::routing::post(routes::backup_restore))
        .with_state(state)
        .layer(cors);

    let addr = format!("127.0.0.1:{}", port);
    log::info!("Discord Tipping Service listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
