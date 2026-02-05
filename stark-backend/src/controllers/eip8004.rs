//! EIP-8004 Trustless Agents API endpoints
//!
//! Endpoints for identity, reputation, and discovery.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::eip8004::{
    config::Eip8004Config,
    discovery::{AgentDiscovery, SearchCriteria},
    identity::{IdentityRegistry, RegistrationBuilder},
    reputation::ReputationRegistry,
    types::TrustLevel,
};
use crate::AppState;

// =====================================================
// Response Types
// =====================================================

#[derive(Debug, Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(msg: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.to_string()),
        }
    }
}

// =====================================================
// Request Types
// =====================================================

#[derive(Debug, Deserialize)]
pub struct DiscoverQuery {
    offset: Option<u64>,
    limit: Option<u64>,
    x402_only: Option<bool>,
    service: Option<String>,
    min_reputation: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRegistrationRequest {
    name: String,
    description: String,
    image: Option<String>,
    services: Option<Vec<ServiceInput>>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceInput {
    name: String,
    endpoint: String,
    version: String,
}

// =====================================================
// Route Configuration
// =====================================================

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/eip8004")
            // Configuration
            .route("/config", web::get().to(get_config))
            // Identity
            .route("/identity", web::get().to(get_our_identity))
            .route("/identity/registration", web::post().to(create_registration_json))
            .route("/identity/{agent_id}", web::get().to(get_agent_identity))
            // Reputation
            .route("/reputation/{agent_id}", web::get().to(get_agent_reputation))
            .route("/reputation/{agent_id}/trust", web::get().to(check_trust))
            // Discovery
            .route("/agents", web::get().to(discover_agents))
            .route("/agents/search", web::get().to(search_agents))
            .route("/agents/{agent_id}", web::get().to(get_agent_details))
    );
}

// =====================================================
// Configuration Endpoints
// =====================================================

/// Get EIP-8004 configuration
async fn get_config(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let config = Eip8004Config::from_env();

    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "config": {
            "chain_id": config.chain_id,
            "chain_name": config.chain_name,
            "identity_registry": config.identity_registry,
            "reputation_registry": config.reputation_registry,
            "validation_registry": config.validation_registry,
            "identity_deployed": config.is_identity_deployed(),
            "reputation_deployed": config.is_reputation_deployed(),
            "explorer_url": config.explorer_url
        }
    }))
}

// =====================================================
// Identity Endpoints
// =====================================================

/// Get our agent's identity
async fn get_our_identity(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let conn = state.db.conn();

    // Check if we have a stored identity
    let identity = conn.query_row(
        "SELECT agent_id, agent_registry, chain_id, registration_uri, wallet_address, name, is_active, created_at
         FROM agent_identity ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            Ok(serde_json::json!({
                "agent_id": row.get::<_, i64>(0)?,
                "agent_registry": row.get::<_, String>(1)?,
                "chain_id": row.get::<_, i64>(2)?,
                "registration_uri": row.get::<_, Option<String>>(3)?,
                "wallet_address": row.get::<_, String>(4)?,
                "name": row.get::<_, Option<String>>(5)?,
                "is_active": row.get::<_, i64>(6)? != 0,
                "created_at": row.get::<_, String>(7)?
            }))
        },
    );

    match identity {
        Ok(id) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "registered": true,
            "identity": id
        })),
        Err(_) => {
            // Not registered yet
            let config = Eip8004Config::from_env();
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "registered": false,
                "config": {
                    "chain_id": config.chain_id,
                    "identity_registry": config.identity_registry,
                    "deployed": config.is_identity_deployed()
                }
            }))
        }
    }
}

/// Get agent identity by ID
async fn get_agent_identity(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<u64>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let agent_id = path.into_inner();
    let config = Eip8004Config::from_env();

    if !config.is_identity_deployed() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Identity Registry not deployed"));
    }

    let registry = if let Some(ref wp) = state.wallet_provider {
        IdentityRegistry::new_with_wallet_provider(config, wp.clone())
    } else {
        IdentityRegistry::new(config)
    };

    match registry.get_agent_details(agent_id).await {
        Ok(agent) => HttpResponse::Ok().json(ApiResponse::success(agent)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}

/// Create registration JSON file
async fn create_registration_json(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateRegistrationRequest>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let mut builder = RegistrationBuilder::new(&body.name, &body.description);

    if let Some(ref image) = body.image {
        builder = builder.image(image);
    }

    if let Some(ref services) = body.services {
        for service in services {
            builder = builder.service(&service.name, &service.endpoint, &service.version);
        }
    }

    let registration = builder.build();

    match serde_json::to_string_pretty(&registration) {
        Ok(json) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "registration": registration,
            "json": json
        })),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&format!("Failed to serialize: {}", e))),
    }
}

