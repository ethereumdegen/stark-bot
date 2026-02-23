use actix_web::{web, HttpRequest, HttpResponse, Responder};
use ethers::signers::{LocalWallet, Signer};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator};

use crate::backup::{ApiKeyEntry, BackupData};
use crate::keystore_client::KEYSTORE_CLIENT;
use crate::models::ApiKeyResponse;
use crate::AppState;

/// Derive wallet address from private key
fn get_wallet_address(private_key: &str) -> Option<String> {
    let wallet: LocalWallet = private_key.parse().ok()?;
    Some(format!("{:?}", wallet.address()))
}

/// Capitalize the first letter of each word (e.g. "bankr" -> "Bankr", "my_skill" -> "My Skill")
fn titleize(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-' || c == ' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Enum of all valid API key identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr)]
pub enum ApiKeyId {
    #[strum(serialize = "GITHUB_TOKEN")]
    GithubToken,
    #[strum(serialize = "TWITTER_CONSUMER_KEY")]
    TwitterConsumerKey,
    #[strum(serialize = "TWITTER_CONSUMER_SECRET")]
    TwitterConsumerSecret,
    #[strum(serialize = "TWITTER_ACCESS_TOKEN")]
    TwitterAccessToken,
    #[strum(serialize = "TWITTER_ACCESS_TOKEN_SECRET")]
    TwitterAccessTokenSecret,
    #[strum(serialize = "SUPABASE_ACCESS_TOKEN")]
    SupabaseAccessToken,
    #[strum(serialize = "ALCHEMY_API_KEY")]
    AlchemyApiKey,
    #[strum(serialize = "XAI_API_KEY")]
    XaiApiKey,
    #[strum(serialize = "ZEROX_API_KEY")]
    ZeroxApiKey,
}

impl ApiKeyId {
    /// The key name as stored in the database
    pub fn as_str(&self) -> &'static str {
        // AsRefStr from strum provides static string references
        match self {
            Self::GithubToken => "GITHUB_TOKEN",
            Self::TwitterConsumerKey => "TWITTER_CONSUMER_KEY",
            Self::TwitterConsumerSecret => "TWITTER_CONSUMER_SECRET",
            Self::TwitterAccessToken => "TWITTER_ACCESS_TOKEN",
            Self::TwitterAccessTokenSecret => "TWITTER_ACCESS_TOKEN_SECRET",
            Self::SupabaseAccessToken => "SUPABASE_ACCESS_TOKEN",
            Self::AlchemyApiKey => "ALCHEMY_API_KEY",
            Self::XaiApiKey => "XAI_API_KEY",
            Self::ZeroxApiKey => "ZEROX_API_KEY",
        }
    }

    /// Environment variable names to set when this key is available
    pub fn env_vars(&self) -> Option<&'static [&'static str]> {
        match self {
            Self::GithubToken => Some(&["GH_TOKEN", "GITHUB_TOKEN"]),
            Self::TwitterConsumerKey => Some(&["TWITTER_CONSUMER_KEY", "TWITTER_API_KEY"]),
            Self::TwitterConsumerSecret => Some(&["TWITTER_CONSUMER_SECRET", "TWITTER_API_SECRET"]),
            Self::TwitterAccessToken => Some(&["TWITTER_ACCESS_TOKEN"]),
            Self::TwitterAccessTokenSecret => Some(&["TWITTER_ACCESS_TOKEN_SECRET"]),
            Self::SupabaseAccessToken => Some(&["SUPABASE_ACCESS_TOKEN"]),
            Self::AlchemyApiKey => Some(&["ALCHEMY_API_KEY"]),
            Self::XaiApiKey => Some(&["XAI_API_KEY"]),
            Self::ZeroxApiKey => Some(&["ZEROX_API_KEY"]),
        }
    }

    /// Legacy/old names for keys that were renamed. Used for backward-compatible DB lookups.
    pub fn legacy_name(&self) -> Option<&'static str> {
        match self {
            _ => None,
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
    pub name: String,
    pub label: String,
    pub secret: bool,
}

