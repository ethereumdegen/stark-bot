//! Starflask command & control REST endpoints.
//!
//! Provides both the Starkbot-level agent registry/command routing AND
//! full passthrough to the Starflask API for managing agents, sessions,
//! hooks, integrations, tasks, and memories.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::command_router::Command;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/starflask")
            // ── Starkbot registry & command routing ──────────
            .route("/agents", web::get().to(list_agents))
            .route("/agents/{capability}", web::get().to(get_agent))
            .route("/agents/{capability}", web::delete().to(delete_agent_by_capability))
            .route("/provision", web::post().to(provision))
            .route("/reprovision/{capability}", web::post().to(reprovision))
            .route("/command", web::post().to(send_command))
            .route("/commands", web::get().to(list_commands))

            // ── Starflask passthrough: agents ────────────────
            .route("/remote/agents", web::get().to(remote_list_agents))
            .route("/remote/agents", web::post().to(remote_create_agent))
            .route("/remote/agents/{agent_id}", web::get().to(remote_get_agent))
            .route("/remote/agents/{agent_id}", web::put().to(remote_update_agent))
            .route("/remote/agents/{agent_id}", web::delete().to(remote_delete_agent))
            .route("/remote/agents/{agent_id}/active", web::put().to(remote_set_agent_active))

            // ── Starflask passthrough: sessions ──────────────
            .route("/remote/agents/{agent_id}/sessions", web::get().to(remote_list_sessions))
            .route("/remote/agents/{agent_id}/sessions/{session_id}", web::get().to(remote_get_session))
            .route("/remote/agents/{agent_id}/query", web::post().to(remote_query_agent))

            // ── Starflask passthrough: hooks ─────────────────
            .route("/remote/agents/{agent_id}/hooks", web::get().to(remote_get_hooks))
            .route("/remote/agents/{agent_id}/fire_hook", web::post().to(remote_fire_hook))

            // ── Starflask passthrough: packs ─────────────────
            .route("/remote/agents/{agent_id}/agent-pack", web::put().to(remote_install_pack))

            // ── Starflask passthrough: integrations ──────────
            .route("/remote/agents/{agent_id}/integrations", web::get().to(remote_list_integrations))
            .route("/remote/agents/{agent_id}/integrations", web::post().to(remote_create_integration))
            .route("/remote/agents/{agent_id}/integrations/{integration_id}", web::delete().to(remote_delete_integration))

            // ── Starflask passthrough: tasks ─────────────────
            .route("/remote/agents/{agent_id}/tasks", web::get().to(remote_list_tasks))
            .route("/remote/agents/{agent_id}/tasks", web::post().to(remote_create_task))

            // ── Starflask passthrough: memories ──────────────
            .route("/remote/agents/{agent_id}/memories", web::get().to(remote_list_memories))

            // ── Starflask passthrough: subscription ──────────
            .route("/remote/subscription", web::get().to(remote_subscription_status))

            // ── Re-initialize Starflask after adding API key ──
            .route("/init", web::post().to(init_starflask))

            // ── Convenience: capability-based session/hook access ─
            .route("/agents/{capability}/sessions", web::get().to(capability_list_sessions))
            .route("/agents/{capability}/sessions/{session_id}", web::get().to(capability_get_session))
            .route("/agents/{capability}/hooks", web::get().to(capability_get_hooks))
            .route("/agents/{capability}/query", web::post().to(capability_query))
            .route("/agents/{capability}/fire_hook", web::post().to(capability_fire_hook))
            .route("/agents/{capability}/memories", web::get().to(capability_list_memories))
            .route("/agents/{capability}/tasks", web::get().to(capability_list_tasks))
            .route("/agents/{capability}/integrations", web::get().to(capability_list_integrations))

            // ── Chat agents (agents with "chat" hook) ─────────
            .route("/chat_agents", web::get().to(list_chat_agents))
    );
}

// ─── Helpers ────────────────────────────────────────────────────────

async fn require_starflask(state: &web::Data<AppState>) -> Result<std::sync::Arc<starflask::Starflask>, HttpResponse> {
    state.starflask.read().await.clone().ok_or_else(|| {
        HttpResponse::ServiceUnavailable().json(json!({ "error": "Starflask not configured — add STARFLASK_API_KEY via API Keys page" }))
    })
}

fn parse_uuid(s: &str) -> Result<uuid::Uuid, HttpResponse> {
    uuid::Uuid::parse_str(s).map_err(|e| {
        HttpResponse::BadRequest().json(json!({ "error": format!("Invalid UUID: {}", e) }))
    })
}

