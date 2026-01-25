use actix_cors::Cors;
use actix_files::Files;
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use std::sync::Arc;

mod ai;
mod channels;
mod config;
mod controllers;
mod db;
mod gateway;
mod middleware;
mod models;

use config::Config;
use db::Database;
use gateway::Gateway;

pub struct AppState {
    pub db: Arc<Database>,
    pub config: Config,
    pub gateway: Arc<Gateway>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let config = Config::from_env();
    let port = config.port;
    let gateway_port = config.gateway_port;

    log::info!("Initializing database at {}", config.database_url);
    let db = Database::new(&config.database_url).expect("Failed to initialize database");
    let db = Arc::new(db);

    // Initialize Gateway
    log::info!("Initializing Gateway");
    let gateway = Arc::new(Gateway::new(db.clone()));

    // Start Gateway WebSocket server
    let gw = gateway.clone();
    tokio::spawn(async move {
        gw.start(gateway_port).await;
    });

    // Start enabled channels
    log::info!("Starting enabled channels");
    gateway.start_enabled_channels().await;

    log::info!("Starting StarkBot server on port {}", port);
    log::info!("Gateway WebSocket server on port {}", gateway_port);

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .app_data(web::Data::new(AppState {
                db: Arc::clone(&db),
                config: config.clone(),
                gateway: Arc::clone(&gateway),
            }))
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
            .service(Files::new("/", "./stark-frontend").index_file("index.html"))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
