use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use std::sync::Arc;

mod backup;
mod config;
mod controllers;
mod crypto;
mod crypto_executor;
mod db;
mod gateway;
mod middleware;
mod models;
mod siwa;
mod wallet;
mod x402;
mod credits_session;
mod erc8128;
pub mod http;
mod tx_queue;
mod web3;
mod keystore_client;
mod identity_client;
mod rpc_config;
mod starflask_bridge;

use tx_queue::TxQueueManager;
use config::Config;
use db::Database;
use gateway::events::EventBroadcaster;
use wallet::WalletProvider;

pub struct AppState {
    pub db: Arc<Database>,
    pub config: Config,
    pub wallet_provider: Option<Arc<dyn WalletProvider>>,
    pub tx_queue: Arc<TxQueueManager>,
    pub broadcaster: Arc<EventBroadcaster>,
    pub crypto_executor: Option<Arc<crypto_executor::CryptoExecutor>>,
    pub starflask: Option<Arc<starflask::Starflask>>,
    pub credits_session: Option<Arc<credits_session::CreditsSessionClient>>,
    pub internal_token: String,
    pub started_at: std::time::Instant,
}

/// Auto-retrieve backup from keystore on fresh instance
async fn load_keystore_state_from_cloud(
    db: &Arc<Database>,
    wallet_provider: &Arc<dyn WalletProvider>,
    broadcaster: &Arc<EventBroadcaster>,
) {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_BACKOFF_SECS: u64 = 2;

    let wallet_address = wallet_provider.get_address().to_lowercase();

    let emit = |status: &str, message: &str| {
        broadcaster.broadcast(gateway::protocol::GatewayEvent::new(
            format!("system.keystore_{}", status),
            serde_json::json!({
                "status": status,
                "message": message,
                "wallet": &wallet_address,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        ));
    };

    match db.has_keystore_auto_retrieved(&wallet_address) {
        Ok(true) => {
            log::debug!("[Keystore] Already auto-retrieved for wallet {}", wallet_address);
            return;
        }
        Ok(false) => {}
        Err(e) => {
            log::warn!("[Keystore] Failed to check auto-retrieval status: {}", e);
            return;
        }
    }

    let has_api_keys = db.list_api_keys().map(|k| !k.is_empty()).unwrap_or(false);
    if has_api_keys {
        log::info!("[Keystore] Local state exists, skipping auto-retrieval");
        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
        let _ = db.record_auto_sync_result(&wallet_address, "skipped", "Local state already exists", None, None);
        emit("skipped", "Local state already exists, skipping cloud restore");
        return;
    }

    log::info!("[Keystore] Fresh instance detected, attempting auto-retrieval for {}", wallet_address);
    emit("started", "Fresh instance detected, attempting cloud backup restore...");

    let mut last_error = String::new();
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let backoff = INITIAL_BACKOFF_SECS * (1 << (attempt - 1));
            log::info!("[Keystore] Retry {} of {}, waiting {}s...", attempt + 1, MAX_RETRIES, backoff);
            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        }

        let get_result = keystore_client::KEYSTORE_CLIENT
            .get_keys_with_provider(wallet_provider)
            .await;
        match get_result {
            Ok(resp) => {
                if resp.success {
                    if let Some(encrypted_data) = resp.encrypted_data {
                        let encryption_key = match wallet_provider.get_encryption_key().await {
                            Ok(k) => k,
                            Err(e) => {
                                log::error!("[Keystore] Failed to get encryption key: {}", e);
                                let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                                return;
                            }
                        };
                        let mut backup_data = match keystore_client::decrypt_backup_data(&encryption_key, &encrypted_data) {
                            Ok(b) => b,
                            Err(e) => {
                                log::error!("[Keystore] Failed to decrypt backup: {}", e);
                                let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                                return;
                            }
                        };
                        match backup::restore::restore_all(db, &mut backup_data).await {
                            Ok(restore_result) => {
                                let summary = restore_result.summary();
                                log::info!("[Keystore] Auto-sync: {}", summary);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address, "success", &summary,
                                    Some(restore_result.api_keys as i32), None,
                                );
                                emit("success", &format!("Cloud backup restored: {}", summary));
                            }
                            Err(e) => {
                                log::error!("[Keystore] Failed to restore backup: {}", e);
                                let msg = format!("Restore failed: {}", e);
                                let _ = db.record_auto_sync_result(&wallet_address, "error", &msg, None, None);
                                emit("error", &msg);
                            }
                        }
                        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                        return;
                    } else {
                        log::info!("[Keystore] Server returned success but no backup data");
                        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                        let _ = db.record_auto_sync_result(&wallet_address, "no_backup", "No backup data found", None, None);
                        emit("no_backup", "No cloud backup data found");
                        return;
                    }
                } else if let Some(error) = &resp.error {
                    if error.contains("No backup found") {
                        log::info!("[Keystore] No cloud backup found - starting fresh");
                        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                        let _ = db.record_auto_sync_result(&wallet_address, "no_backup", "No cloud backup found", None, None);
                        emit("no_backup", "No cloud backup found");
                        return;
                    }
                    last_error = error.clone();
                }
            }
            Err(e) => {
                last_error = e;
                log::warn!("[Keystore] Attempt {} failed: {}", attempt + 1, last_error);
            }
        }
    }

    log::error!("[Keystore] Auto-retrieval failed after {} attempts: {}", MAX_RETRIES, last_error);
    let _ = db.mark_keystore_auto_retrieved(&wallet_address);
    let _ = db.record_auto_sync_result(&wallet_address, "error", &format!("Auto-sync failed: {}", last_error), None, None);
    emit("error", &format!("Auto-sync failed after {} attempts", MAX_RETRIES));
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    // Load config from config directory
    let config_dir = if std::path::Path::new("./config").exists() {
        std::path::Path::new("./config")
    } else if std::path::Path::new("../config").exists() {
        std::path::Path::new("../config")
    } else {
        panic!("Config directory not found in ./config or ../config");
    };
    log::info!("Starkbot v{}", env!("CARGO_PKG_VERSION"));
    log::info!("Using config directory: {:?}", config_dir);

    // Load token configs and x402 payment limits
    crypto::token_utils::load_tokens(config_dir);
    x402::payment_limits::load_defaults(config_dir);

    let config = Config::from_env();
    let port = config.port;

    log::info!("Initializing database at {}", config.database_url);
    let db = Database::new(&config.database_url).expect("Failed to initialize database");
    let db = Arc::new(db);

    // Override x402 payment limits with user-configured values from DB
    match db.get_all_x402_payment_limits() {
        Ok(limits) => {
            for l in &limits {
                x402::payment_limits::set_limit(&l.asset, &l.max_amount, l.decimals, &l.display_name, l.address.as_deref());
            }
            if !limits.is_empty() {
                log::info!("Loaded {} x402 payment limits from database", limits.len());
            }
        }
        Err(e) => log::warn!("Failed to load x402 payment limits from DB: {}", e),
    }

    // Initialize Transaction Queue Manager
    log::info!("Initializing transaction queue manager");
    let tx_queue = Arc::new(TxQueueManager::with_db(db.clone()));

    // Initialize Wallet Provider
    let is_flash_mode = std::env::var("FLASH_KEYSTORE_URL").is_ok();
    log::info!("Initializing wallet provider");
    let wallet_provider: Option<Arc<dyn WalletProvider>> = if is_flash_mode {
        log::info!("Flash mode: initializing FlashWalletProvider...");
        match wallet::FlashWalletProvider::new().await {
            Ok(provider) => {
                log::info!("Flash wallet provider initialized: {} (mode: {})",
                    provider.get_address(), provider.mode_name());
                Some(Arc::new(provider) as Arc<dyn WalletProvider>)
            }
            Err(e) => {
                log::error!("Failed to create Flash wallet provider: {}", e);
                None
            }
        }
    } else if let Some(ref pk) = config.burner_wallet_private_key {
        log::info!("Standard mode: initializing EnvWalletProvider...");
        match wallet::EnvWalletProvider::from_private_key(pk) {
            Ok(provider) => {
                log::info!("Wallet provider initialized: {} (mode: {})",
                    provider.get_address(), provider.mode_name());
                Some(Arc::new(provider) as Arc<dyn WalletProvider>)
            }
            Err(e) => {
                log::warn!("Failed to create wallet provider: {}. Wallet features disabled.", e);
                None
            }
        }
    } else {
        log::warn!("No wallet configured - set FLASH_KEYSTORE_URL or BURNER_WALLET_BOT_PRIVATE_KEY");
        None
    };

    // Create credits session client
    let credits_session: Option<Arc<credits_session::CreditsSessionClient>> = if let Some(ref wp) = wallet_provider {
        let base_url = std::env::var("CREDITS_BASE_URL")
            .unwrap_or_else(|_| "https://inference.defirelay.com".to_string());
        log::info!("Credits session client initialized (base_url: {}, wallet: {})", base_url, wp.get_address());
        Some(Arc::new(credits_session::CreditsSessionClient::new(wp.clone(), &base_url)))
    } else {
        None
    };

    // Create EventBroadcaster
    let broadcaster = Arc::new(EventBroadcaster::new());

    // Create CryptoExecutor
    let crypto_executor = wallet_provider.as_ref().map(|wp| {
        Arc::new(crypto_executor::CryptoExecutor {
            wallet_provider: wp.clone(),
            tx_queue: tx_queue.clone(),
            broadcaster: broadcaster.clone(),
            credits_session: credits_session.clone(),
            db: Some(db.clone()),
        })
    });

    // Create Starflask client
    let starflask = starflask_bridge::create_starflask_client()
        .map(Arc::new);
    if starflask.is_some() {
        log::info!("Starflask client initialized");
    }

    // Generate internal token
    let internal_token = std::env::var("STARKBOT_INTERNAL_TOKEN").unwrap_or_else(|_| {
        let mut buf = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut buf);
        let token = hex::encode(buf);
        log::info!("Generated STARKBOT_INTERNAL_TOKEN");
        token
    });

    // Initialize keystore URL
    let env_keystore_url = std::env::var("KEYSTORE_URL").ok().filter(|s| !s.is_empty());
    let cfg_keystore_url = models::BotConfig::load().services.keystore_server_url.filter(|s| !s.is_empty());
    if let Some(url) = cfg_keystore_url.or(env_keystore_url) {
        log::info!("Using custom keystore URL: {}", url);
        keystore_client::KEYSTORE_CLIENT.set_base_url(&url).await;
    }

    // Background init: keystore auto-retrieve
    {
        let db_bg = db.clone();
        let wallet_provider_bg = wallet_provider.clone();
        let broadcaster_bg = broadcaster.clone();
        tokio::spawn(async move {
            let auto_sync = std::env::var(config::env_vars::AUTO_SYNC_FROM_KEYSTORE)
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true);
            if auto_sync {
                if let Some(ref wp) = wallet_provider_bg {
                    load_keystore_state_from_cloud(&db_bg, wp, &broadcaster_bg).await;
                }
            }
        });
    }

    log::info!("Starting StarkBot server on port {}", port);
    log::info!("WebSocket available at /ws");

    let db_clone = db.clone();
    let bcast = broadcaster.clone();
    let tx_q = tx_queue.clone();
    let wallet_prov = wallet_provider.clone();
    let credits_sess = credits_session.clone();
    let crypto_exec = crypto_executor.clone();
    let starflask_clone = starflask.clone();
    let internal_token_clone = internal_token.clone();

    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .app_data(web::Data::new(AppState {
                db: Arc::clone(&db_clone),
                config: config.clone(),
                wallet_provider: wallet_prov.clone(),
                tx_queue: Arc::clone(&tx_q),
                broadcaster: Arc::clone(&bcast),
                crypto_executor: crypto_exec.clone(),
                starflask: starflask_clone.clone(),
                credits_session: credits_sess.clone(),
                internal_token: internal_token_clone.clone(),
                started_at: std::time::Instant::now(),
            }))
            // Extra app_data for /ws handler
            .app_data(web::Data::new(Arc::clone(&db_clone)))
            .app_data(web::Data::new(Arc::clone(&bcast)))
            .app_data(web::Data::new(Arc::clone(&tx_q)))
            .app_data(web::Data::new(wallet_prov.clone()))
            .wrap(Logger::default())
            .wrap(cors)
            // Routes
            .configure(controllers::health::config_routes)
            .configure(controllers::auth::config)
            .configure(controllers::api_keys::config)
            .configure(controllers::tx_queue::config)
            .configure(controllers::broadcasted_transactions::config)
            .configure(controllers::internal_wallet::config)
            .configure(controllers::x402::config)
            .configure(controllers::x402_limits::config)
            .configure(controllers::well_known::config)
            .configure(controllers::payments::config)
            // Crypto execution endpoints
            .route("/api/execute", web::post().to(controllers::execute::execute_instruction))
            .route("/webhook/starflask", web::post().to(controllers::execute::starflask_webhook))
            // WebSocket
            .route("/ws", web::get().to(gateway::actix_ws::ws_handler))
    })
    .bind(("0.0.0.0", port))?
    .run();

    let server_handle = server.handle();

    // Ctrl+C handler
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        log::info!("Received Ctrl+C, shutting down...");
        let server_stop = server_handle.stop(true);
        if tokio::time::timeout(std::time::Duration::from_secs(5), server_stop).await.is_err() {
            log::warn!("Timeout waiting for HTTP server to stop");
        }
        log::info!("Shutdown complete");
    });

    server.await
}
