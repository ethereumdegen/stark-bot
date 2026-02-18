use actix_web::{web, HttpRequest, HttpResponse, Responder};
use ethers::signers::{LocalWallet, Signer};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator};

use crate::backup::{ApiKeyEntry, BackupData};
use crate::db::tables::mind_nodes::CreateMindNodeRequest;
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
                message: None,
                error: Some(format!("Failed to get encryption key: {}", e)),
            });
        }
    };

    // Build BackupData with all user data
    let backup = crate::backup::collect_backup_data(&state.db, wallet_address).await;

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
            message: None,
            error: Some("No data to backup".to_string()),
        });
    }

    let key_count = backup.api_keys.len();
    // Count only non-trunk nodes to be consistent with restore
    let node_count = backup.mind_map_nodes.iter().filter(|n| !n.is_trunk).count();
    let connection_count = backup.mind_map_connections.len();
    let cron_job_count = backup.cron_jobs.len();
    let channel_count = backup.channels.len();
    let channel_setting_count = backup.channel_settings.len();
    let discord_registration_count = backup.discord_registrations.len();
    let skill_count = backup.skills.len();
    let agent_settings_count = backup.agent_settings.len();
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
                message: Some(format!(
                    "Backed up {} items ({} keys, {} nodes, {} connections, {} cron jobs, {} channels, {} channel settings, {} discord registrations, {} skills, {} AI models{}{}{}{})",
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

    // Restore API keys
    let mut restored_keys = 0;
    for key in &backup_data.api_keys {
        if let Err(e) = state.db.upsert_api_key(&key.key_name, &key.key_value) {
            log::error!("Failed to restore key {}: {}", key.key_name, e);
        } else {
            restored_keys += 1;
        }
    }

    // Clear existing mind nodes and connections before restore
    match state.db.clear_mind_nodes_for_restore() {
        Ok((nodes_deleted, connections_deleted)) => {
            log::info!("Cleared {} nodes and {} connections for restore", nodes_deleted, connections_deleted);
        }
        Err(e) => {
            log::warn!("Failed to clear mind nodes for restore: {}", e);
        }
    }

    // Clear existing cron jobs before restore
    match state.db.clear_cron_jobs_for_restore() {
        Ok(jobs_deleted) => {
            log::info!("Cleared {} cron jobs for restore", jobs_deleted);
        }
        Err(e) => {
            log::warn!("Failed to clear cron jobs for restore: {}", e);
        }
    }

    // Clear existing channel settings before restore
    match state.db.clear_channel_settings_for_restore() {
        Ok(settings_deleted) => {
            log::info!("Cleared {} channel settings for restore", settings_deleted);
        }
        Err(e) => {
            log::warn!("Failed to clear channel settings for restore: {}", e);
        }
    }

    // Clear existing channels before restore (non-safe-mode only)
    match state.db.clear_channels_for_restore() {
        Ok(channels_deleted) => {
            log::info!("Cleared {} channels for restore", channels_deleted);
        }
        Err(e) => {
            log::warn!("Failed to clear channels for restore: {}", e);
        }
    }

    // Restore mind map nodes with ID mapping
    // Get or create trunk node and map backup trunk ID to current trunk ID
    let mut old_to_new_id: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let current_trunk = state.db.get_or_create_trunk_node().ok();

    // Find trunk in backup and map its ID to current trunk ID
    if let Some(ref trunk) = current_trunk {
        for node in &backup_data.mind_map_nodes {
            if node.is_trunk {
                old_to_new_id.insert(node.id, trunk.id);
                break;
            }
        }
    }

    let mut restored_nodes = 0;
    for node in &backup_data.mind_map_nodes {
        // Skip trunk nodes - they're auto-managed (already mapped above)
        if node.is_trunk {
            continue;
        }

        let request = CreateMindNodeRequest {
            body: Some(node.body.clone()),
            position_x: node.position_x,
            position_y: node.position_y,
            parent_id: None, // Connections are handled separately
        };

        match state.db.create_mind_node(&request) {
            Ok(new_node) => {
                old_to_new_id.insert(node.id, new_node.id);
                restored_nodes += 1;
            }
            Err(e) => {
                log::warn!("Failed to restore mind node: {}", e);
            }
        }
    }

    // Restore mind map connections using ID mapping
    let mut restored_connections = 0;
    for conn in &backup_data.mind_map_connections {
        let new_parent_id = old_to_new_id.get(&conn.parent_id);
        let new_child_id = old_to_new_id.get(&conn.child_id);
        if let (Some(&parent_id), Some(&child_id)) = (new_parent_id, new_child_id) {
            match state.db.create_mind_node_connection(parent_id, child_id) {
                Ok(_) => restored_connections += 1,
                Err(e) => {
                    log::warn!("Failed to restore connection: {}", e);
                }
            }
        } else {
            log::warn!(
                "Could not map connection parent_id={} child_id={} (mapped: parent={:?}, child={:?})",
                conn.parent_id, conn.child_id, new_parent_id, new_child_id
            );
        }
    }

    // Restore bot settings if present
    let has_settings = backup_data.bot_settings.is_some();
    if let Some(settings) = &backup_data.bot_settings {
        // Parse custom_rpc_endpoints from JSON string if present
        let custom_rpc: Option<std::collections::HashMap<String, String>> =
            settings.custom_rpc_endpoints.as_ref().and_then(|s| {
                serde_json::from_str(s).ok()
            });

        if let Err(e) = state.db.update_bot_settings_full(
            Some(&settings.bot_name),
            Some(&settings.bot_email),
            Some(settings.web3_tx_requires_confirmation),
            settings.rpc_provider.as_deref(),
            custom_rpc.as_ref(),
            settings.max_tool_iterations,
            Some(settings.rogue_mode_enabled),
            settings.safe_mode_max_queries_per_10min,
            None, // Don't restore keystore_url - it's infrastructure config
            None,
            Some(settings.guest_dashboard_enabled),
            settings.theme_accent.as_deref(),
            None, // Don't restore proxy_url - it's infrastructure config
            None, // Don't restore kanban_auto_execute - keep current setting
        ) {
            log::warn!("Failed to restore bot settings: {}", e);
        }
    }

    // Restore channels FIRST (we need ID mapping for cron jobs, heartbeat, and channel settings)
    let mut old_channel_to_new_id: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let mut restored_channels = 0;
    for channel in &backup_data.channels {
        match state.db.create_channel(
            &channel.channel_type,
            &channel.name,
            &channel.bot_token,
            channel.app_token.as_deref(),
        ) {
            Ok(new_channel) => {
                old_channel_to_new_id.insert(channel.id, new_channel.id);
                // Restore enabled state
                if channel.enabled {
                    let _ = state.db.set_channel_enabled(new_channel.id, true);
                }
                // Migrate legacy bot_token column → channel setting (backwards compat)
                if !channel.bot_token.is_empty() {
                    let setting_key = match channel.channel_type.as_str() {
                        "discord" => Some("discord_bot_token"),
                        "telegram" => Some("telegram_bot_token"),
                        "slack" => Some("slack_bot_token"),
                        _ => None,
                    };
                    if let Some(key) = setting_key {
                        let _ = state.db.set_channel_setting(new_channel.id, key, &channel.bot_token);
                    }
                }
                // Migrate legacy app_token column → channel setting (backwards compat)
                if let Some(ref app_token) = channel.app_token {
                    if !app_token.is_empty() && channel.channel_type == "slack" {
                        let _ = state.db.set_channel_setting(new_channel.id, "slack_app_token", app_token);
                    }
                }
                restored_channels += 1;
            }
            Err(e) => {
                log::warn!("Failed to restore channel {}: {}", channel.name, e);
            }
        }
    }

    // Restore channel settings (using new channel IDs from mapping)
    let mut restored_channel_settings = 0;
    for setting in &backup_data.channel_settings {
        // Map old channel ID to new channel ID
        let new_channel_id = old_channel_to_new_id
            .get(&setting.channel_id)
            .copied()
            .unwrap_or(setting.channel_id); // Fallback to original ID if not found

        if let Err(e) = state.db.set_channel_setting(
            new_channel_id,
            &setting.setting_key,
            &setting.setting_value,
        ) {
            log::warn!(
                "Failed to restore channel setting {}/{}: {}",
                new_channel_id, setting.setting_key, e
            );
        } else {
            restored_channel_settings += 1;
        }
    }

    // Restore cron jobs (with mapped channel IDs)
    let mut restored_cron_jobs = 0;
    for job in &backup_data.cron_jobs {
        // Map old channel_id to new channel_id
        let mapped_channel_id = job.channel_id.and_then(|old_id| old_channel_to_new_id.get(&old_id).copied());
        match state.db.create_cron_job(
            &job.name,
            job.description.as_deref(),
            &job.schedule_type,
            &job.schedule_value,
            job.timezone.as_deref(),
            &job.session_mode,
            job.message.as_deref(),
            job.system_event.as_deref(),
            mapped_channel_id,
            job.deliver_to.as_deref(),
            job.deliver,
            job.model_override.as_deref(),
            job.thinking_level.as_deref(),
            job.timeout_seconds,
            job.delete_after_run,
        ) {
            Ok(_) => restored_cron_jobs += 1,
            Err(e) => {
                log::warn!("Failed to restore cron job {}: {}", job.name, e);
            }
        }
    }

    // Restore heartbeat config if present (with mapped channel ID)
    let has_heartbeat = backup_data.heartbeat_config.is_some();
    if let Some(hb_config) = &backup_data.heartbeat_config {
        // Map old channel_id to new channel_id
        let mapped_channel_id = hb_config.channel_id.and_then(|old_id| old_channel_to_new_id.get(&old_id).copied());
        match state.db.get_or_create_heartbeat_config(mapped_channel_id) {
            Ok(existing) => {
                // Update with restored values
                if let Err(e) = state.db.update_heartbeat_config(
                    existing.id,
                    Some(hb_config.interval_minutes),
                    Some(&hb_config.target),
                    hb_config.active_hours_start.as_deref(),
                    hb_config.active_hours_end.as_deref(),
                    hb_config.active_days.as_deref(),
                    Some(hb_config.enabled),
                ) {
                    log::warn!("Failed to restore heartbeat config: {}", e);
                }
            }
            Err(e) => {
                log::warn!("Failed to create heartbeat config for restore: {}", e);
            }
        }
    }

    // Restore soul document from keystore backup — ALWAYS overrides the template.
    // The keystore soul is the agent's evolved personality (with agent modifications),
    // while soul_template/SOUL.md is just the initial starting point.
    let mut has_soul = false;
    if let Some(soul_content) = &backup_data.soul_document {
        let soul_path = crate::config::soul_document_path();
        // Ensure soul directory exists
        if let Some(parent) = soul_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&soul_path, soul_content) {
            Ok(_) => {
                has_soul = true;
                log::info!("[Keystore] Restored soul document from backup (overrides template)");
            }
            Err(e) => {
                log::warn!("[Keystore] Failed to restore soul document: {}", e);
            }
        }
    }

    // Restore agent identity from backup (DB is single source of truth)
    let mut has_identity = false;
    if let Some(ref ai) = backup_data.agent_identity {
        let conn = state.db.conn();
        let existing: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_identity", [], |r| r.get(0))
            .unwrap_or(0);
        if existing == 0 {
            // Use full metadata from backup entry
            match state.db.upsert_agent_identity(
                ai.agent_id,
                &ai.agent_registry,
                ai.chain_id,
                ai.name.as_deref(),
                ai.description.as_deref(),
                ai.image.as_deref(),
                ai.x402_support,
                ai.active,
                &ai.services_json,
                &ai.supported_trust_json,
                ai.registration_uri.as_deref(),
            ) {
                Ok(_) => {
                    has_identity = true;
                    log::info!(
                        "[Keystore] Restored agent identity (agent_id={}) from backup",
                        ai.agent_id
                    );
                }
                Err(e) => {
                    log::warn!("[Keystore] Failed to restore agent identity: {}", e);
                }
            }
        } else {
            has_identity = true;
            log::info!("[Keystore] Agent identity already exists locally, skipping restore from backup");
        }
    }

    // Legacy: if old backup has identity_document but no agent_identity entry,
    // and no DB row exists, parse the JSON and create a minimal DB row
    if !has_identity {
        if let Some(identity_content) = &backup_data.identity_document {
            let existing: i64 = state.db.conn()
                .query_row("SELECT COUNT(*) FROM agent_identity", [], |r| r.get(0))
                .unwrap_or(0);
            if existing == 0 {
                if let Ok(reg) = serde_json::from_str::<crate::eip8004::types::RegistrationFile>(identity_content) {
                    let services_json = serde_json::to_string(&reg.services).unwrap_or_else(|_| "[]".to_string());
                    let supported_trust_json = serde_json::to_string(&reg.supported_trust).unwrap_or_else(|_| "[]".to_string());
                    match state.db.upsert_agent_identity(
                        0, "", 0,
                        Some(&reg.name), Some(&reg.description), reg.image.as_deref(),
                        reg.x402_support, reg.active,
                        &services_json, &supported_trust_json,
                        None,
                    ) {
                        Ok(_) => {
                            has_identity = true;
                            log::info!("[Keystore] Migrated legacy identity_document to DB");
                        }
                        Err(e) => {
                            log::warn!("[Keystore] Failed to migrate legacy identity_document: {}", e);
                        }
                    }
                }
            }
        }
    }

    // Restore module data (generic module restore)
    let mut restored_discord_registrations = 0;
    {
        let module_registry = crate::modules::ModuleRegistry::new();

        // Backward-compat shim: if old discord_registrations field is populated
        // AND module_data["discord_tipping"] is absent, convert old format
        if !backup_data.discord_registrations.is_empty() && !backup_data.module_data.contains_key("discord_tipping") {
            log::info!("Converting legacy discord_registrations to module_data format");
            let legacy_entries: Vec<serde_json::Value> = backup_data.discord_registrations.iter().map(|reg| {
                serde_json::json!({
                    "discord_user_id": reg.discord_user_id,
                    "discord_username": reg.discord_username,
                    "public_address": reg.public_address,
                    "registered_at": reg.registered_at,
                })
            }).collect();
            backup_data.module_data.insert("discord_tipping".to_string(), serde_json::Value::Array(legacy_entries));
        }

        // Restore each module's data
        for (module_name, data) in &backup_data.module_data {
            if let Some(module) = module_registry.get(module_name) {
                // Auto-install the module if not already installed
                if !state.db.is_module_installed(module_name).unwrap_or(true) {
                    let _ = state.db.install_module(
                        module_name,
                        module.description(),
                        module.version(),
                        module.has_tools(),
                        module.has_dashboard(),
                    );
                }
                match module.restore_data(&state.db, data).await {
                    Ok(()) => {
                        log::info!("Restored module data for '{}'", module_name);
                        if module_name == "discord_tipping" {
                            // Count restored entries for response
                            restored_discord_registrations = data.as_array().map(|a| a.len()).unwrap_or(0);
                        }
                    }
                    Err(e) => log::warn!("Failed to restore module data for '{}': {}", module_name, e),
                }
            } else {
                log::warn!("Skipping restore for unknown module '{}'", module_name);
            }
        }
    }

    // Restore skills (version-aware: won't downgrade bundled skills that have newer versions on disk)
    let mut restored_skills = 0;
    for skill_entry in &backup_data.skills {
        let now = chrono::Utc::now().to_rfc3339();
        let arguments: std::collections::HashMap<String, crate::skills::types::SkillArgument> =
            serde_json::from_str(&skill_entry.arguments).unwrap_or_default();

        let requires_api_keys: std::collections::HashMap<String, crate::skills::types::SkillApiKey> =
            serde_json::from_str(&skill_entry.requires_api_keys).unwrap_or_default();

        let db_skill = crate::skills::DbSkill {
            id: None,
            name: skill_entry.name.clone(),
            description: skill_entry.description.clone(),
            body: skill_entry.body.clone(),
            version: skill_entry.version.clone(),
            author: skill_entry.author.clone(),
            homepage: skill_entry.homepage.clone(),
            metadata: skill_entry.metadata.clone(),
            enabled: skill_entry.enabled,
            requires_tools: skill_entry.requires_tools.clone(),
            requires_binaries: skill_entry.requires_binaries.clone(),
            arguments,
            tags: skill_entry.tags.clone(),
            subagent_type: skill_entry.subagent_type.clone(),
            requires_api_keys,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match state.db.create_skill(&db_skill) {
            Ok(skill_id) => {
                // Restore scripts for this skill
                for script in &skill_entry.scripts {
                    let db_script = crate::skills::DbSkillScript {
                        id: None,
                        skill_id,
                        name: script.name.clone(),
                        code: script.code.clone(),
                        language: script.language.clone(),
                        created_at: now.clone(),
                    };
                    if let Err(e) = state.db.create_skill_script(&db_script) {
                        log::warn!("Failed to restore script '{}' for skill '{}': {}", script.name, skill_entry.name, e);
                    }
                }
                restored_skills += 1;
            }
            Err(e) => {
                log::warn!("Failed to restore skill '{}': {}", skill_entry.name, e);
            }
        }
    }

    // Re-apply bundled skills from disk to ensure newer versions aren't downgraded by cloud restore
    if restored_skills > 0 {
        match state.skill_registry.load_all().await {
            Ok(count) => log::info!("Re-applied {} file-based skills after cloud restore", count),
            Err(e) => log::warn!("Failed to re-apply file-based skills after restore: {}", e),
        }
    }

    // Restore agent settings (AI model configurations)
    let mut restored_agent_settings = 0;
    if !backup_data.agent_settings.is_empty() {
        // Clear existing agent settings before restore
        if let Err(e) = state.db.disable_agent_settings() {
            log::warn!("Failed to disable existing agent settings for restore: {}", e);
        }
        for entry in &backup_data.agent_settings {
            match state.db.save_agent_settings(
                &entry.endpoint,
                &entry.model_archetype,
                entry.model.as_deref(),
                entry.max_response_tokens,
                entry.max_context_tokens,
                entry.secret_key.as_deref(),
            ) {
                Ok(saved) => {
                    // save_agent_settings enables the last one saved; if the backup entry was
                    // disabled we need to disable all again and rely on the enabled one being
                    // saved last (they are ordered by id in the backup).
                    if !entry.enabled {
                        let _ = state.db.disable_agent_settings();
                    }
                    restored_agent_settings += 1;
                    log::info!("Restored agent settings: {} ({})", saved.endpoint, saved.model_archetype);
                }
                Err(e) => {
                    log::warn!("Failed to restore agent settings for {}: {}", entry.endpoint, e);
                }
            }
        }
    }

    // Restore x402 payment limits
    let mut restored_x402_limits = 0;
    for limit in &backup_data.x402_payment_limits {
        match state.db.set_x402_payment_limit(&limit.asset, &limit.max_amount, limit.decimals, &limit.display_name, limit.address.as_deref()) {
            Ok(_) => {
                crate::x402::payment_limits::set_limit(&limit.asset, &limit.max_amount, limit.decimals, &limit.display_name, limit.address.as_deref());
                restored_x402_limits += 1;
            }
            Err(e) => log::warn!("Failed to restore x402 payment limit for {}: {}", limit.asset, e),
        }
    }
    if restored_x402_limits > 0 {
        log::info!("Restored {} x402 payment limits", restored_x402_limits);
    }

    // Restore kanban board items
    if !backup_data.kanban_items.is_empty() {
        // Clear existing kanban items before restore
        if let Ok(existing) = state.db.list_kanban_items() {
            for item in existing {
                let _ = state.db.delete_kanban_item(item.id);
            }
        }

        let mut restored_kanban = 0;
        for item in &backup_data.kanban_items {
            let request = crate::db::tables::kanban::CreateKanbanItemRequest {
                title: item.title.clone(),
                description: Some(item.description.clone()),
                priority: Some(item.priority),
            };
            match state.db.create_kanban_item(&request) {
                Ok(new_item) => {
                    let update_req = crate::db::tables::kanban::UpdateKanbanItemRequest {
                        status: Some(item.status.clone()),
                        result: item.result.clone(),
                        ..Default::default()
                    };
                    let _ = state.db.update_kanban_item(new_item.id, &update_req);
                    restored_kanban += 1;
                }
                Err(e) => log::warn!("Failed to restore kanban item: {}", e),
            }
        }
        if restored_kanban > 0 {
            log::info!("Restored {} kanban board items", restored_kanban);
        }
    }

    // Restore special roles
    let mut restored_special_roles = 0;
    for entry in &backup_data.special_roles {
        let role = crate::models::SpecialRole {
            name: entry.name.clone(),
            allowed_tools: serde_json::from_str(&entry.allowed_tools_json).unwrap_or_default(),
            allowed_skills: serde_json::from_str(&entry.allowed_skills_json).unwrap_or_default(),
            description: entry.description.clone(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        match state.db.upsert_special_role(&role) {
            Ok(_) => restored_special_roles += 1,
            Err(e) => log::warn!("Failed to restore special role '{}': {}", entry.name, e),
        }
    }
    if restored_special_roles > 0 {
        log::info!("Restored {} special roles", restored_special_roles);
    }

    // Restore special role assignments (roles must exist first due to FK constraint)
    let mut restored_special_role_assignments = 0;
    for entry in &backup_data.special_role_assignments {
        match state.db.create_special_role_assignment(
            &entry.channel_type,
            &entry.user_id,
            &entry.special_role_name,
            entry.label.as_deref(),
        ) {
            Ok(_) => restored_special_role_assignments += 1,
            Err(e) => log::warn!(
                "Failed to restore special role assignment ({}/{} -> {}): {}",
                entry.channel_type, entry.user_id, entry.special_role_name, e
            ),
        }
    }
    if restored_special_role_assignments > 0 {
        log::info!("Restored {} special role assignments", restored_special_role_assignments);
    }

    // Auto-start channels with auto_start_on_boot setting enabled
    let mut auto_started_channels = 0;
    for (old_id, new_id) in &old_channel_to_new_id {
        // Check if this channel has auto_start_on_boot enabled
        let should_auto_start = state.db
            .get_channel_setting(*new_id, "auto_start_on_boot")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);

        if should_auto_start {
            // Get the channel and start it
            if let Ok(Some(channel)) = state.db.get_channel(*new_id) {
                match state.channel_manager.start_channel(channel).await {
                    Ok(_) => {
                        auto_started_channels += 1;
                        log::info!("Auto-started channel {} (old_id={})", new_id, old_id);
                    }
                    Err(e) => {
                        log::warn!("Failed to auto-start channel {}: {}", new_id, e);
                    }
                }
            }
        }
    }
    if auto_started_channels > 0 {
        log::info!("Auto-started {} channels after restore", auto_started_channels);
    }

    // Record retrieval in local state
    if let Some(wallet_address) = get_wallet_address(&private_key) {
        let _ = state.db.record_keystore_retrieval(&wallet_address);
    }

    HttpResponse::Ok().json(BackupResponse {
        success: true,
        key_count: Some(restored_keys),
        node_count: Some(restored_nodes),
        connection_count: Some(restored_connections),
        cron_job_count: Some(restored_cron_jobs),
        channel_count: Some(restored_channels),
        channel_setting_count: Some(restored_channel_settings),
        discord_registration_count: Some(restored_discord_registrations),
        skill_count: Some(restored_skills),
        agent_settings_count: Some(restored_agent_settings),
        has_settings: Some(has_settings),
        has_heartbeat: Some(has_heartbeat),
        has_soul: Some(has_soul),
        has_identity: Some(has_identity),
        special_role_count: Some(restored_special_roles),
        special_role_assignment_count: Some(restored_special_role_assignments),
        message: Some(format!(
            "Restored {} keys, {} nodes, {} connections, {} cron jobs, {} channels, {} channel settings, {} discord registrations, {} skills, {} AI models, {} special roles, {} role assignments{}{}{}{}",
            restored_keys,
            restored_nodes,
            restored_connections,
            restored_cron_jobs,
            restored_channels,
            restored_channel_settings,
            restored_discord_registrations,
            restored_skills,
            restored_agent_settings,
            restored_special_roles,
            restored_special_role_assignments,
            if has_settings { ", settings" } else { "" },
            if has_heartbeat { ", heartbeat" } else { "" },
            if has_soul { ", soul" } else { "" },
            if has_identity { ", identity" } else { "" }
        )),
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
        let non_trunk_node_count = backup_data.mind_map_nodes.iter().filter(|n| !n.is_trunk).count();

        return HttpResponse::Ok().json(PreviewKeysResponse {
            success: true,
            key_count: previews.len(),
            keys: previews,
            node_count: Some(non_trunk_node_count),
            connection_count: Some(backup_data.mind_map_connections.len()),
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
        backup_version: None,
        message: Some("Cloud keys retrieved successfully (legacy format)".to_string()),
        error: None,
    })
}
