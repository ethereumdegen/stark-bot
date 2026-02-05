use actix_web::{web, HttpResponse, Responder};

use crate::{config, AppState};

/// Version from Cargo.toml, available at compile time
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn config_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/api/health").route(web::get().to(health_check)));
    cfg.service(web::resource("/api/version").route(web::get().to(get_version)));
    cfg.service(web::resource("/api/health/config").route(web::get().to(get_config_status)));
}

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "version": VERSION
    }))
}

async fn get_version() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "version": VERSION
    }))
}

async fn get_config_status(state: web::Data<AppState>) -> impl Responder {
    // Get the bot's wallet address and mode from the wallet provider (if configured)
    let (wallet_address, wallet_mode) = match &state.wallet_provider {
        Some(provider) => (Some(provider.get_address()), Some(provider.mode_name())),
        None => (None, None),
    };

    HttpResponse::Ok().json(serde_json::json!({
        "login_configured": state.config.login_admin_public_address.is_some(),
        "burner_wallet_configured": config::burner_wallet_private_key().is_some(),
        "wallet_configured": state.wallet_provider.is_some(),
        "guest_dashboard_enabled": config::guest_dashboard_enabled(),
        "wallet_address": wallet_address,
        "wallet_mode": wallet_mode
    }))
}