// =====================================================
// Reputation Endpoints
// =====================================================

/// Get agent reputation
async fn get_agent_reputation(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<u64>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let agent_id = path.into_inner();
    let config = Eip8004Config::from_env();

    if !config.is_reputation_deployed() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Reputation Registry not deployed"));
    }

    let registry = if let Some(ref wp) = state.wallet_provider {
        ReputationRegistry::new_with_wallet_provider(config, wp.clone())
    } else {
        ReputationRegistry::new(config)
    };

    match registry.get_summary(agent_id, &[], "", "").await {
        Ok(summary) => HttpResponse::Ok().json(ApiResponse::success(summary)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}

/// Check agent trust level
async fn check_trust(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<u64>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let agent_id = path.into_inner();
    let config = Eip8004Config::from_env();

    if !config.is_reputation_deployed() {
        return HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "agent_id": agent_id,
            "trust_level": "unverified",
            "reason": "Reputation Registry not deployed"
        }));
    }

    let registry = if let Some(ref wp) = state.wallet_provider {
        ReputationRegistry::new_with_wallet_provider(config, wp.clone())
    } else {
        ReputationRegistry::new(config)
    };

    match registry.get_summary(agent_id, &[], "", "").await {
        Ok(summary) => {
            let trust_level = summary.trust_level();
            let should_trust = matches!(trust_level, TrustLevel::High | TrustLevel::Medium);

            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "agent_id": agent_id,
                "trust_level": trust_level.to_string(),
                "should_trust": should_trust,
                "reputation": {
                    "count": summary.count,
                    "average_score": summary.average_score
                }
            }))
        }
        Err(e) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "agent_id": agent_id,
            "trust_level": "unverified",
            "reason": e
        })),
    }
}

// =====================================================
// Discovery Endpoints
// =====================================================

/// Discover agents
async fn discover_agents(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<DiscoverQuery>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let config = Eip8004Config::from_env();

    if !config.is_identity_deployed() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Identity Registry not deployed"));
    }

    let mut discovery = if let Some(ref wp) = state.wallet_provider {
        AgentDiscovery::new_with_wallet_provider(config, wp.clone())
    } else {
        AgentDiscovery::new(config)
    };
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).min(100);

    match discovery.discover_all(offset, limit).await {
        Ok(agents) => {
            let total = discovery.total_agents().await.unwrap_or(0);
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "agents": agents,
                "total": total,
                "offset": offset,
                "limit": limit
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}

/// Search for agents with criteria
async fn search_agents(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<DiscoverQuery>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let config = Eip8004Config::from_env();

    if !config.is_identity_deployed() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Identity Registry not deployed"));
    }

    let mut discovery = if let Some(ref wp) = state.wallet_provider {
        AgentDiscovery::new_with_wallet_provider(config, wp.clone())
    } else {
        AgentDiscovery::new(config)
    };

    let criteria = SearchCriteria {
        x402_required: query.x402_only.unwrap_or(false),
        active_only: true,
        required_service: query.service.clone(),
        min_reputation_count: query.min_reputation,
        sort_by_reputation: true,
        limit: Some(query.limit.unwrap_or(50) as usize),
        ..Default::default()
    };

    match discovery.search(criteria).await {
        Ok(agents) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "agents": agents,
            "count": agents.len()
        })),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}

/// Get full agent details
async fn get_agent_details(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<u64>,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let agent_id = path.into_inner();
    let config = Eip8004Config::from_env();

    if !config.is_identity_deployed() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Identity Registry not deployed"));
    }

    let mut discovery = if let Some(ref wp) = state.wallet_provider {
        AgentDiscovery::new_with_wallet_provider(config, wp.clone())
    } else {
        AgentDiscovery::new(config)
    };

    match discovery.discover_agent(agent_id).await {
        Ok(agent) => HttpResponse::Ok().json(ApiResponse::success(agent)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    }
}

// =====================================================
// Auth Helper
// =====================================================

fn validate_auth(state: &web::Data<AppState>, req: &HttpRequest) -> Result<(), HttpResponse> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(HttpResponse::Unauthorized().json(serde_json::json!({
                "success": false,
                "error": "No authorization token provided"
            })));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "error": "Invalid or expired session"
        }))),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": "Internal server error"
            })))
        }
    }
}
