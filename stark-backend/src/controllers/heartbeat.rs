use actix_web::{web, HttpRequest, HttpResponse};
use std::sync::Arc;

use crate::models::{HeartbeatConfigResponse, UpdateHeartbeatConfigRequest};
use crate::scheduler::Scheduler;
use crate::AppState;

fn validate_session_for_heartbeat(
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
            return Err(HttpResponse::Unauthorized().json(HeartbeatConfigResponse {
                success: false,
                config: None,
                error: Some("No authorization token provided".to_string()),
            }));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(HeartbeatConfigResponse {
            success: false,
            config: None,
            error: Some("Invalid or expired session".to_string()),
        })),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
                success: false,
                config: None,
                error: Some("Internal server error".to_string()),
            }))
        }
    }
}

/// Configure heartbeat routes
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/heartbeat")
            .route("/config", web::get().to(get_heartbeat_config))
            .route("/config", web::put().to(update_heartbeat_config))
            .route("/config/{channel_id}", web::get().to(get_channel_heartbeat_config))
            .route("/config/{channel_id}", web::put().to(update_channel_heartbeat_config))
            .route("/pulse_once", web::post().to(pulse_heartbeat)),
    );
}

/// Get global heartbeat config
async fn get_heartbeat_config(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session_for_heartbeat(&state, &req) {
        return resp;
    }

    match state.db.get_or_create_heartbeat_config(None) {
        Ok(config) => HttpResponse::Ok().json(HeartbeatConfigResponse {
            success: true,
            config: Some(config),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Database error: {}", e)),
        }),
    }
}

/// Update global heartbeat config
async fn update_heartbeat_config(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<UpdateHeartbeatConfigRequest>,
) -> HttpResponse {
    log::info!("[HEARTBEAT] Update requested: enabled={:?}", body.enabled);

    if let Err(resp) = validate_session_for_heartbeat(&state, &req) {
        log::warn!("[HEARTBEAT] Session validation failed for update");
        return resp;
    }

    // Get or create first
    let config = match state.db.get_or_create_heartbeat_config(None) {
        Ok(c) => {
            log::info!("[HEARTBEAT] Got existing config id={}", c.id);
            c
        }
        Err(e) => {
            log::error!("[HEARTBEAT] Failed to get/create config: {}", e);
            return HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Database error: {}", e)),
            });
        }
    };

    match state.db.update_heartbeat_config(
        config.id,
        body.interval_minutes,
        body.target.as_deref(),
        body.active_hours_start.as_deref(),
        body.active_hours_end.as_deref(),
        body.active_days.as_deref(),
        body.enabled,
        body.impulse_evolver,
    ) {
        Ok(updated) => {
            log::info!("[HEARTBEAT] Config updated successfully, enabled={}", updated.enabled);
            HttpResponse::Ok().json(HeartbeatConfigResponse {
                success: true,
                config: Some(updated),
                error: None,
            })
        }
        Err(e) => {
            log::error!("[HEARTBEAT] Failed to update config: {}", e);
            HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Failed to update config: {}", e)),
            })
        }
    }
}

/// Get heartbeat config for a specific channel
async fn get_channel_heartbeat_config(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> HttpResponse {
    if let Err(resp) = validate_session_for_heartbeat(&state, &req) {
        return resp;
    }

    let channel_id = path.into_inner();

    match state.db.get_or_create_heartbeat_config(Some(channel_id)) {
        Ok(config) => HttpResponse::Ok().json(HeartbeatConfigResponse {
            success: true,
            config: Some(config),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Database error: {}", e)),
        }),
    }
}

/// Update heartbeat config for a specific channel
async fn update_channel_heartbeat_config(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
    body: web::Json<UpdateHeartbeatConfigRequest>,
) -> HttpResponse {
    if let Err(resp) = validate_session_for_heartbeat(&state, &req) {
        return resp;
    }

    let channel_id = path.into_inner();

    // Get or create first
    let config = match state.db.get_or_create_heartbeat_config(Some(channel_id)) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Database error: {}", e)),
            });
        }
    };

    match state.db.update_heartbeat_config(
        config.id,
        body.interval_minutes,
        body.target.as_deref(),
        body.active_hours_start.as_deref(),
        body.active_hours_end.as_deref(),
        body.active_days.as_deref(),
        body.enabled,
        body.impulse_evolver,
    ) {
        Ok(updated) => HttpResponse::Ok().json(HeartbeatConfigResponse {
            success: true,
            config: Some(updated),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Failed to update config: {}", e)),
        }),
    }
}

/// Manually trigger a heartbeat pulse
async fn pulse_heartbeat(
    state: web::Data<AppState>,
    req: HttpRequest,
    scheduler: web::Data<Arc<Scheduler>>,
) -> HttpResponse {
    log::info!("[HEARTBEAT] Pulse requested");

    if let Err(resp) = validate_session_for_heartbeat(&state, &req) {
        log::warn!("[HEARTBEAT] Session validation failed for pulse");
        return resp;
    }

    // Get or create global heartbeat config
    let config = match state.db.get_or_create_heartbeat_config(None) {
        Ok(c) => {
            log::info!("[HEARTBEAT] Got config id={}, enabled={}", c.id, c.enabled);
            c
        }
        Err(e) => {
            log::error!("[HEARTBEAT] Failed to get config: {}", e);
            return HttpResponse::InternalServerError().json(HeartbeatConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Database error: {}", e)),
            });
        }
    };

    log::info!("[HEARTBEAT] Running heartbeat now for config {}", config.id);

    // Fire and forget - returns immediately, heartbeat runs in background
    match scheduler.get_ref().run_heartbeat_now(config.id) {
        Ok(msg) => {
            log::info!("[HEARTBEAT] {}", msg);
            HttpResponse::Ok().json(HeartbeatConfigResponse {
                success: true,
                config: Some(config),
                error: None,
            })
        }
        Err(e) => {
            log::error!("[HEARTBEAT] Pulse failed to start: {}", e);
            HttpResponse::BadRequest().json(HeartbeatConfigResponse {
                success: false,
                config: Some(config),
                error: Some(e),
            })
        }
    }
}
