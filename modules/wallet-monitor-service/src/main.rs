//! Wallet Monitor Service — standalone binary for monitoring wallet activity.
//!
//! Hosts both an RPC API and a dashboard UI on the same port.
//! Default: http://127.0.0.1:9100/

mod alchemy;
mod db;
mod dashboard;
mod routes;
mod worker;

use routes::AppState;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    let port: u16 = std::env::var("WALLET_MONITOR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9100);

    let db_path = std::env::var("WALLET_MONITOR_DB_PATH")
        .unwrap_or_else(|_| "./wallet_monitor.db".to_string());

    let api_key = std::env::var("ALCHEMY_API_KEY").unwrap_or_default();

    let alert_callback_url = std::env::var("ALERT_CALLBACK_URL").ok();

    let poll_interval_secs: u64 = std::env::var("WALLET_MONITOR_POLL_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    log::info!("Opening database at: {}", db_path);
    let database = Arc::new(
        db::Db::open(&db_path).expect("Failed to open database"),
    );

    let last_tick_at: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let worker_enabled = !api_key.is_empty();

    let state = Arc::new(AppState {
        db: database.clone(),
        start_time: Instant::now(),
        last_tick_at: last_tick_at.clone(),
        poll_interval_secs,
        worker_enabled,
    });

    // Spawn background worker if API key is configured
    if worker_enabled {
        let worker_db = database.clone();
        let worker_last_tick = last_tick_at.clone();
        tokio::spawn(async move {
            worker::run_worker(
                worker_db,
                api_key,
                poll_interval_secs,
                alert_callback_url,
                worker_last_tick,
            )
            .await;
        });
        log::info!("Background worker started (poll interval: {}s)", poll_interval_secs);
    } else {
        log::warn!("ALCHEMY_API_KEY not set — background worker disabled");
    }

    let cors = tower_http::cors::CorsLayer::permissive();

    let app = axum::Router::new()
        .route("/", axum::routing::get(dashboard::dashboard))
        .route("/rpc/watchlist/add", axum::routing::post(routes::watchlist_add))
        .route("/rpc/watchlist/remove", axum::routing::post(routes::watchlist_remove))
        .route("/rpc/watchlist/list", axum::routing::get(routes::watchlist_list))
        .route("/rpc/watchlist/update", axum::routing::post(routes::watchlist_update))
        .route("/rpc/activity/query", axum::routing::post(routes::activity_query))
        .route("/rpc/activity/stats", axum::routing::get(routes::activity_stats))
        .route("/rpc/status", axum::routing::get(routes::status))
        .route("/rpc/backup/export", axum::routing::post(routes::backup_export))
        .route("/rpc/backup/restore", axum::routing::post(routes::backup_restore))
        .with_state(state)
        .layer(cors);

    let addr = format!("127.0.0.1:{}", port);
    log::info!("Wallet Monitor Service listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
