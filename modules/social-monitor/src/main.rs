//! Social Monitor Service — standalone binary for monitoring Twitter/X accounts.
//!
//! Hosts both an RPC API and a dashboard UI on the same port.
//! Default: http://127.0.0.1:9102/

mod dashboard;
mod db;
mod forensics;
mod routes;
mod twitter_api;
mod worker;

use routes::AppState;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    let port: u16 = std::env::var("SOCIAL_MONITOR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9102);

    let db_path = std::env::var("SOCIAL_MONITOR_DB_PATH")
        .unwrap_or_else(|_| "./social_monitor.db".to_string());

    let poll_interval_secs: u64 = std::env::var("SOCIAL_MONITOR_POLL_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300); // 5 minutes default

    let has_twitter_creds = std::env::var("TWITTER_CONSUMER_KEY").is_ok()
        && std::env::var("TWITTER_CONSUMER_SECRET").is_ok()
        && std::env::var("TWITTER_ACCESS_TOKEN").is_ok()
        && std::env::var("TWITTER_ACCESS_TOKEN_SECRET").is_ok();

    log::info!("Opening database at: {}", db_path);
    let database = Arc::new(db::Db::open(&db_path).expect("Failed to open database"));

    let last_tick_at: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let state = Arc::new(AppState {
        db: database.clone(),
        start_time: Instant::now(),
        last_tick_at: last_tick_at.clone(),
        poll_interval_secs,
    });

    // Spawn background worker if Twitter credentials are configured
    if has_twitter_creds {
        let worker_db = database.clone();
        let worker_last_tick = last_tick_at.clone();
        tokio::spawn(async move {
            worker::run_worker(worker_db, poll_interval_secs, worker_last_tick).await;
        });
        log::info!(
            "Background worker started (poll interval: {}s)",
            poll_interval_secs
        );
    } else {
        log::warn!("Twitter credentials not set — background worker disabled");
    }

    let cors = tower_http::cors::CorsLayer::permissive();

    let app = axum::Router::new()
        .route("/", axum::routing::get(dashboard::dashboard))
        // Account management
        .route(
            "/rpc/accounts/add",
            axum::routing::post(routes::accounts_add),
        )
        .route(
            "/rpc/accounts/remove",
            axum::routing::post(routes::accounts_remove),
        )
        .route(
            "/rpc/accounts/list",
            axum::routing::get(routes::accounts_list),
        )
        .route(
            "/rpc/accounts/update",
            axum::routing::post(routes::accounts_update),
        )
        // Keyword management
        .route(
            "/rpc/keywords/add",
            axum::routing::post(routes::keywords_add),
        )
        .route(
            "/rpc/keywords/remove",
            axum::routing::post(routes::keywords_remove),
        )
        .route(
            "/rpc/keywords/list",
            axum::routing::get(routes::keywords_list),
        )
        // Tweet queries
        .route(
            "/rpc/tweets/query",
            axum::routing::post(routes::tweets_query),
        )
        .route(
            "/rpc/tweets/stats",
            axum::routing::get(routes::tweets_stats),
        )
        // Forensics
        .route(
            "/rpc/topics/query",
            axum::routing::post(routes::topics_query),
        )
        .route(
            "/rpc/sentiment/query",
            axum::routing::post(routes::sentiment_query),
        )
        .route(
            "/rpc/forensics/report",
            axum::routing::post(routes::forensics_report),
        )
        // Service
        .route("/rpc/status", axum::routing::get(routes::status))
        .route(
            "/rpc/backup/export",
            axum::routing::post(routes::backup_export),
        )
        .route(
            "/rpc/backup/restore",
            axum::routing::post(routes::backup_restore),
        )
        .with_state(state)
        .layer(cors);

    let addr = format!("127.0.0.1:{}", port);
    log::info!("Social Monitor Service listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}
