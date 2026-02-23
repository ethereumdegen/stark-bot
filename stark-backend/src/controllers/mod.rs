pub mod agent_settings;
pub mod agent_subtypes;
pub mod api_keys;
pub mod auth;
pub mod broadcasted_transactions;
pub mod channels;
pub mod chat;
pub mod cron;
pub mod dashboard;
pub mod heartbeat;
pub mod eip8004;
pub mod ext;
pub mod external_channel;
pub mod files;
pub mod gmail;
pub mod health;
pub mod identity;
pub mod internal_wallet;
pub mod intrinsic;
pub mod kanban;
pub mod notes;
pub mod memory;
pub mod impulse_map;
pub mod modules;
pub mod payments;
pub mod public_files;
pub mod sessions;
pub mod skills;
pub mod tools;
pub mod tx_queue;
pub mod well_known;
pub mod system;
pub mod special_roles;
pub mod telemetry;
pub mod transcribe;
pub mod x402_limits;

use actix_web::{web, HttpRequest, HttpResponse};
use crate::AppState;

/// Shared session validation for controller handlers.
pub fn validate_session(
    state: &web::Data<AppState>,
    req: &HttpRequest,
) -> Result<(), HttpResponse> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "No authorization token provided"
            })));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid or expired session"
        }))),
        Err(e) => {
            log::error!("Session validation error: {}", e);
            Err(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            })))
        }
    }
}
