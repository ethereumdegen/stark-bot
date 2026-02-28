use actix_web::{web, HttpRequest, HttpResponse, Responder};
use crate::ai::ArchetypeId;
use crate::keystore_client::{KEYSTORE_CLIENT, DEFAULT_KEYSTORE_URL};
use crate::models::{AgentSettings, AgentSettingsResponse, UpdateAgentSettingsRequest, UpdateBotSettingsRequest, DEFAULT_EMBEDDINGS_SERVER_URL, DEFAULT_WHISPER_SERVER_URL};
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
        serde_json::json!({
            "id": "minimax",
            "name": "MiniMax (Native Tool Calling)",
            "description": "MiniMax native tool calling for MiniMax and Morpheus models.",
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

    // Validate payment_mode if provided
    let payment_mode = request.payment_mode.as_deref().unwrap_or("credits");
    if !["none", "credits", "custom"].contains(&payment_mode) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Invalid payment_mode: {}. Must be none, credits, or custom.", payment_mode)
        }));
    }

    // If payment_mode is "none", disable AI and return early
    if payment_mode == "none" {
        match state.db.disable_agent_settings() {
            Ok(_) => {
                log::info!("Disabled AI agent (payment_mode=none)");
                let response: AgentSettingsResponse = AgentSettings::default().into();
                return HttpResponse::Ok().json(response);
            }
            Err(e) => {
                log::error!("Failed to disable agent: {}", e);
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Database error: {}", e)
                }));
            }
        }
    }

    // Validate endpoint
    if request.endpoint.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Endpoint URL is required"
        }));
    }

    // Validate archetype
    if ArchetypeId::from_str(&request.model_archetype).is_none() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Invalid archetype: {}. Must be kimi, llama, claude, openai, minimax, or standard.", request.model_archetype)
        }));
    }

    // Save settings
    log::info!(
        "Saving agent settings: endpoint_name={:?}, endpoint={}, archetype={}, max_response_tokens={}, max_context_tokens={}, has_secret_key={}, payment_mode={}",
        request.endpoint_name,
        request.endpoint,
        request.model_archetype,
        request.max_response_tokens,
        request.max_context_tokens,
        request.secret_key.is_some(),
        payment_mode
    );

    match state.db.save_agent_settings(request.endpoint_name.as_deref(), &request.endpoint, &request.model_archetype, request.model.as_deref(), request.max_response_tokens, request.max_context_tokens, request.secret_key.as_deref(), payment_mode) {
        Ok(settings) => {
            log::info!("Updated agent settings to use {:?} / {} endpoint with {} archetype", request.endpoint_name, request.endpoint, request.model_archetype);
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

    // Update live embeddings generator URL if embeddings_server_url is being changed
    if let Some(ref url) = request.embeddings_server_url {
        let resolved_url = if url.is_empty() { DEFAULT_EMBEDDINGS_SERVER_URL } else { url.as_str() };
        if let Some(ref emb_gen) = state.remote_embedding_generator {
            emb_gen.update_server_url(resolved_url);
            log::info!("Embeddings server URL updated live to: {}", resolved_url);
        }
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
        request.whisper_server_url.as_deref(),
        request.embeddings_server_url.as_deref(),
    ) {
        Ok(settings) => {
            log::info!(
                "Updated bot settings: name={}, email={}, rpc_provider={}",
                settings.bot_name,
                settings.bot_email,
                settings.rpc_provider
            );
            // Sync custom RPC endpoints to the global resolver so all codepaths pick them up
            if let Some(ref endpoints) = settings.custom_rpc_endpoints {
                crate::tools::rpc_config::set_custom_rpc_endpoints(endpoints.clone());
            }
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

/// Health check for infrastructure services (whisper + embeddings)
pub async fn services_health(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let settings = state.db.get_bot_settings().unwrap_or_default();
    let whisper_url = settings.whisper_server_url
        .unwrap_or_else(|| DEFAULT_WHISPER_SERVER_URL.to_string());
    let embeddings_url = settings.embeddings_server_url
        .unwrap_or_else(|| DEFAULT_EMBEDDINGS_SERVER_URL.to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let whisper_check = {
        let url = format!("{}/health", whisper_url.trim_end_matches('/'));
        let client = client.clone();
        async move { client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false) }
    };

    let embeddings_check = {
        let url = format!("{}/health", embeddings_url.trim_end_matches('/'));
        let client = client.clone();
        async move { client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false) }
    };

    let (whisper_healthy, embeddings_healthy) = tokio::join!(whisper_check, embeddings_check);

    HttpResponse::Ok().json(serde_json::json!({
        "whisper": { "url": whisper_url, "healthy": whisper_healthy },
        "embeddings": { "url": embeddings_url, "healthy": embeddings_healthy },
    }))
}

/// Get credit balance from the inference super-router via ERC-8128 signed request.
///
/// The bot signs a GET /credits/balance request using its wallet, and the
/// super-router recovers the address to look up credits.
pub async fn get_credit_balance(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "No wallet configured"
            }));
        }
    };

    // Determine the inference endpoint base URL from active settings
    let base_url = match state.db.get_active_agent_settings() {
        Ok(Some(settings)) if settings.endpoint.contains("defirelay.com") => {
            // Extract origin (scheme + host) from endpoint
            // e.g. "https://inference.defirelay.com/minimax/api/v1/chat/completions" -> "https://inference.defirelay.com"
            if let Ok(url) = reqwest::Url::parse(&settings.endpoint) {
                format!("{}://{}", url.scheme(), url.host_str().unwrap_or("inference.defirelay.com"))
            } else {
                "https://inference.defirelay.com".to_string()
            }
        }
        _ => "https://inference.defirelay.com".to_string(),
    };

    // Sign the request with ERC-8128
    let signer = crate::erc8128::Erc8128Signer::new(wallet_provider, 8453);
    let authority = base_url
        .strip_prefix("https://")
        .or_else(|| base_url.strip_prefix("http://"))
        .unwrap_or(&base_url)
        .split('/')
        .next()
        .unwrap_or("inference.defirelay.com");

    let signed = match signer
        .sign_request("GET", authority, "/credits/balance", None, None)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to sign credits balance request: {}", e);
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to sign request"
            }));
        }
    };

    // Make the request
    let url = format!("{}/credits/balance", base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut req_builder = client.get(&url);
    req_builder = req_builder.header("signature-input", &signed.signature_input);
    req_builder = req_builder.header("signature", &signed.signature);
    if let Some(ref digest) = signed.content_digest {
        req_builder = req_builder.header("content-digest", digest);
    }

    match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) => {
                    if status.is_success() {
                        log::info!("Credits balance response: {}", body);
                        match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(json) => HttpResponse::Ok().json(json),
                            Err(_) => HttpResponse::Ok().json(serde_json::json!({
                                "error": "Invalid response from credits service"
                            })),
                        }
                    } else {
                        log::warn!("Credits balance request failed: {} {}", status, body);
                        HttpResponse::Ok().json(serde_json::json!({
                            "error": format!("Credits service returned {}", status)
                        }))
                    }
                }
                Err(e) => {
                    log::error!("Failed to read credits response: {}", e);
                    HttpResponse::Ok().json(serde_json::json!({
                        "error": "Failed to read response"
                    }))
                }
            }
        }
        Err(e) => {
            log::error!("Credits balance request failed: {}", e);
            HttpResponse::Ok().json(serde_json::json!({
                "error": "Failed to connect to credits service"
            }))
        }
    }
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
            .route("/credit-balance", web::get().to(get_credit_balance))
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
    cfg.service(
        web::resource("/api/services/health")
            .route(web::get().to(services_health))
    );
}
