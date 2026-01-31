use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator};

use crate::models::ApiKeyResponse;
use crate::AppState;

/// Enum of all valid API key identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr)]
pub enum ApiKeyId {
    #[strum(serialize = "GITHUB_TOKEN")]
    GithubToken,
    #[strum(serialize = "BANKR_API_KEY")]
    BankrApiKey,
    #[strum(serialize = "TWITTER_TOKEN")]
    TwitterToken,
    #[strum(serialize = "DISCORD_BOT_TOKEN")]
    DiscordBotToken,
    #[strum(serialize = "TELEGRAM_BOT_TOKEN")]
    TelegramBotToken,
    #[strum(serialize = "SLACK_BOT_TOKEN")]
    SlackBotToken,
    #[strum(serialize = "MOLTBOOK_TOKEN")]
    MoltbookToken,
}

impl ApiKeyId {
    /// The key name as stored in the database
    pub fn as_str(&self) -> &'static str {
        // AsRefStr from strum provides static string references
        match self {
            Self::GithubToken => "GITHUB_TOKEN",
            Self::BankrApiKey => "BANKR_API_KEY",
            Self::TwitterToken => "TWITTER_TOKEN",
            Self::DiscordBotToken => "DISCORD_BOT_TOKEN",
            Self::TelegramBotToken => "TELEGRAM_BOT_TOKEN",
            Self::SlackBotToken => "SLACK_BOT_TOKEN",
            Self::MoltbookToken => "MOLTBOOK_TOKEN",
        }
    }

    /// Environment variable names to set when this key is available
    pub fn env_vars(&self) -> Option<&'static [&'static str]> {
        match self {
            Self::GithubToken => Some(&["GH_TOKEN", "GITHUB_TOKEN"]),
            Self::BankrApiKey => Some(&["BANKR_API_KEY"]),
            Self::TwitterToken => Some(&["TWITTER_TOKEN"]),
            Self::DiscordBotToken => Some(&["DISCORD_BOT_TOKEN", "DISCORD_TOKEN"]),
            Self::TelegramBotToken => Some(&["TELEGRAM_BOT_TOKEN", "TELEGRAM_TOKEN"]),
            Self::SlackBotToken => Some(&["SLACK_BOT_TOKEN", "SLACK_TOKEN"]),
            Self::MoltbookToken => Some(&["MOLTBOOK_TOKEN"]),
        }
    }

    /// Whether this key requires special git configuration when set
    pub fn requires_git_config(&self) -> bool {
        matches!(self, Self::GithubToken)
    }

    /// Iterate over all API key variants
    pub fn iter() -> impl Iterator<Item = ApiKeyId> {
        <Self as IntoEnumIterator>::iter()
    }

    /// Get all variants as a slice (for backwards compatibility)
    pub fn all() -> Vec<ApiKeyId> {
        Self::iter().collect()
    }

    /// Get all key names as strings
    pub fn all_names() -> Vec<&'static str> {
        Self::iter().map(|k| k.as_str()).collect()
    }
}

/// Configuration for a single key within a service group
#[derive(Debug, Clone, Serialize)]
pub struct KeyConfig {
    pub name: &'static str,
    pub label: &'static str,
    pub secret: bool,
}

/// Configuration for a service group (e.g., "twitter" groups TWITTER_CLIENT_ID and TWITTER_CLIENT_SECRET)
#[derive(Debug, Clone, Serialize)]
pub struct ServiceConfig {
    pub group: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub url: &'static str,
    pub keys: Vec<KeyConfig>,
}

