use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod db;
mod models;
mod services;

use crate::api::handlers;
use crate::services::{blockchain::BlockchainService, voting::VotingService};

pub struct AppState {
    pub voting_service: VotingService,
    pub blockchain_service: BlockchainService,
    pub db_pool: sqlx::PgPool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daily_voting=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Daily Voting Service for x402book");

    // Database connection
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/x402book".to_string());
    
    let db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    tracing::info!("Connected to database");

    // Run migrations
    sqlx::migrate!("./migrations").run(&db_pool).await?;
    tracing::info!("Migrations complete");

    // Initialize services
    let rpc_url = std::env::var("BASE_RPC_URL")
        .unwrap_or_else(|_| "https://mainnet.base.org".to_string());
    
    let contract_address = std::env::var("VOTING_CONTRACT_ADDRESS")
        .expect("VOTING_CONTRACT_ADDRESS must be set");
    
    let token_address = std::env::var("TOKEN_ADDRESS")
        .unwrap_or_else(|_| "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07".to_string());

    let blockchain_service = BlockchainService::new(
        &rpc_url,
        &contract_address,
        &token_address,
    ).await?;

    let voting_service = VotingService::new(db_pool.clone(), blockchain_service.clone());

    let app_state = Arc::new(AppState {
        voting_service,
        blockchain_service,
        db_pool,
    });

    // Build router
    let app = Router::new()
        // Cycle endpoints
        .route("/api/v1/cycle/current", get(handlers::get_current_cycle))
        .route("/api/v1/cycle/:cycle_id", get(handlers::get_cycle))
        .route("/api/v1/cycle/:cycle_id/finalize", post(handlers::finalize_cycle))
        
        // Post endpoints
        .route("/api/v1/posts", get(handlers::get_posts))
        .route("/api/v1/posts/register", post(handlers::register_post))
        .route("/api/v1/posts/:post_id", get(handlers::get_post))
        
        // Voting endpoints
        .route("/api/v1/vote", post(handlers::cast_vote))
        .route("/api/v1/votes/user/:address", get(handlers::get_user_votes))
        
        // Leaderboard
        .route("/api/v1/leaderboard", get(handlers::get_leaderboard))
        .route("/api/v1/leaderboard/:cycle_id", get(handlers::get_cycle_leaderboard))
        
        // Rewards
        .route("/api/v1/rewards/:address", get(handlers::get_user_rewards))
        .route("/api/v1/rewards/claim", post(handlers::claim_reward))
        
        // Health check
        .route("/health", get(handlers::health_check))
        
        // Add middleware
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(app_state);

    // Start background tasks
    tokio::spawn(cycle_finalizer_task());

    // Start server
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    tracing::info!("Listening on {}", addr);
    
    axum::serve(listener, app).await?;

    Ok(())
}

/// Background task to automatically finalize cycles
async fn cycle_finalizer_task() {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
    
    loop {
        interval.tick().await;
        // Check if current cycle needs finalizing
        // This would call the blockchain service to check and finalize
        tracing::debug!("Checking for cycles to finalize...");
    }
}
