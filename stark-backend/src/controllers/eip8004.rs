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

/// Get our agent's identity (reads metadata from DB, optionally enriches with on-chain data)
async fn get_our_identity(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_auth(&state, &req) {
        return resp;
    }

    let config = Eip8004Config::from_env();

    // Read identity from DB (single source of truth)
    let row = match state.db.get_agent_identity_full() {
        Some(r) => r,
        None => {
            return HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "registered": false,
                "config": {
                    "chain_id": config.chain_id,
                    "identity_registry": config.identity_registry,
                    "deployed": config.is_identity_deployed()
                }
            }));
        }
    };

    let agent_id = row.agent_id;
    let agent_registry = row.agent_registry.clone();
    let chain_id = row.chain_id;
    let name = row.name.clone();
    let description = row.description.clone();
    let image = row.image.clone();
    let is_active = row.active;
    let x402_support = row.x402_support;
    let registration_uri = row.registration_uri.clone();
    let services: serde_json::Value = serde_json::from_str(&row.services_json).unwrap_or(serde_json::json!([]));

    // On-chain enrichment (wallet_address, owner_address) — optional, only if registry is deployed
    let mut wallet_address = String::new();
    let mut owner_address = String::new();
    let mut fetch_errors: Vec<String> = Vec::new();

    if agent_id > 0 && config.is_identity_deployed() {
        let rpc = match config.build_rpc(state.wallet_provider.clone()) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[eip8004/identity] Failed to build RPC: {}", e);
                return HttpResponse::InternalServerError().json(serde_json::json!({"error": e}));
            }
        };
        let registry = IdentityRegistry::new(config.clone(), rpc);

        match registry.get_owner(agent_id as u64).await {
            Ok(o) => owner_address = o,
            Err(e) => fetch_errors.push(format!("owner: {}", e)),
        }

        match registry.get_agent_wallet(agent_id as u64).await {
            Ok(w) => wallet_address = w,
            Err(e) => fetch_errors.push(format!("wallet: {}", e)),
        }

        if !fetch_errors.is_empty() {
            log::warn!("[eip8004/identity] Fetch errors for agent_id={}: {:?}", agent_id, fetch_errors);
        }
    }

    let registered = agent_id > 0 && !agent_registry.is_empty();
    let local_identity = !registered && (name.is_some() || description.is_some());

    // Use config chain_id for display when DB has chain_id=0 (local-only identity)
    let display_chain_id = if chain_id > 0 { chain_id } else { config.chain_id as i64 };
    let display_registry = if agent_registry.is_empty() {
        format!("{:?}", config.identity_registry)
    } else {
        agent_registry.clone()
    };

    let mut resp = serde_json::json!({
        "success": true,
        "registered": registered,
        "local_identity": local_identity,
        "identity": {
            "agent_id": if agent_id > 0 { serde_json::json!(agent_id) } else { serde_json::json!(null) },
            "agent_registry": display_registry,
            "chain_id": display_chain_id,
            "name": name,
            "description": description,
            "image": image,
            "registration_uri": registration_uri,
            "wallet_address": wallet_address,
            "owner_address": owner_address,
            "is_active": is_active,
            "x402_support": x402_support,
            "services": services,
        }
    });

    if local_identity {
        resp["warning"] = serde_json::json!(
            "This identity exists locally but is not linked to an on-chain NFT. Use import_identity to link an existing NFT, or register on-chain to mint a new one."
        );
    }

    if !fetch_errors.is_empty() {
        resp["fetch_errors"] = serde_json::json!(fetch_errors);
    }

    HttpResponse::Ok().json(resp)
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

    let rpc = match config.build_rpc(state.wallet_provider.clone()) {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    };
    let registry = IdentityRegistry::new(config, rpc);

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

    let rpc = match config.build_rpc(state.wallet_provider.clone()) {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    };
    let registry = ReputationRegistry::new(config, rpc);

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

    let rpc = match config.build_rpc(state.wallet_provider.clone()) {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e)),
    };
    let registry = ReputationRegistry::new(config, rpc);

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

    let (identity_rpc, reputation_rpc) = match (
        config.build_rpc(state.wallet_provider.clone()),
        config.build_rpc(state.wallet_provider.clone()),
    ) {
        (Ok(i), Ok(r)) => (i, r),
        (Err(e), _) | (_, Err(e)) => {
            return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e));
        }
    };
    let mut discovery = AgentDiscovery::new(config, identity_rpc, reputation_rpc);
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

    let (identity_rpc, reputation_rpc) = match (
        config.build_rpc(state.wallet_provider.clone()),
        config.build_rpc(state.wallet_provider.clone()),
    ) {
        (Ok(i), Ok(r)) => (i, r),
        (Err(e), _) | (_, Err(e)) => {
            return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e));
        }
    };
    let mut discovery = AgentDiscovery::new(config, identity_rpc, reputation_rpc);

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

    let (identity_rpc, reputation_rpc) = match (
        config.build_rpc(state.wallet_provider.clone()),
        config.build_rpc(state.wallet_provider.clone()),
    ) {
        (Ok(i), Ok(r)) => (i, r),
        (Err(e), _) | (_, Err(e)) => {
            return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e));
        }
    };
    let mut discovery = AgentDiscovery::new(config, identity_rpc, reputation_rpc);

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
