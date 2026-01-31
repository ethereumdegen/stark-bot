use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use std::sync::Arc;

mod ai;
mod channels;
mod config;
mod context;
mod controllers;
mod db;
mod domain_types;
mod execution;
mod gateway;
mod integrations;
mod memory;
mod middleware;
mod models;
mod scheduler;
mod skills;
mod tools;
mod x402;
mod eip8004;
mod hooks;

use channels::MessageDispatcher;
use config::Config;
use db::Database;
use execution::ExecutionTracker;
use gateway::Gateway;
use scheduler::{Scheduler, SchedulerConfig};
use skills::SkillRegistry;
use tools::ToolRegistry;

pub struct AppState {
    pub db: Arc<Database>,
    pub config: Config,
    pub gateway: Arc<Gateway>,
    pub tool_registry: Arc<ToolRegistry>,
    pub skill_registry: Arc<SkillRegistry>,
    pub dispatcher: Arc<MessageDispatcher>,
    pub execution_tracker: Arc<ExecutionTracker>,
    pub scheduler: Arc<Scheduler>,
}

/// SPA fallback handler - serves index.html for client-side routing
async fn spa_fallback() -> actix_web::Result<NamedFile> {
    // Check both possible locations for frontend dist
    if std::path::Path::new("./stark-frontend/dist/index.html").exists() {
        Ok(NamedFile::open("./stark-frontend/dist/index.html")?)
    } else {
        Ok(NamedFile::open("../stark-frontend/dist/index.html")?)
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    // Load presets and tokens from config directory
    // Check ./config first, then ../config (for running from subdirectory)
    let config_dir = if std::path::Path::new("./config").exists() {
        std::path::Path::new("./config")
    } else if std::path::Path::new("../config").exists() {
        std::path::Path::new("../config")
    } else {
        panic!("Config directory not found in ./config or ../config");
    };
    log::info!("Using config directory: {:?}", config_dir);
    log::info!("Loading presets from config directory");
    tools::presets::load_presets(config_dir);
    log::info!("Loading token configs from config directory");
    tools::builtin::token_lookup::load_tokens(config_dir);
    log::info!("Loading RPC provider configs from config directory");
    tools::rpc_config::load_rpc_providers(config_dir);

    let config = Config::from_env();
    let port = config.port;
    let gateway_port = config.gateway_port;

    log::info!("Initializing database at {}", config.database_url);
    let db = Database::new(&config.database_url).expect("Failed to initialize database");
    let db = Arc::new(db);

    // Initialize Tool Registry with built-in tools
    log::info!("Initializing tool registry");
    let tool_registry = Arc::new(tools::create_default_registry());
    log::info!("Registered {} tools", tool_registry.len());

    // Initialize Skill Registry (database-backed)
    log::info!("Initializing skill registry");
    let skill_registry = Arc::new(skills::create_default_registry(db.clone()));

    // Load file-based skills into database (for backward compatibility)
    let skill_count = skill_registry.load_all().await.unwrap_or_else(|e| {
        log::warn!("Failed to load skills from disk: {}", e);
        0
    });
    log::info!("Loaded {} skills from disk, {} total in database", skill_count, skill_registry.len());

    // Initialize Gateway with tool registry and wallet for x402 payment support
    log::info!("Initializing Gateway");
    let gateway = Arc::new(Gateway::new_with_tools_and_wallet(
        db.clone(),
        tool_registry.clone(),
        config.burner_wallet_private_key.clone(),
    ));

    // Initialize Execution Tracker for progress display
    log::info!("Initializing execution tracker");
    let execution_tracker = Arc::new(ExecutionTracker::new(gateway.broadcaster().clone()));

    // Create the shared MessageDispatcher for all message processing
    log::info!("Initializing message dispatcher");
    let dispatcher = Arc::new(MessageDispatcher::new_with_wallet(
        db.clone(),
        gateway.broadcaster().clone(),
        tool_registry.clone(),
        execution_tracker.clone(),
        config.burner_wallet_private_key.clone(),
    ));

    // Start Gateway WebSocket server
    let gw = gateway.clone();
    tokio::spawn(async move {
        gw.start(gateway_port).await;
    });

    // Start enabled channels
    log::info!("Starting enabled channels");
    gateway.start_enabled_channels().await;

    // Initialize and start the scheduler
    log::info!("Initializing scheduler");
    let scheduler_config = SchedulerConfig::default();
    let scheduler = Arc::new(Scheduler::new(
        db.clone(),
        dispatcher.clone(),
        gateway.broadcaster().clone(),
        scheduler_config,
    ));

    // Start scheduler background task
    let scheduler_handle = Arc::clone(&scheduler);
    let (scheduler_shutdown_tx, scheduler_shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        scheduler_handle.start(scheduler_shutdown_rx).await;
    });

    // Determine frontend dist path (check both locations)
    // Set DISABLE_FRONTEND=1 to disable static file serving (for separate dev server)
    let frontend_dist = if std::env::var("DISABLE_FRONTEND").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false) {
        log::info!("Frontend serving disabled via DISABLE_FRONTEND env var");
        ""
    } else if std::path::Path::new("./stark-frontend/dist").exists() {
        "./stark-frontend/dist"
    } else if std::path::Path::new("../stark-frontend/dist").exists() {
        "../stark-frontend/dist"
    } else {
        log::warn!("Frontend dist not found in ./stark-frontend/dist or ../stark-frontend/dist - static file serving disabled");
        ""
    };

    log::info!("Starting StarkBot server on port {}", port);
    log::info!("Gateway WebSocket server on port {}", gateway_port);
    log::info!("Scheduler started with cron and heartbeat support");
    if !frontend_dist.is_empty() {
        log::info!("Serving frontend from: {}", frontend_dist);
    }

    let tool_reg = tool_registry.clone();
    let skill_reg = skill_registry.clone();
    let disp = dispatcher.clone();
    let exec_tracker = execution_tracker.clone();
    let sched = scheduler.clone();
    let frontend_dist = frontend_dist.to_string();

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        let mut app = App::new()
            .app_data(web::Data::new(AppState {
                db: Arc::clone(&db),
                config: config.clone(),
                gateway: Arc::clone(&gateway),
                tool_registry: Arc::clone(&tool_reg),
                skill_registry: Arc::clone(&skill_reg),
                dispatcher: Arc::clone(&disp),
                execution_tracker: Arc::clone(&exec_tracker),
                scheduler: Arc::clone(&sched),
            }))
            .app_data(web::Data::new(Arc::clone(&sched)))
            .wrap(Logger::default())
            .wrap(cors)
            .configure(controllers::health::config)
            .configure(controllers::auth::config)
            .configure(controllers::dashboard::config)
            .configure(controllers::chat::config)
            .configure(controllers::api_keys::config)
            .configure(controllers::channels::config)
            .configure(controllers::agent_settings::configure)
            .configure(controllers::sessions::config)
            .configure(controllers::memories::config)
            .configure(controllers::identity::config)
            .configure(controllers::tools::config)
            .configure(controllers::skills::config)
            .configure(controllers::cron::config)
            .configure(controllers::gmail::config)
            .configure(controllers::payments::config)
            .configure(controllers::eip8004::config)
            .configure(controllers::files::config)
            .configure(controllers::intrinsic::config);

        // Serve static files only if frontend dist exists
        if !frontend_dist.is_empty() {
            app = app.service(
                Files::new("/", frontend_dist.clone())
                    .index_file("index.html")
                    .default_handler(actix_web::web::to(spa_fallback))
            );
        }

        app
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