/// Configuration for a service group (e.g., "github" groups GITHUB_TOKEN)
#[derive(Debug, Clone, Serialize)]
pub struct ServiceConfig {
    pub group: String,
    pub label: String,
    pub description: String,
    pub url: String,
    pub keys: Vec<KeyConfig>,
}

/// Get all hardcoded service configurations
pub fn get_service_configs() -> Vec<ServiceConfig> {
    vec![
        ServiceConfig {
            group: "alchemy".into(),
            label: "Alchemy".into(),
            description: "Blockchain RPC provider for wallet monitoring. Create a free app to get an API key.".into(),
            url: "https://dashboard.alchemy.com/apps".into(),
            keys: vec![KeyConfig {
                name: "ALCHEMY_API_KEY".into(),
                label: "API Key".into(),
                secret: true,
            }],
        },
        ServiceConfig {
            group: "github".into(),
            label: "GitHub".into(),
            description: "Create a Personal Access Token with repo scope".into(),
            url: "https://github.com/settings/tokens".into(),
            keys: vec![KeyConfig {
                name: "GITHUB_TOKEN".into(),
                label: "Personal Access Token".into(),
                secret: true,
            }],
        },
        ServiceConfig {
            group: "supabase".into(),
            label: "Supabase".into(),
            description: "Manage Supabase projects. Create a Personal Access Token from your dashboard.".into(),
            url: "https://supabase.com/dashboard/account/tokens".into(),
            keys: vec![KeyConfig {
                name: "SUPABASE_ACCESS_TOKEN".into(),
                label: "Personal Access Token".into(),
                secret: true,
            }],
        },
        ServiceConfig {
            group: "twitter".into(),
            label: "Twitter/X".into(),
            description: "OAuth 1.0a credentials for posting tweets. Get all 4 keys from your Twitter Developer App's 'Keys and Tokens' tab.".into(),
            url: "https://developer.twitter.com/en/portal/projects-and-apps".into(),
            keys: vec![
                KeyConfig {
                    name: "TWITTER_CONSUMER_KEY".into(),
                    label: "API Key (Consumer Key)".into(),
                    secret: true,
                },
                KeyConfig {
                    name: "TWITTER_CONSUMER_SECRET".into(),
                    label: "API Secret (Consumer Secret)".into(),
                    secret: true,
                },
                KeyConfig {
                    name: "TWITTER_ACCESS_TOKEN".into(),
                    label: "Access Token".into(),
                    secret: true,
                },
                KeyConfig {
                    name: "TWITTER_ACCESS_TOKEN_SECRET".into(),
                    label: "Access Token Secret".into(),
                    secret: true,
                },
            ],
        },
        ServiceConfig {
            group: "zerox".into(),
            label: "0x (Swap API)".into(),
            description: "API key for direct 0x swap quotes. Free tier available. Falls back to paid x402 relay if not set.".into(),
            url: "https://dashboard.0x.org/".into(),
            keys: vec![KeyConfig {
                name: "ZEROX_API_KEY".into(),
                label: "API Key".into(),
                secret: true,
            }],
        },
        ServiceConfig {
            group: "xai".into(),
            label: "xAI (Grok)".into(),
            description: "xAI API key for Grok web and X/Twitter search. Create an API key from the xAI console.".into(),
            url: "https://console.x.ai/".into(),
            keys: vec![KeyConfig {
                name: "XAI_API_KEY".into(),
                label: "API Key".into(),
                secret: true,
            }],
        },
    ]
}

/// Get all valid key names (known service keys)
#[allow(dead_code)]
pub fn get_valid_key_names() -> Vec<&'static str> {
    ApiKeyId::all().iter().map(|k| k.as_str()).collect()
}

/// Get key config by key name
pub fn get_key_config(key_name: &str) -> Option<(String, KeyConfig)> {
    for config in get_service_configs() {
        for key in &config.keys {
            if key.name == key_name {
                return Some((config.group.clone(), key.clone()));
            }
        }
    }
    None
}