/// Get all service configurations
pub fn get_service_configs() -> Vec<ServiceConfig> {
    vec![
        ServiceConfig {
            group: "github",
            label: "GitHub",
            description: "Create a Personal Access Token with repo scope",
            url: "https://github.com/settings/tokens",
            keys: vec![KeyConfig {
                name: "GITHUB_TOKEN",
                label: "Personal Access Token",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "twitter",
            label: "Twitter/X",
            description: "Get a User Access Token from the Developer Portal to post tweets.",
            url: "https://developer.x.com/en/portal/dashboard",
            keys: vec![
                KeyConfig {
                    name: "TWITTER_TOKEN",
                    label: "User Access Token",
                    secret: true,
                },
            ],
        },
        ServiceConfig {
            group: "bankr",
            label: "Bankr",
            description: "Generate an API key with Agent API access enabled",
            url: "https://bankr.bot/api",
            keys: vec![KeyConfig {
                name: "BANKR_API_KEY",
                label: "API Key",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "discord",
            label: "Discord",
            description: "Create a Bot application and copy its token",
            url: "https://discord.com/developers/applications",
            keys: vec![KeyConfig {
                name: "DISCORD_BOT_TOKEN",
                label: "Bot Token",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "telegram",
            label: "Telegram",
            description: "Create a bot via @BotFather and copy the token",
            url: "https://t.me/BotFather",
            keys: vec![KeyConfig {
                name: "TELEGRAM_BOT_TOKEN",
                label: "Bot Token",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "slack",
            label: "Slack",
            description: "Create a Slack App and copy the Bot User OAuth Token",
            url: "https://api.slack.com/apps",
            keys: vec![KeyConfig {
                name: "SLACK_BOT_TOKEN",
                label: "Bot User OAuth Token",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "moltbook",
            label: "Moltbook",
            description: "Social network for AI agents. Register via API or get token from moltbook.com",
            url: "https://www.moltbook.com",
            keys: vec![KeyConfig {
                name: "MOLTBOOK_TOKEN",
                label: "API Token",
                secret: true,
            }],
        },
    ]
}

/// Get all valid key names
pub fn get_valid_key_names() -> Vec<&'static str> {
    ApiKeyId::all().iter().map(|k| k.as_str()).collect()
}

/// Get key config by key name
pub fn get_key_config(key_name: &str) -> Option<(&'static str, KeyConfig)> {
    for config in get_service_configs() {
        for key in &config.keys {
            if key.name == key_name {
                return Some((config.group, KeyConfig {
                    name: key.name,
                    label: key.label,
                    secret: key.secret,
                }));
            }
        }
    }
    None
}

#[derive(Debug, Deserialize)]
pub struct UpsertApiKeyRequest {
    pub key_name: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteApiKeyRequest {
    pub key_name: String,
}

#[derive(Serialize)]
pub struct ApiKeysListResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<ApiKeyResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ApiKeyOperationResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<ApiKeyResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response for service configs endpoint
#[derive(Serialize)]
pub struct ServiceConfigsResponse {
    pub success: bool,
    pub configs: Vec<ServiceConfig>,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/keys")
            .route("", web::get().to(list_api_keys))
            .route("", web::post().to(upsert_api_key))
            .route("", web::delete().to(delete_api_key))
            .route("/config", web::get().to(get_configs)),
    );
}

async fn get_configs() -> impl Responder {
    HttpResponse::Ok().json(ServiceConfigsResponse {
        success: true,
        configs: get_service_configs(),
    })
}

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
            return Err(HttpResponse::Unauthorized().json(ApiKeysListResponse {
                success: false,
                keys: None,
                error: Some("No authorization token provided".to_string()),
            }));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(ApiKeysListResponse {
            success: false,
            keys: None,
            error: Some("Invalid or expired session".to_string()),
        })),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(ApiKeysListResponse {
                success: false,
                keys: None,
                error: Some("Internal server error".to_string()),
            }))
        }
    }
}

async fn list_api_keys(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.db.list_api_keys() {
        Ok(keys) => {
            let key_responses: Vec<ApiKeyResponse> = keys
                .into_iter()
                .map(|k| k.to_response())
                .collect();
            HttpResponse::Ok().json(ApiKeysListResponse {
                success: true,
                keys: Some(key_responses),
                error: None,
            })
        }
        Err(e) => {
            log::error!("Failed to list API keys: {}", e);
            HttpResponse::InternalServerError().json(ApiKeysListResponse {
                success: false,
                keys: None,
                error: Some("Failed to retrieve API keys".to_string()),
            })
        }
    }
}

async fn upsert_api_key(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<UpsertApiKeyRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Validate key name
    let valid_keys = get_valid_key_names();
    if !valid_keys.contains(&body.key_name.as_str()) {
        return HttpResponse::BadRequest().json(ApiKeyOperationResponse {
            success: false,
            key: None,
            error: Some(format!(
                "Invalid key name. Valid options: {}",
                valid_keys.join(", ")
            )),
        });
    }

    // Validate api_key is not empty
    if body.api_key.trim().is_empty() {
        return HttpResponse::BadRequest().json(ApiKeyOperationResponse {
            success: false,
            key: None,
            error: Some("API key cannot be empty".to_string()),
        });
    }

    // Store the key (key_name is the service_name in the database)
    match state.db.upsert_api_key(&body.key_name, &body.api_key) {
        Ok(key) => HttpResponse::Ok().json(ApiKeyOperationResponse {
            success: true,
            key: Some(key.to_response()),
            error: None,
        }),
        Err(e) => {
            log::error!("Failed to save API key: {}", e);
            HttpResponse::InternalServerError().json(ApiKeyOperationResponse {
                success: false,
                key: None,
                error: Some("Failed to save API key".to_string()),
            })
        }
    }
}

async fn delete_api_key(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<DeleteApiKeyRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.db.delete_api_key(&body.key_name) {
        Ok(deleted) => {
            if deleted {
                HttpResponse::Ok().json(ApiKeyOperationResponse {
                    success: true,
                    key: None,
                    error: None,
                })
            } else {
                HttpResponse::NotFound().json(ApiKeyOperationResponse {
                    success: false,
                    key: None,
                    error: Some("API key not found".to_string()),
                })
            }
        }
        Err(e) => {
            log::error!("Failed to delete API key: {}", e);
            HttpResponse::InternalServerError().json(ApiKeyOperationResponse {
                success: false,
                key: None,
                error: Some("Failed to delete API key".to_string()),
            })
        }
    }
}
