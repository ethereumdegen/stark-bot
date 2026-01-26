use actix_web::{web, HttpRequest, HttpResponse, Responder};
use crate::models::{AgentSettingsResponse, AiProvider, UpdateAgentSettingsRequest};
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

/// Get current agent settings (active provider)
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
            HttpResponse::Ok().json(serde_json::json!({
                "configured": false,
                "message": "No AI provider configured"
            }))
        }
        Err(e) => {
            log::error!("Failed to get agent settings: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// List all configured providers
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

/// Get available providers with defaults
pub async fn get_available_providers(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    let providers = vec![
        serde_json::json!({
            "id": "claude",
            "name": "Claude (Anthropic)",
            "placeholder_endpoint": AiProvider::Claude.placeholder_endpoint(),
            "placeholder_model": AiProvider::Claude.placeholder_model(),
        }),
        serde_json::json!({
            "id": "openai",
            "name": "OpenAI",
            "placeholder_endpoint": AiProvider::OpenAI.placeholder_endpoint(),
            "placeholder_model": AiProvider::OpenAI.placeholder_model(),
        }),
        serde_json::json!({
            "id": "openai_compatible",
            "name": "OpenAI Compatible (DigitalOcean, Azure, etc.)",
            "placeholder_endpoint": AiProvider::OpenAICompatible.placeholder_endpoint(),
            "placeholder_model": AiProvider::OpenAICompatible.placeholder_model(),
        }),
        serde_json::json!({
            "id": "llama",
            "name": "Llama (Ollama)",
            "placeholder_endpoint": AiProvider::Llama.placeholder_endpoint(),
            "placeholder_model": AiProvider::Llama.placeholder_model(),
        }),
    ];

    HttpResponse::Ok().json(providers)
}

/// Update agent settings (set active provider)
pub async fn update_agent_settings(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<UpdateAgentSettingsRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }
    let request = body.into_inner();

    // Validate provider
    let provider = match AiProvider::from_str(&request.provider) {
        Some(p) => p,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid provider: {}. Must be claude, openai, or llama.", request.provider)
            }));
        }
    };

    // Validate endpoint
    if request.endpoint.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Endpoint URL is required"
        }));
    }

    // Get existing settings to preserve API key if not provided
    let existing_api_key = state.db.get_agent_settings_by_provider(&request.provider)
        .ok()
        .flatten()
        .map(|s| s.api_key)
        .unwrap_or_default();

    // Use provided API key, or fall back to existing one
    let api_key = if request.api_key.is_empty() {
        existing_api_key
    } else {
        request.api_key.clone()
    };

    // API key is required for Claude, OpenAI, and OpenAI-compatible; optional for Llama
    if api_key.is_empty() && provider != AiProvider::Llama {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "API key is required for this provider"
        }));
    }

    // Model is optional - use provided or empty string (some endpoints auto-select)
    let model = request.model.clone().unwrap_or_default();

    // Save settings
    log::info!(
        "Saving agent settings: provider={}, endpoint={}, api_key_len={}, model={}",
        request.provider,
        request.endpoint,
        api_key.len(),
        model
    );

    match state.db.save_agent_settings(
        &request.provider,
        &request.endpoint,
        &api_key,
        &model,
        request.bot_name.as_deref(),
        request.bot_email.as_deref(),
    ) {
        Ok(settings) => {
            log::info!("Updated agent settings to use {} provider, api_key stored: {}", request.provider, !settings.api_key.is_empty());
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

/// Disable agent (set no active provider)
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

/// Configure routes
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/agent-settings")
            .route("", web::get().to(get_agent_settings))
            .route("", web::put().to(update_agent_settings))
            .route("/list", web::get().to(list_agent_settings))
            .route("/providers", web::get().to(get_available_providers))
            .route("/disable", web::post().to(disable_agent))
    );
}
