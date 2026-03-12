use actix_web::{web, HttpRequest, HttpResponse, Responder};
use ethers::signers::{LocalWallet, Signer};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator};

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

/// Response for backup/restore operations
#[derive(Serialize)]
pub struct BackupResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x402_limit_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bot_config: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_size_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BackupResponse {
    fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            key_count: None,
            has_settings: None,
            x402_limit_count: None,
            has_bot_config: None,
            item_count: None,
            backup_size_bytes: None,
            message: None,
            error: Some(msg.into()),
        }
    }
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
    pub has_settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x402_limit_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_bot_config: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_size_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PreviewKeysResponse {
    fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            key_count: 0,
            keys: vec![],
            has_settings: None,
            x402_limit_count: None,
            has_bot_config: None,
            backup_size_bytes: None,
            backup_version: None,
            message: None,
            error: Some(msg.into()),
        }
    }
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
async fn sign_message(private_key: &str, message: &str) -> Result<String, String> {
    use ethers::signers::{LocalWallet, Signer};

    let wallet: LocalWallet = private_key
        .parse()
        .map_err(|e| format!("Invalid private key: {}", e))?;

    let signature = wallet
        .sign_message(message)
        .await
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

    let configs = get_service_configs();

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

    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => return HttpResponse::BadRequest().json(BackupResponse::error("No wallet configured")),
    };
    let wallet_address = wallet_provider.get_address();

    // Get ECIES encryption key from wallet provider
    let private_key = match wallet_provider.get_encryption_key().await {
        Ok(k) => k,
        Err(e) => return HttpResponse::InternalServerError().json(BackupResponse::error(format!("Failed to get encryption key: {}", e))),
    };

    // Build BackupData
    let backup = crate::backup::collect_backup_data(&state.db, &wallet_address).await;

    if backup.is_empty() {
        return HttpResponse::BadRequest().json(BackupResponse::error("No data to backup"));
    }

    let key_count = backup.api_keys.len();
    let has_settings = backup.bot_settings.is_some();
    let x402_limit_count = backup.x402_payment_limits.len();
    let has_bot_config = backup.bot_config.is_some();
    let total_items = backup.item_count();

    // Encrypt with ECIES
    let encrypted_data = match crate::keystore_client::encrypt_backup_data(&private_key, &backup) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to encrypt backup: {}", e);
            return HttpResponse::InternalServerError().json(BackupResponse::error("Failed to encrypt backup"));
        }
    };

    // Upload to keystore API
    let store_result = KEYSTORE_CLIENT
        .store_keys_with_provider(&wallet_provider, &encrypted_data, total_items)
        .await;
    match store_result {
        Ok(resp) if resp.success => {
            HttpResponse::Ok().json(BackupResponse {
                success: true,
                key_count: Some(key_count),
                has_settings: Some(has_settings),
                x402_limit_count: Some(x402_limit_count),
                has_bot_config: Some(has_bot_config),
                item_count: Some(total_items),
                backup_size_bytes: Some(encrypted_data.len()),
                message: Some(format!("Backed up {} items ({} keys, {} x402 limits)", total_items, key_count, x402_limit_count)),
                error: None,
            })
        }
        Ok(resp) => {
            log::error!("Keystore store_keys failed: {:?}", resp.error);
            HttpResponse::BadGateway().json(BackupResponse::error(
                resp.error.unwrap_or_else(|| "Failed to upload to keystore".to_string())
            ))
        }
        Err(e) => {
            log::error!("Failed to connect to keystore: {}", e);
            HttpResponse::BadGateway().json(BackupResponse::error(format!("Keystore error: {}", e)))
        }
    }
}