#[derive(Debug, Deserialize)]
pub struct GetApiKeyValueQuery {
    pub key_name: String,
}

#[derive(Serialize)]
pub struct GetApiKeyValueResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

/// Key data for backup/restore (internal use only)
#[derive(Serialize, Deserialize)]
struct BackupKey {
    key_name: String,
    key_value: String,
}

/// Response for backup/restore operations
#[derive(Serialize)]
pub struct BackupResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_job_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_setting_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_registration_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_settings_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_heartbeat: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_soul: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_identity: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_role_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_role_assignment_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_size_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Key preview for cloud keys preview
#[derive(Serialize)]
pub struct CloudKeyPreview {
    pub key_name: String,
    pub key_preview: String,
}

/// Response for preview cloud backup
#[derive(Serialize)]
pub struct PreviewKeysResponse {
    pub success: bool,
    pub key_count: usize,
    pub keys: Vec<CloudKeyPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_job_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_setting_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_registration_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_settings_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_heartbeat: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_soul: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_identity: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_role_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_role_assignment_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_size_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Request/response for keystore API
#[derive(Serialize, Deserialize)]
struct KeystoreBackupRequest {
    wallet_id: String,
    encrypted_data: String,
    key_count: usize,
    timestamp: i64,
    signature: String,
}

#[derive(Deserialize)]
struct KeystoreBackupResponse {
    encrypted_data: String,
}

/// Sign a message with the burner wallet private key
fn sign_message(private_key: &str, message: &str) -> Result<String, String> {
    use ethers::signers::{LocalWallet, Signer};

    let wallet: LocalWallet = private_key
        .parse()
        .map_err(|e| format!("Invalid private key: {}", e))?;

    // Sign synchronously using the blocking runtime
    let handle = tokio::runtime::Handle::current();
    let message = message.to_string();
    let signature = std::thread::spawn(move || {
        handle.block_on(async {
            wallet.sign_message(message).await
        })
    })
    .join()
    .expect("sign_message thread panicked")
    .map_err(|e| format!("Failed to sign message: {}", e))?;

    Ok(format!("0x{}", hex::encode(signature.to_vec())))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/keys")
            .route("", web::get().to(list_api_keys))
            .route("", web::post().to(upsert_api_key))
            .route("", web::delete().to(delete_api_key))
            .route("/config", web::get().to(get_configs))
            .route("/value", web::get().to(get_api_key_value))
            .route("/cloud_backup", web::post().to(backup_to_cloud))
            .route("/cloud_restore", web::post().to(restore_from_cloud))
            .route("/cloud_preview", web::get().to(preview_cloud_keys)),
    );
}

