use actix_web::{web, HttpRequest, HttpResponse, Responder};
use crate::ai::ArchetypeId;
use crate::keystore_client::{KEYSTORE_CLIENT, DEFAULT_KEYSTORE_URL};
use crate::models::{AgentSettings, AgentSettingsResponse, UpdateAgentSettingsRequest, UpdateBotSettingsRequest};
use crate::ai_endpoint_config;
use crate::tools::rpc_config;
use crate::AppState;

/// Validate session token from request
fn validate_session_from_request(
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

/// Get current agent settings (active endpoint)
pub async fn get_agent_settings(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    match state.db.get_active_agent_settings() {
        Ok(Some(settings)) => {
            let response: AgentSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Ok(None) => {
            // Return default kimi settings when none configured
            let response: AgentSettingsResponse = AgentSettings::default().into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            log::error!("Failed to get agent settings: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// List all configured endpoints
pub async fn list_agent_settings(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    match state.db.list_agent_settings() {
        Ok(settings) => {
            let responses: Vec<AgentSettingsResponse> = settings
                .into_iter()
                .map(|s| s.into())
                .collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            log::error!("Failed to list agent settings: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get available archetypes with descriptions
pub async fn get_available_archetypes(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    let archetypes = vec![
        serde_json::json!({
            "id": "kimi",
            "name": "Kimi (Native Tool Calling)",
            "description": "OpenAI-compatible native tool calling. Best for Kimi, OpenAI, and similar endpoints.",
            "uses_native_tools": true,
        }),
        serde_json::json!({
            "id": "llama",
            "name": "Llama (Text-based Tool Calling)",
            "description": "JSON-based tool calling via text. Best for generic Llama endpoints.",
            "uses_native_tools": false,
        }),
        serde_json::json!({
            "id": "claude",
            "name": "Claude (Native Tool Calling)",
            "description": "Anthropic Claude native tool calling.",
            "uses_native_tools": true,
        }),
        serde_json::json!({
            "id": "openai",
            "name": "OpenAI (Native Tool Calling)",
            "description": "OpenAI native tool calling. Same as Kimi.",
            "uses_native_tools": true,
        }),
    ];

    HttpResponse::Ok().json(archetypes)
}

/// Update agent settings (set active endpoint)
pub async fn update_agent_settings(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<UpdateAgentSettingsRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    let request = body.into_inner();

    // Validate endpoint
    if request.endpoint.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Endpoint URL is required"
        }));
    }

    // Validate archetype
    if ArchetypeId::from_str(&request.model_archetype).is_none() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Invalid archetype: {}. Must be kimi, llama, claude, openai, or minimax.", request.model_archetype)
        }));
    }

    // Save settings
    log::info!(
        "Saving agent settings: endpoint={}, archetype={}, max_response_tokens={}, max_context_tokens={}, has_secret_key={}",
        request.endpoint,
        request.model_archetype,
        request.max_response_tokens,
        request.max_context_tokens,
        request.secret_key.is_some()
    );

    match state.db.save_agent_settings(&request.endpoint, &request.model_archetype, request.model.as_deref(), request.max_response_tokens, request.max_context_tokens, request.secret_key.as_deref()) {
        Ok(settings) => {
            log::info!("Updated agent settings to use {} endpoint with {} archetype", request.endpoint, request.model_archetype);
            let response: AgentSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            log::error!("Failed to save agent settings: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Disable agent (set no active endpoint)
pub async fn disable_agent(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    match state.db.disable_agent_settings() {
        Ok(_) => {
            log::info!("Disabled AI agent");
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "AI agent disabled"
            }))
        }
        Err(e) => {
            log::error!("Failed to disable agent: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get bot settings
pub async fn get_bot_settings(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    match state.db.get_bot_settings() {
        Ok(settings) => HttpResponse::Ok().json(settings),
        Err(e) => {
            log::error!("Failed to get bot settings: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Update bot settings
pub async fn update_bot_settings(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<UpdateBotSettingsRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    let request = body.into_inner();

    // Validate rpc_provider if provided
    if let Some(ref provider) = request.rpc_provider {
        if provider != "custom" && rpc_config::get_rpc_provider(provider).is_none() {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid RPC provider: {}. Valid options: defirelay, custom", provider)
            }));
        }
    }

    // Update KEYSTORE_CLIENT URL if keystore_url is being changed
    if let Some(ref url) = request.keystore_url {
        let new_url = if url.is_empty() { DEFAULT_KEYSTORE_URL } else { url.as_str() };
        KEYSTORE_CLIENT.set_base_url(new_url).await;
        log::info!("Keystore URL updated to: {}", new_url);
    }

    match state.db.update_bot_settings_full(
        request.bot_name.as_deref(),
        request.bot_email.as_deref(),
        request.web3_tx_requires_confirmation,
        request.rpc_provider.as_deref(),
        request.custom_rpc_endpoints.as_ref(),
        request.max_tool_iterations,
        request.rogue_mode_enabled,
        request.safe_mode_max_queries_per_10min,
        request.keystore_url.as_deref(),
        request.chat_session_memory_generation,
        request.guest_dashboard_enabled,
        request.theme_accent.as_deref(),
        request.proxy_url.as_deref(),
        request.kanban_auto_execute,
    ) {
        Ok(settings) => {
            log::info!(
                "Updated bot settings: name={}, email={}, rpc_provider={}",
                settings.bot_name,
                settings.bot_email,
                settings.rpc_provider
            );
            HttpResponse::Ok().json(settings)
        }
        Err(e) => {
            log::error!("Failed to update bot settings: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get auto-sync status for the current wallet
pub async fn get_auto_sync_status(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Get wallet address from wallet provider
    let wallet_address = match &state.wallet_provider {
        Some(provider) => provider.get_address().to_lowercase(),
        None => {
            return HttpResponse::Ok().json(serde_json::json!({
                "status": null,
                "message": "No wallet configured",
                "keystore_url": KEYSTORE_CLIENT.get_base_url().await
            }));
        }
    };

    match state.db.get_auto_sync_status(&wallet_address) {
        Ok(Some(status)) => {
            HttpResponse::Ok().json(serde_json::json!({
                "status": status.status,
                "message": status.message,
                "synced_at": status.synced_at,
                "key_count": status.key_count,
                "node_count": status.node_count,
                "keystore_url": KEYSTORE_CLIENT.get_base_url().await
            }))
        }
        Ok(None) => {
            HttpResponse::Ok().json(serde_json::json!({
                "status": null,
                "message": "No auto-sync has been attempted yet",
                "keystore_url": KEYSTORE_CLIENT.get_base_url().await
            }))
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get available RPC providers
pub async fn get_rpc_providers(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let mut providers: Vec<serde_json::Value> = rpc_config::list_rpc_providers()
        .into_iter()
        .map(|(id, provider)| {
            serde_json::json!({
                "id": id,
                "display_name": provider.display_name,
                "description": provider.description,
                "x402": provider.x402,
                "networks": provider.endpoints.keys().collect::<Vec<_>>(),
            })
        })
        .collect();

    // Add "custom" option
    providers.push(serde_json::json!({
        "id": "custom",
        "display_name": "Custom",
        "description": "User-provided RPC endpoints (no x402 payment)",
        "x402": false,
        "networks": ["base", "mainnet"],
    }));

    HttpResponse::Ok().json(providers)
}

/// Get available AI endpoint presets
pub async fn get_ai_endpoint_presets(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let presets: Vec<serde_json::Value> = ai_endpoint_config::list_ai_endpoints()
        .into_iter()
        .map(|(id, preset)| {
            serde_json::json!({
                "id": id,
                "display_name": preset.display_name,
                "endpoint": preset.endpoint,
                "model_archetype": preset.model_archetype,
                "model": preset.model,
                "x402_cost": preset.x402_cost,
            })
        })
        .collect();

    HttpResponse::Ok().json(presets)
}

/// Configure routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/agent-settings")
            .route("", web::get().to(get_agent_settings))
            .route("", web::put().to(update_agent_settings))
            .route("/list", web::get().to(list_agent_settings))
            .route("/archetypes", web::get().to(get_available_archetypes))
            .route("/endpoints", web::get().to(get_ai_endpoint_presets))
            .route("/disable", web::post().to(disable_agent))
    );
    cfg.service(
        web::scope("/api/bot-settings")
            .route("", web::get().to(get_bot_settings))
            .route("", web::put().to(update_bot_settings))
    );
    cfg.service(
        web::resource("/api/rpc-providers")
            .route(web::get().to(get_rpc_providers))
    );
    cfg.service(
        web::resource("/api/auto-sync-status")
            .route(web::get().to(get_auto_sync_status))
    );
}
