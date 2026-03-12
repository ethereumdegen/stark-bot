use actix_web::{web, HttpResponse, Responder};

use crate::AppState;

/// Serve the agent registration file at /.well-known/agent-registration.json
/// This is a PUBLIC endpoint (no auth) per EIP-8004 for domain verification.
async fn agent_registration(state: web::Data<AppState>) -> impl Responder {
    // Try to read agent identity from the database
    let conn = state.db.conn();
    let result = conn.query_row(
        "SELECT agent_id, agent_registry FROM agent_identity ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            let agent_id: Option<i64> = row.get(0)?;
            let agent_registry: Option<String> = row.get(1)?;
            Ok((agent_id, agent_registry))
        },
    );

    match result {
        Ok((agent_id, agent_registry)) => {
            HttpResponse::Ok()
                .content_type("application/json")
                .json(serde_json::json!({
                    "agent_id": agent_id,
                    "agent_registry": agent_registry,
                }))
        }
        Err(_) => {
            HttpResponse::NotFound().json(serde_json::json!({
                "error": "Agent registration not configured"
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/.well-known")
            .route("/agent-registration.json", web::get().to(agent_registration)),
    );
}