async fn get_configs(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let mut configs = get_service_configs();

    // Collect all hardcoded key names for deduplication
    let hardcoded_keys: std::collections::HashSet<String> = configs
        .iter()
        .flat_map(|c| c.keys.iter().map(|k| k.name.clone()))
        .collect();

    // Append dynamic keys from enabled skills
    if let Ok(skills) = state.db.list_enabled_skills() {
        for skill in skills {
            if skill.requires_api_keys.is_empty() {
                continue;
            }

            // Build keys list, skipping any that duplicate hardcoded keys
            let keys: Vec<KeyConfig> = skill
                .requires_api_keys
                .iter()
                .filter(|(name, _)| !hardcoded_keys.contains(*name))
                .map(|(name, api_key)| KeyConfig {
                    name: name.clone(),
                    label: if api_key.description.is_empty() {
                        name.clone()
                    } else {
                        api_key.description.clone()
                    },
                    secret: api_key.secret,
                })
                .collect();

            if !keys.is_empty() {
                configs.push(ServiceConfig {
                    group: format!("skill_{}", skill.name),
                    label: format!("{} [skill]", titleize(&skill.name)),
                    description: skill.description.clone(),
                    url: skill.homepage.unwrap_or_default(),
                    keys,
                });
            }
        }
    }

    HttpResponse::Ok().json(ServiceConfigsResponse {
        success: true,
        configs,
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

async fn get_api_key_value(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<GetApiKeyValueQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.db.get_api_key(&query.key_name) {
        Ok(Some(key)) => HttpResponse::Ok().json(GetApiKeyValueResponse {
            success: true,
            key_name: Some(key.service_name),
            key_value: Some(key.api_key),
            error: None,
        }),
        Ok(None) => HttpResponse::NotFound().json(GetApiKeyValueResponse {
            success: false,
            key_name: None,
            key_value: None,
            error: Some("API key not found".to_string()),
        }),
        Err(e) => {
            log::error!("Failed to get API key value: {}", e);
            HttpResponse::InternalServerError().json(GetApiKeyValueResponse {
                success: false,
                key_name: None,
                key_value: None,
                error: Some("Failed to retrieve API key".to_string()),
            })
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

    // Validate key name: non-empty, uppercase alphanumeric + underscores, max 64 chars
    let key_name = body.key_name.trim();
    if key_name.is_empty() {
        return HttpResponse::BadRequest().json(ApiKeyOperationResponse {
            success: false,
            key: None,
            error: Some("Key name cannot be empty".to_string()),
        });
    }
    if key_name.len() > 64 {
        return HttpResponse::BadRequest().json(ApiKeyOperationResponse {
            success: false,
            key: None,
            error: Some("Key name must be 64 characters or fewer".to_string()),
        });
    }
    if !key_name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return HttpResponse::BadRequest().json(ApiKeyOperationResponse {
            success: false,
            key: None,
            error: Some("Key name must contain only uppercase letters, digits, and underscores".to_string()),
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

/// Backup all user data to cloud (encrypted with burner wallet key)
async fn backup_to_cloud(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Wallet provider is the source of truth (Standard=EnvWalletProvider, Flash=FlashWalletProvider)
    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => {
            return HttpResponse::BadRequest().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some("No wallet configured".to_string()),
            });
        }
    };
    let wallet_address = wallet_provider.get_address();

    // Get ECIES encryption key from wallet provider
    let private_key = match wallet_provider.get_encryption_key().await {
        Ok(k) => k,
        Err(e) => {
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some(format!("Failed to get encryption key: {}", e)),
            });
        }
    };

    // Build BackupData with all user data
    let backup = crate::backup::collect_backup_data(
        &state.db,
        wallet_address,
    ).await;

    // Check if there's anything to backup
    if backup.is_empty() {
        return HttpResponse::BadRequest().json(BackupResponse {
            success: false,
            key_count: None,
            node_count: None,
            connection_count: None,
            cron_job_count: None,
            channel_count: None,
            channel_setting_count: None,
            discord_registration_count: None,
            skill_count: None,
            agent_settings_count: None,
            has_settings: None,
            has_heartbeat: None,
            has_soul: None,
            has_identity: None,
            special_role_count: None,
            special_role_assignment_count: None,
            memory_count: None,
            note_count: None,
            module_count: None,
                backup_size_bytes: None,
            message: None,
            error: Some("No data to backup".to_string()),
        });
    }

    let key_count = backup.api_keys.len();
    // Count only non-trunk nodes to be consistent with restore
    let node_count = backup.impulse_map_nodes.iter().filter(|n| !n.is_trunk).count();
    let connection_count = backup.impulse_map_connections.len();
    let cron_job_count = backup.cron_jobs.len();
    let channel_count = backup.channels.len();
    let channel_setting_count = backup.channel_settings.len();
    let discord_registration_count = backup.discord_registrations.len();
    let skill_count = backup.skills.len();
    let agent_settings_count = backup.agent_settings.len();
    let memory_count = backup.memories.as_ref().map(|m| m.len()).unwrap_or(0);
    let note_count = backup.notes.len();
    let module_count = backup.modules.len();
    let has_settings = backup.bot_settings.is_some();
    let has_heartbeat = backup.heartbeat_config.is_some();
    let item_count = backup.item_count();

    // Serialize BackupData to JSON
    let backup_json = match serde_json::to_string(&backup) {
        Ok(j) => j,
        Err(e) => {
            log::error!("Failed to serialize backup: {}", e);
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some("Failed to serialize backup".to_string()),
            });
        }
    };

    // Encrypt with ECIES using the burner wallet's public key
    let encrypted_data = match crate::backup::encrypt_with_private_key(&private_key, &backup_json) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to encrypt backup: {}", e);
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some("Failed to encrypt backup".to_string()),
            });
        }
    };

    // Upload to keystore API — use wallet provider for SIWE auth (works in both modes)
    let store_result = KEYSTORE_CLIENT
        .store_keys_with_provider(&wallet_provider, &encrypted_data, item_count)
        .await;
    match store_result {
        Ok(resp) if resp.success => {
            // Record backup in local state
            if let Err(e) = state.db.record_keystore_backup(&backup.wallet_address, backup.version, item_count) {
                log::warn!("Failed to record backup: {}", e);
            }

            let has_soul = backup.soul_document.is_some();
            let has_identity = backup.identity_document.is_some();
            HttpResponse::Ok().json(BackupResponse {
                success: true,
                key_count: Some(key_count),
                node_count: Some(node_count),
                connection_count: Some(connection_count),
                cron_job_count: Some(cron_job_count),
                channel_count: Some(channel_count),
                channel_setting_count: Some(channel_setting_count),
                discord_registration_count: Some(discord_registration_count),
                skill_count: Some(skill_count),
                agent_settings_count: Some(agent_settings_count),
                has_settings: Some(has_settings),
                has_heartbeat: Some(has_heartbeat),
                has_soul: Some(has_soul),
                has_identity: Some(has_identity),
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: Some(memory_count),
                note_count: Some(note_count),
                module_count: Some(module_count),
                backup_size_bytes: Some(encrypted_data.len()),
                message: Some(format!(
                    "Backed up {} items ({} keys, {} nodes, {} connections, {} cron jobs, {} channels, {} channel settings, {} discord registrations, {} skills, {} AI models, {} memories, {} notes, {} modules{}{}{}{})",
                    item_count,
                    key_count,
                    node_count,
                    connection_count,
                    cron_job_count,
                    channel_count,
                    channel_setting_count,
                    discord_registration_count,
                    skill_count,
                    agent_settings_count,
                    memory_count,
                    note_count,
                    module_count,
                    if has_settings { ", settings" } else { "" },
                    if has_heartbeat { ", heartbeat" } else { "" },
                    if has_soul { ", soul" } else { "" },
                    if has_identity { ", identity" } else { "" }
                )),
                error: None,
            })
        }
        Ok(resp) => {
            log::error!("Keystore store_keys failed: {:?}", resp.error);
            HttpResponse::BadGateway().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: resp.error.or(Some("Failed to upload to keystore".to_string())),
            })
        }
        Err(e) => {
            log::error!("Failed to connect to keystore: {}", e);
            HttpResponse::BadGateway().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some(format!("Keystore error: {}", e)),
            })
        }
    }
}