/// Resolve a capability string to an agent_id UUID from the local DB.
fn resolve_capability(state: &web::Data<AppState>, capability: &str) -> Result<uuid::Uuid, HttpResponse> {
    let agent = state.db.get_starflask_agent(capability)
        .map_err(|e| HttpResponse::InternalServerError().json(json!({ "error": e })))?
        .ok_or_else(|| HttpResponse::NotFound().json(json!({
            "error": format!("No agent for capability '{}'", capability)
        })))?;
    parse_uuid(&agent.agent_id)
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct MemoriesQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

// ═══════════════════════════════════════════════════════════════════
// Starkbot registry & command routing endpoints
// ═══════════════════════════════════════════════════════════════════

/// POST /api/starflask/init — (re)initialize Starflask client from DB key
async fn init_starflask(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    state.init_starflask().await;
    let connected = state.starflask.read().await.is_some();
    if connected {
        HttpResponse::Ok().json(json!({ "status": "ok", "message": "Starflask initialized" }))
    } else {
        HttpResponse::BadRequest().json(json!({ "error": "No STARFLASK_API_KEY found in env or database" }))
    }
}

/// GET /api/starflask/agents — list locally provisioned agents
async fn list_agents(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let guard = state.agent_registry.read().await;
    let registry = match guard.as_ref() {
        Some(r) => r,
        None => return HttpResponse::ServiceUnavailable().json(json!({ "error": "Starflask not configured" })),
    };

    match registry.list_agents() {
        Ok(agents) => HttpResponse::Ok().json(agents),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e })),
    }
}

/// GET /api/starflask/chat_agents — list agents that have a "chat" hook
async fn list_chat_agents(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };

    let guard = state.agent_registry.read().await;
    let registry = match guard.as_ref() {
        Some(r) => r,
        None => return HttpResponse::ServiceUnavailable().json(json!({ "error": "Starflask not configured" })),
    };

    let agents = match registry.list_agents() {
        Ok(a) => a,
        Err(e) => return HttpResponse::InternalServerError().json(json!({ "error": e })),
    };

    let mut chat_agents = Vec::new();
    for agent in &agents {
        let Ok(agent_id) = uuid::Uuid::parse_str(&agent.agent_id) else { continue };
        if let Ok(hooks_resp) = sf.get_hooks(&agent_id).await {
            let has_chat = hooks_resp.hooks.iter().any(|h| {
                h.get("event").and_then(|v| v.as_str()) == Some("chat")
            });
            if has_chat {
                chat_agents.push(json!({
                    "capability": &agent.capability,
                    "name": &agent.name,
                    "description": &agent.description,
                    "agent_id": &agent.agent_id,
                }));
            }
        }
    }

    HttpResponse::Ok().json(chat_agents)
}

/// GET /api/starflask/agents/{capability}
async fn get_agent(state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let capability = path.into_inner();
    match state.db.get_starflask_agent(&capability) {
        Ok(Some(agent)) => HttpResponse::Ok().json(agent),
        Ok(None) => HttpResponse::NotFound().json(json!({ "error": format!("No agent for capability '{}'", capability) })),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e })),
    }
}

/// POST /api/starflask/provision
/// Syncs remote agents from the Starflask account, then provisions from seed config.
async fn provision(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let guard = state.agent_registry.read().await;
    let registry = match guard.as_ref() {
        Some(r) => r.clone(),
        None => return HttpResponse::ServiceUnavailable().json(json!({ "error": "Starflask not configured" })),
    };
    drop(guard);

    // First sync existing agents from the Starflask account
    let mut all_synced = Vec::new();
    match registry.sync_remote_agents().await {
        Ok(synced) => all_synced.extend(synced),
        Err(e) => log::warn!("[provision] sync_remote_agents failed: {}", e),
    }

    // Then provision from seed (only if real pack hashes exist)
    match registry.provision_from_seed().await {
        Ok(provisioned) => all_synced.extend(provisioned),
        Err(e) => log::warn!("[provision] provision_from_seed failed: {}", e),
    }

    // Return the full list of agents
    let agents = registry.list_agents().unwrap_or_default();
    HttpResponse::Ok().json(json!({
        "status": "ok",
        "provisioned": all_synced,
        "agents": agents,
    }))
}

/// DELETE /api/starflask/agents/{capability}
async fn delete_agent_by_capability(state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let guard = state.agent_registry.read().await;
    let registry = match guard.as_ref() {
        Some(r) => r.clone(),
        None => return HttpResponse::ServiceUnavailable().json(json!({ "error": "Starflask not configured" })),
    };
    drop(guard);

    let capability = path.into_inner();
    match registry.delete_agent(&capability).await {
        Ok(()) => HttpResponse::Ok().json(json!({ "status": "ok", "capability": capability })),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e })),
    }
}

