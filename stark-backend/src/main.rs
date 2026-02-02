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
mod tool_validators;
mod tx_queue;

use channels::{ChannelManager, MessageDispatcher};
use tx_queue::TxQueueManager;
use config::Config;
use db::Database;
use execution::ExecutionTracker;
use gateway::{events::EventBroadcaster, Gateway};
use hooks::{HookManager, builtin::AutoMemoryHook};
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
    pub channel_manager: Arc<ChannelManager>,
    pub broadcaster: Arc<EventBroadcaster>,
    pub hook_manager: Arc<HookManager>,
    pub tx_queue: Arc<TxQueueManager>,
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

    // Initialize workspace directory and copy SOUL.md
    log::info!("Initializing workspace");
    if let Err(e) = config::initialize_workspace() {
        log::error!("Failed to initialize workspace: {}", e);
    }

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

    // Initialize Hook Manager with auto-memory hook
    log::info!("Initializing hook manager");
    let hook_manager = Arc::new(HookManager::new());
    hook_manager.register(Arc::new(AutoMemoryHook::new(db.clone())));
    log::info!("Registered {} hooks", hook_manager.hook_count());

    // Initialize Tool Validator Registry
    log::info!("Initializing tool validator registry");
    let validator_registry = Arc::new(tool_validators::create_default_registry());
    log::info!("Registered {} tool validators", validator_registry.len());

    // Initialize Transaction Queue Manager
    log::info!("Initializing transaction queue manager");
    let tx_queue = Arc::new(TxQueueManager::new());

    // Create the shared MessageDispatcher for all message processing
    log::info!("Initializing message dispatcher");
    let dispatcher = Arc::new(
        MessageDispatcher::new_with_wallet_and_skills(
            db.clone(),
            gateway.broadcaster().clone(),
            tool_registry.clone(),
            execution_tracker.clone(),
            config.burner_wallet_private_key.clone(),
            Some(skill_registry.clone()),
        ).with_hook_manager(hook_manager.clone())
         .with_validator_registry(validator_registry.clone())
         .with_tx_queue(tx_queue.clone())
    );

    // Get broadcaster and channel_manager for the /ws route
    let broadcaster = gateway.broadcaster();
    let channel_manager = gateway.channel_manager();

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
    log::info!("WebSocket Gateway available at /ws");
    log::info!("Scheduler started with cron and heartbeat support");
    if !frontend_dist.is_empty() {
        log::info!("Serving frontend from: {}", frontend_dist);
    }

    let tool_reg = tool_registry.clone();
    let skill_reg = skill_registry.clone();
    let disp = dispatcher.clone();
    let exec_tracker = execution_tracker.clone();
    let sched = scheduler.clone();
    let bcast = broadcaster.clone();
    let chan_mgr = channel_manager.clone();
    let hook_mgr = hook_manager.clone();
    let tx_q = tx_queue.clone();
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
                channel_manager: Arc::clone(&chan_mgr),
                broadcaster: Arc::clone(&bcast),
                hook_manager: Arc::clone(&hook_mgr),
                tx_queue: Arc::clone(&tx_q),
            }))
            .app_data(web::Data::new(Arc::clone(&sched)))
            // WebSocket data for /ws route
            .app_data(web::Data::new(Arc::clone(&db)))
            .app_data(web::Data::new(Arc::clone(&chan_mgr)))
            .app_data(web::Data::new(Arc::clone(&bcast)))
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
            .configure(controllers::intrinsic::config)
            .configure(controllers::journal::config)
            .configure(controllers::tx_queue::config)
            // WebSocket Gateway route (same port as HTTP, required for single-port platforms)
            .route("/ws", web::get().to(gateway::actix_ws::ws_handler));

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