/// Restore all user data from cloud backup
async fn restore_from_cloud(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Wallet provider is the source of truth (Standard=EnvWalletProvider, Flash=FlashWalletProvider)
    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => {
            return HttpResponse::BadRequest().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some("No wallet configured".to_string()),
            });
        }
    };

    // Get ECIES decryption key from wallet provider
    let private_key = match wallet_provider.get_encryption_key().await {
        Ok(k) => k,
        Err(e) => {
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some(format!("Failed to get encryption key: {}", e)),
            });
        }
    };

    // Fetch from keystore API — use wallet provider for SIWE auth (works in both modes)
    let keystore_result = KEYSTORE_CLIENT
        .get_keys_with_provider(&wallet_provider)
        .await;
    let keystore_resp = match keystore_result {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("Failed to connect to keystore: {}", e);
            return HttpResponse::BadGateway().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some(format!("Keystore error: {}", e)),
            });
        }
    };

    if !keystore_resp.success {
        let error = keystore_resp.error.unwrap_or_else(|| "Unknown error".to_string());
        if error.contains("No backup found") {
            return HttpResponse::NotFound().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some(error),
            });
        }
        return HttpResponse::BadGateway().json(BackupResponse {
            success: false,
            key_count: None,
            node_count: None,
            connection_count: None,
            cron_job_count: None,
            channel_count: None,
            channel_setting_count: None,
            discord_registration_count: None,
            skill_count: None,
            agent_settings_count: None,
            has_settings: None,
            has_heartbeat: None,
            has_soul: None,
            has_identity: None,
            special_role_count: None,
            special_role_assignment_count: None,
            memory_count: None,
            note_count: None,
            module_count: None,
                backup_size_bytes: None,
            message: None,
            error: Some(error),
        });
    }

    let encrypted_data = match keystore_resp.encrypted_data {
        Some(data) => data,
        None => {
            return HttpResponse::BadGateway().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some("No encrypted data in response".to_string()),
            });
        }
    };

    // Decrypt with ECIES using the burner wallet's private key
    let decrypted_json = match crate::backup::decrypt_with_private_key(&private_key, &encrypted_data) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to decrypt backup: {}", e);
            return HttpResponse::BadRequest().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some("Failed to decrypt backup (wrong wallet?)".to_string()),
            });
        }
    };

    // Try to parse as new BackupData format first, fall back to legacy Vec<BackupKey>
    let mut backup_data: BackupData = match serde_json::from_str(&decrypted_json) {
        Ok(data) => data,
        Err(_) => {
            // Try legacy format (just API keys)
            let legacy_keys: Vec<BackupKey> = match serde_json::from_str(&decrypted_json) {
                Ok(keys) => keys,
                Err(e) => {
                    log::error!("Failed to parse backup: {}", e);
                    return HttpResponse::BadRequest().json(BackupResponse {
                        success: false,
                        key_count: None,
                        node_count: None,
                        connection_count: None,
                        cron_job_count: None,
                        channel_count: None,
                        channel_setting_count: None,
                        discord_registration_count: None,
                        skill_count: None,
                        agent_settings_count: None,
                        has_settings: None,
                        has_heartbeat: None,
                        has_soul: None,
                        has_identity: None,
                        special_role_count: None,
                        special_role_assignment_count: None,
                        memory_count: None,
                        note_count: None,
                        module_count: None,
                backup_size_bytes: None,
                        message: None,
                        error: Some("Invalid backup data format".to_string()),
                    });
                }
            };
            // Convert legacy format to BackupData
            let wallet_address = get_wallet_address(&private_key).unwrap_or_default();
            let mut backup = BackupData::new(wallet_address);
            backup.api_keys = legacy_keys
                .into_iter()
                .map(|k| ApiKeyEntry {
                    key_name: k.key_name,
                    key_value: k.key_value,
                })
                .collect();
            backup
        }
    };

    // Unified restore
    let notes_store = state.dispatcher.notes_store();
    let restore_result = crate::backup::restore::restore_all(
        &state.db,
        &mut backup_data,
        Some(&state.skill_registry),
        Some(&state.channel_manager),
        notes_store.as_ref(),
    ).await;

    let restore_result = match restore_result {
        Ok(r) => r,
        Err(e) => {
            log::error!("Restore failed: {}", e);
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                message: None,
                error: Some(format!("Restore failed: {}", e)),
            });
        }
    };

    // Record retrieval in local state
    if let Some(wallet_address) = get_wallet_address(&private_key) {
        let _ = state.db.record_keystore_retrieval(&wallet_address);
    }

    HttpResponse::Ok().json(BackupResponse {
        success: true,
        key_count: Some(restore_result.api_keys),
        node_count: Some(restore_result.impulse_nodes),
        connection_count: Some(restore_result.impulse_connections),
        cron_job_count: Some(restore_result.cron_jobs),
        channel_count: Some(restore_result.channels),
        channel_setting_count: Some(restore_result.channel_settings),
        discord_registration_count: Some(0),
        skill_count: Some(restore_result.skills),
        agent_settings_count: Some(restore_result.agent_settings),
        has_settings: Some(restore_result.bot_settings),
        has_heartbeat: Some(restore_result.heartbeat_config),
        has_soul: Some(restore_result.soul_document),
        has_identity: Some(restore_result.agent_identity),
        special_role_count: Some(restore_result.special_roles),
        special_role_assignment_count: Some(restore_result.special_role_assignments),
        memory_count: Some(restore_result.memories),
        note_count: Some(restore_result.notes),
        module_count: Some(restore_result.modules),
        backup_size_bytes: Some(encrypted_data.len()),
        message: Some(restore_result.summary()),
        error: None,
    })
}

