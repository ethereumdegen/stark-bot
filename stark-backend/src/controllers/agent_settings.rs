use actix_web::{web, HttpResponse, Responder};
use crate::models::{AgentSettingsResponse, AiProvider, UpdateAgentSettingsRequest};
use crate::AppState;

/// Get current agent settings (active provider)
pub async fn get_agent_settings(
    state: web::Data<AppState>,
) -> impl Responder {
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
) -> impl Responder {
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
pub async fn get_available_providers() -> impl Responder {
    let providers = vec![
        serde_json::json!({
            "id": "claude",
            "name": "Claude (Anthropic)",
            "default_endpoint": AiProvider::Claude.default_endpoint(),
            "default_model": AiProvider::Claude.default_model(),
        }),
        serde_json::json!({
            "id": "openai",
            "name": "OpenAI",
            "default_endpoint": AiProvider::OpenAI.default_endpoint(),
            "default_model": AiProvider::OpenAI.default_model(),
        }),
        serde_json::json!({
            "id": "llama",
            "name": "Llama (Ollama)",
            "default_endpoint": AiProvider::Llama.default_endpoint(),
            "default_model": AiProvider::Llama.default_model(),
        }),
    ];

    HttpResponse::Ok().json(providers)
}

/// Update agent settings (set active provider)
pub async fn update_agent_settings(
    state: web::Data<AppState>,
    body: web::Json<UpdateAgentSettingsRequest>,
) -> impl Responder {
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

    // API key is required for Claude and OpenAI, optional for Llama
    if request.api_key.is_empty() && provider != AiProvider::Llama {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "API key is required for this provider"
        }));
    }

    // Use provided model or default
    let model = request.model.unwrap_or_else(|| provider.default_model().to_string());

    // Save settings
    match state.db.save_agent_settings(&request.provider, &request.endpoint, &request.api_key, &model) {
        Ok(settings) => {
            log::info!("Updated agent settings to use {} provider", request.provider);
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
) -> impl Responder {
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