/// POST /api/starflask/reprovision/{capability}
async fn reprovision(state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let guard = state.agent_registry.read().await;
    let registry = match guard.as_ref() {
        Some(r) => r.clone(),
        None => return HttpResponse::ServiceUnavailable().json(json!({ "error": "Starflask not configured" })),
    };
    drop(guard);

    let capability = path.into_inner();
    match registry.reprovision(&capability).await {
        Ok(agent_id) => HttpResponse::Ok().json(json!({ "status": "ok", "capability": capability, "agent_id": agent_id.to_string() })),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e })),
    }
}

/// POST /api/starflask/command
async fn send_command(state: web::Data<AppState>, req: HttpRequest, body: web::Json<Command>) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let guard = state.command_router.read().await;
    let router = match guard.as_ref() {
        Some(r) => r.clone(),
        None => return HttpResponse::ServiceUnavailable().json(json!({ "error": "Command router not configured — add STARFLASK_API_KEY via API Keys page" })),
    };
    drop(guard);

    match router.route(body.into_inner()).await {
        Ok(output) => HttpResponse::Ok().json(output),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e })),
    }
}

/// GET /api/starflask/commands
async fn list_commands(state: web::Data<AppState>, req: HttpRequest, query: web::Query<LimitQuery>) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let limit = query.limit.unwrap_or(50);
    match state.db.list_starflask_commands(limit) {
        Ok(commands) => HttpResponse::Ok().json(commands),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e })),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Capability-based convenience endpoints (resolve capability → agent_id)
// ═══════════════════════════════════════════════════════════════════