/// Restore all user data from cloud backup
async fn restore_from_cloud(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => return HttpResponse::BadRequest().json(BackupResponse::error("No wallet configured")),
    };

    let private_key = match wallet_provider.get_encryption_key().await {
        Ok(k) => k,
        Err(e) => return HttpResponse::InternalServerError().json(BackupResponse::error(format!("Failed to get encryption key: {}", e))),
    };

    // Fetch from keystore API
    let keystore_resp = match KEYSTORE_CLIENT.get_keys_with_provider(&wallet_provider).await {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("Failed to connect to keystore: {}", e);
            return HttpResponse::BadGateway().json(BackupResponse::error(format!("Keystore error: {}", e)));
        }
    };

    if !keystore_resp.success {
        let error = keystore_resp.error.unwrap_or_else(|| "Unknown error".to_string());
        if error.contains("No backup found") {
            return HttpResponse::NotFound().json(BackupResponse::error(error));
        }
        return HttpResponse::BadGateway().json(BackupResponse::error(error));
    }

    let encrypted_data = match keystore_resp.encrypted_data {
        Some(data) => data,
        None => return HttpResponse::BadGateway().json(BackupResponse::error("No encrypted data in response")),
    };

    // Decrypt with ECIES
    let mut backup_data = match crate::keystore_client::decrypt_backup_data(&private_key, &encrypted_data) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to decrypt backup: {}", e);
            return HttpResponse::BadRequest().json(BackupResponse::error("Failed to decrypt backup (wrong wallet?)"));
        }
    };

    // Restore
    let restore_result = match crate::backup::restore::restore_all(&state.db, &mut backup_data).await {
        Ok(r) => r,
        Err(e) => {
            log::error!("Restore failed: {}", e);
            return HttpResponse::InternalServerError().json(BackupResponse::error(format!("Restore failed: {}", e)));
        }
    };

    HttpResponse::Ok().json(BackupResponse {
        success: true,
        key_count: Some(restore_result.api_keys),
        has_settings: Some(restore_result.bot_settings),
        x402_limit_count: Some(restore_result.x402_payment_limits),
        has_bot_config: Some(restore_result.bot_config),
        item_count: None,
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

    let wallet_provider = match &state.wallet_provider {
        Some(wp) => wp.clone(),
        None => return HttpResponse::BadRequest().json(PreviewKeysResponse::error("No wallet configured")),
    };

    let private_key = match wallet_provider.get_encryption_key().await {
        Ok(k) => k,
        Err(e) => return HttpResponse::InternalServerError().json(PreviewKeysResponse::error(format!("Failed to get encryption key: {}", e))),
    };

    let keystore_resp = match KEYSTORE_CLIENT.get_keys_with_provider(&wallet_provider).await {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("Failed to connect to keystore: {}", e);
            return HttpResponse::BadGateway().json(PreviewKeysResponse::error(format!("Keystore error: {}", e)));
        }
    };

    if !keystore_resp.success {
        let error = keystore_resp.error.unwrap_or_else(|| "Unknown error".to_string());
        if error.contains("No backup found") {
            return HttpResponse::NotFound().json(PreviewKeysResponse::error(error));
        }
        return HttpResponse::BadGateway().json(PreviewKeysResponse::error(error));
    }

    let encrypted_data = match keystore_resp.encrypted_data {
        Some(data) => data,
        None => return HttpResponse::BadGateway().json(PreviewKeysResponse::error("No encrypted data in response")),
    };

    // Decrypt with ECIES
    let backup_data = match crate::keystore_client::decrypt_backup_data(&private_key, &encrypted_data) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to decrypt backup: {}", e);
            return HttpResponse::BadRequest().json(PreviewKeysResponse::error("Failed to decrypt backup (wrong wallet?)"));
        }
    };

    let previews: Vec<CloudKeyPreview> = backup_data
        .api_keys
        .iter()
        .map(|k| CloudKeyPreview {
            key_name: k.service_name.clone(),
            key_preview: create_key_preview(&k.api_key),
        })
        .collect();

    HttpResponse::Ok().json(PreviewKeysResponse {
        success: true,
        key_count: previews.len(),
        keys: previews,
        has_settings: Some(backup_data.bot_settings.is_some()),
        x402_limit_count: Some(backup_data.x402_payment_limits.len()),
        has_bot_config: Some(backup_data.bot_config.is_some()),
        backup_size_bytes: Some(encrypted_data.len()),
        backup_version: Some(backup_data.version),
        message: Some("Cloud backup retrieved successfully".to_string()),
        error: None,
    })
}