/// Create a preview string from an API key value (e.g., "sk-abc...xyz")
fn create_key_preview(value: &str) -> String {
    if value.len() <= 8 {
        "*".repeat(value.len())
    } else {
        format!("{}...{}", &value[..4], &value[value.len()-4..])
    }
}

/// Preview cloud backup contents (without restoring)
async fn preview_cloud_keys(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Wallet provider is the source of truth (Standard=EnvWalletProvider, Flash=FlashWalletProvider)
    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => {
            return HttpResponse::BadRequest().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some("No wallet configured".to_string()),
            });
        }
    };

    // Get ECIES decryption key from wallet provider
    let private_key = match wallet_provider.get_encryption_key().await {
        Ok(k) => k,
        Err(e) => {
            return HttpResponse::InternalServerError().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some(format!("Failed to get encryption key: {}", e)),
            });
        }
    };

    // Fetch from keystore API — use wallet provider for SIWE auth (works in both modes)
    let keystore_result = KEYSTORE_CLIENT
        .get_keys_with_provider(&wallet_provider)
        .await;
    let keystore_resp = match keystore_result {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("Failed to connect to keystore: {}", e);
            return HttpResponse::BadGateway().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some(format!("Keystore error: {}", e)),
            });
        }
    };

    if !keystore_resp.success {
        let error = keystore_resp.error.unwrap_or_else(|| "Unknown error".to_string());
        if error.contains("No backup found") {
            return HttpResponse::NotFound().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some(error),
            });
        }
        return HttpResponse::BadGateway().json(PreviewKeysResponse {
            success: false,
            key_count: 0,
            keys: vec![],
            node_count: None,
            connection_count: None,
            cron_job_count: None,
            channel_count: None,
            channel_setting_count: None,
            discord_registration_count: None,
            skill_count: None,
            agent_settings_count: None,
            has_settings: None,
            has_heartbeat: None,
            has_soul: None,
            has_identity: None,
            special_role_count: None,
            special_role_assignment_count: None,
            memory_count: None,
            note_count: None,
            module_count: None,
                backup_size_bytes: None,
            backup_version: None,
            message: None,
            error: Some(error),
        });
    }

    let encrypted_data = match keystore_resp.encrypted_data {
        Some(data) => data,
        None => {
            return HttpResponse::BadGateway().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some("No encrypted data in response".to_string()),
            });
        }
    };

    // Decrypt with ECIES using the burner wallet's private key
    let decrypted_json = match crate::backup::decrypt_with_private_key(&private_key, &encrypted_data) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to decrypt backup: {}", e);
            return HttpResponse::BadRequest().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some("Failed to decrypt backup (wrong wallet?)".to_string()),
            });
        }
    };

    // Try to parse as new BackupData format first, fall back to legacy Vec<BackupKey>

    // Try new format first
    if let Ok(backup_data) = serde_json::from_str::<BackupData>(&decrypted_json) {
        let previews: Vec<CloudKeyPreview> = backup_data
            .api_keys
            .iter()
            .map(|k| CloudKeyPreview {
                key_name: k.key_name.clone(),
                key_preview: create_key_preview(&k.key_value),
            })
            .collect();

        // Count only non-trunk nodes to match restore behavior
        let non_trunk_node_count = backup_data.impulse_map_nodes.iter().filter(|n| !n.is_trunk).count();

        return HttpResponse::Ok().json(PreviewKeysResponse {
            success: true,
            key_count: previews.len(),
            keys: previews,
            node_count: Some(non_trunk_node_count),
            connection_count: Some(backup_data.impulse_map_connections.len()),
            cron_job_count: Some(backup_data.cron_jobs.len()),
            channel_count: Some(backup_data.channels.len()),
            channel_setting_count: Some(backup_data.channel_settings.len()),
            discord_registration_count: Some(
                // Check module_data first (new format), fall back to legacy field
                backup_data.module_data.get("discord_tipping")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or_else(|| backup_data.discord_registrations.len())
            ),
            skill_count: Some(backup_data.skills.len()),
            agent_settings_count: Some(backup_data.agent_settings.len()),
            has_settings: Some(backup_data.bot_settings.is_some()),
            has_heartbeat: Some(backup_data.heartbeat_config.is_some()),
            has_soul: Some(backup_data.soul_document.is_some()),
            has_identity: Some(backup_data.identity_document.is_some()),
            special_role_count: Some(backup_data.special_roles.len()),
            special_role_assignment_count: Some(backup_data.special_role_assignments.len()),
            memory_count: Some(backup_data.memories.as_ref().map(|m| m.len()).unwrap_or(0)),
            note_count: Some(backup_data.notes.len()),
            module_count: Some(backup_data.modules.len()),
            backup_size_bytes: Some(encrypted_data.len()),
            backup_version: Some(backup_data.version),
            message: Some("Cloud backup retrieved successfully".to_string()),
            error: None,
        });
    }

    // Fall back to legacy format (just API keys)
    let cloud_keys: Vec<BackupKey> = match serde_json::from_str(&decrypted_json) {
        Ok(keys) => keys,
        Err(e) => {
            log::error!("Failed to parse decrypted keys: {}", e);
            return HttpResponse::BadRequest().json(PreviewKeysResponse {
                success: false,
                key_count: 0,
                keys: vec![],
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                discord_registration_count: None,
                skill_count: None,
                agent_settings_count: None,
                has_settings: None,
                has_heartbeat: None,
                has_soul: None,
                has_identity: None,
                special_role_count: None,
                special_role_assignment_count: None,
                memory_count: None,
                note_count: None,
                module_count: None,
                backup_size_bytes: None,
                backup_version: None,
                message: None,
                error: Some("Invalid backup data format".to_string()),
            });
        }
    };

    // Convert to previews (with masked values)
    let previews: Vec<CloudKeyPreview> = cloud_keys
        .iter()
        .map(|k| CloudKeyPreview {
            key_name: k.key_name.clone(),
            key_preview: create_key_preview(&k.key_value),
        })
        .collect();

    HttpResponse::Ok().json(PreviewKeysResponse {
        success: true,
        key_count: previews.len(),
        keys: previews,
        node_count: None,
        connection_count: None,
        cron_job_count: None,
        channel_count: None,
        channel_setting_count: None,
        discord_registration_count: None,
        skill_count: None,
        agent_settings_count: None,
        has_settings: None,
        has_heartbeat: None,
        has_soul: None,
        has_identity: None,
        special_role_count: None,
        special_role_assignment_count: None,
        memory_count: None,
        note_count: None,
        module_count: None,
        backup_size_bytes: Some(encrypted_data.len()),
        backup_version: None,
        message: Some("Cloud keys retrieved successfully (legacy format)".to_string()),
        error: None,
    })
}