/// GET /api/starflask/agents/{capability}/sessions
async fn capability_list_sessions(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, query: web::Query<LimitQuery>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_sessions(&agent_id, query.limit.map(|l| l.min(100))).await {
        Ok(sessions) => HttpResponse::Ok().json(sessions),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/agents/{capability}/sessions/{session_id}
async fn capability_get_session(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<(String, String)>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let (capability, session_id_str) = path.into_inner();
    let agent_id = match resolve_capability(&state, &capability) { Ok(id) => id, Err(resp) => return resp };
    let session_id = match parse_uuid(&session_id_str) { Ok(id) => id, Err(resp) => return resp };

    match sf.get_session(&agent_id, &session_id).await {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/agents/{capability}/hooks
async fn capability_get_hooks(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.get_hooks(&agent_id).await {
        Ok(hooks) => HttpResponse::Ok().json(hooks),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/agents/{capability}/query
async fn capability_query(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    match sf.query(&agent_id, message).await {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/agents/{capability}/fire_hook
///
/// When `wait=true`, routes through `CommandRouter::fire_hook_with_delegation()` so that
/// hook-driven agents can delegate to specialists. Falls back to raw Starflask if no
/// command router is available.
async fn capability_fire_hook(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }

    let capability = path.into_inner();
    let event = body.get("event").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let payload = body.get("payload").cloned().unwrap_or(json!({}));
    let wait = body.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);

    // When wait=true, try routing through command router for delegation support
    if wait {
        let guard = state.command_router.read().await;
        if let Some(router) = guard.as_ref() {
            let router = router.clone();
            drop(guard);
            match router.fire_hook_with_delegation(&capability, &event, payload).await {
                Ok(output) => return HttpResponse::Ok().json(output),
                Err(e) => return HttpResponse::BadRequest().json(json!({ "error": e })),
            }
        }
        drop(guard);
        // Fall through to raw Starflask call if no command router
    }

    // Raw Starflask call (no delegation support)
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &capability) { Ok(id) => id, Err(resp) => return resp };

    let result = if wait {
        sf.fire_hook_and_wait(&agent_id, &event, payload).await
    } else {
        sf.fire_hook(&agent_id, &event, payload).await
    };

    match result {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/agents/{capability}/memories
async fn capability_list_memories(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, query: web::Query<MemoriesQuery>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_memories(&agent_id, query.limit, query.offset).await {
        Ok(memories) => HttpResponse::Ok().json(memories),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/agents/{capability}/tasks
async fn capability_list_tasks(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_tasks(&agent_id).await {
        Ok(tasks) => HttpResponse::Ok().json(tasks),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/agents/{capability}/integrations
async fn capability_list_integrations(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match resolve_capability(&state, &path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_integrations(&agent_id).await {
        Ok(integrations) => HttpResponse::Ok().json(integrations),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Starflask remote passthrough (direct agent_id access)
// ═══════════════════════════════════════════════════════════════════

/// GET /api/starflask/remote/agents
async fn remote_list_agents(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };

    match sf.list_agents().await {
        Ok(agents) => HttpResponse::Ok().json(agents),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/remote/agents
async fn remote_create_agent(
    state: web::Data<AppState>, req: HttpRequest, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };

    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("Untitled Agent");
    match sf.create_agent(name).await {
        Ok(agent) => HttpResponse::Created().json(agent),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}
async fn remote_get_agent(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.get_agent(&agent_id).await {
        Ok(agent) => HttpResponse::Ok().json(agent),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// PUT /api/starflask/remote/agents/{agent_id}
async fn remote_update_agent(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let name = body.get("name").and_then(|v| v.as_str());
    let description = body.get("description").and_then(|v| v.as_str());
    match sf.update_agent(&agent_id, name, description).await {
        Ok(agent) => HttpResponse::Ok().json(agent),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// DELETE /api/starflask/remote/agents/{agent_id}
async fn remote_delete_agent(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.delete_agent(&agent_id).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// PUT /api/starflask/remote/agents/{agent_id}/active
async fn remote_set_agent_active(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let active = body.get("active").and_then(|v| v.as_bool()).unwrap_or(true);
    match sf.set_agent_active(&agent_id, active).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}/sessions
async fn remote_list_sessions(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, query: web::Query<LimitQuery>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_sessions(&agent_id, query.limit.map(|l| l.min(100))).await {
        Ok(sessions) => HttpResponse::Ok().json(sessions),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}/sessions/{session_id}
async fn remote_get_session(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<(String, String)>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let (agent_id_str, session_id_str) = path.into_inner();
    let agent_id = match parse_uuid(&agent_id_str) { Ok(id) => id, Err(resp) => return resp };
    let session_id = match parse_uuid(&session_id_str) { Ok(id) => id, Err(resp) => return resp };

    match sf.get_session(&agent_id, &session_id).await {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/remote/agents/{agent_id}/query
async fn remote_query_agent(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let message = body.get("message").and_then(|v| v.as_str()).unwrap_or("");
    match sf.query(&agent_id, message).await {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}/hooks
async fn remote_get_hooks(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.get_hooks(&agent_id).await {
        Ok(hooks) => HttpResponse::Ok().json(hooks),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/remote/agents/{agent_id}/fire_hook
async fn remote_fire_hook(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let event = body.get("event").and_then(|v| v.as_str()).unwrap_or("");
    let payload = body.get("payload").cloned().unwrap_or(json!({}));
    let wait = body.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);

    let result = if wait {
        sf.fire_hook_and_wait(&agent_id, event, payload).await
    } else {
        sf.fire_hook(&agent_id, event, payload).await
    };

    match result {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// PUT /api/starflask/remote/agents/{agent_id}/agent-pack
async fn remote_install_pack(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let content_hash = body.get("content_hash").and_then(|v| v.as_str()).unwrap_or("");
    match sf.install_agent_pack(&agent_id, content_hash).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}/integrations
async fn remote_list_integrations(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_integrations(&agent_id).await {
        Ok(integrations) => HttpResponse::Ok().json(integrations),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/remote/agents/{agent_id}/integrations
async fn remote_create_integration(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    match sf.create_integration(&agent_id, platform).await {
        Ok(integration) => HttpResponse::Created().json(integration),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// DELETE /api/starflask/remote/agents/{agent_id}/integrations/{integration_id}
async fn remote_delete_integration(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<(String, String)>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let (agent_id_str, integration_id_str) = path.into_inner();
    let agent_id = match parse_uuid(&agent_id_str) { Ok(id) => id, Err(resp) => return resp };
    let integration_id = match parse_uuid(&integration_id_str) { Ok(id) => id, Err(resp) => return resp };

    match sf.delete_integration(&agent_id, &integration_id).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}/tasks
async fn remote_list_tasks(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_tasks(&agent_id).await {
        Ok(tasks) => HttpResponse::Ok().json(tasks),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// POST /api/starflask/remote/agents/{agent_id}/tasks
async fn remote_create_task(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, body: web::Json<serde_json::Value>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let hook_event = body.get("hook_event").and_then(|v| v.as_str());
    let schedule = body.get("schedule").and_then(|v| v.as_str());
    match sf.create_task(&agent_id, name, hook_event, schedule).await {
        Ok(task) => HttpResponse::Created().json(task),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/agents/{agent_id}/memories
async fn remote_list_memories(
    state: web::Data<AppState>, req: HttpRequest, path: web::Path<String>, query: web::Query<MemoriesQuery>,
) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };
    let agent_id = match parse_uuid(&path.into_inner()) { Ok(id) => id, Err(resp) => return resp };

    match sf.list_memories(&agent_id, query.limit, query.offset).await {
        Ok(memories) => HttpResponse::Ok().json(memories),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}

/// GET /api/starflask/remote/subscription
async fn remote_subscription_status(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = super::validate_session(&state, &req) { return resp; }
    let sf = match require_starflask(&state).await { Ok(sf) => sf, Err(resp) => return resp };

    match sf.get_subscription_status().await {
        Ok(status) => HttpResponse::Ok().json(status),
        Err(e) => HttpResponse::InternalServerError().json(json!({ "error": e.to_string() })),
    }
}
