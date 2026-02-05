use actix_web::{web, HttpRequest, HttpResponse, Responder};
use ethers::signers::{LocalWallet, Signer};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator};

use crate::backup::{
    ApiKeyEntry, BackupData, BotSettingsEntry, ChannelEntry, ChannelSettingEntry, CronJobEntry,
    HeartbeatConfigEntry, MindConnectionEntry, MindNodeEntry,
};
use crate::db::tables::mind_nodes::{CreateMindNodeRequest, UpdateMindNodeRequest};
use crate::keystore_client::KEYSTORE_CLIENT;
use crate::models::ApiKeyResponse;
use crate::AppState;

/// Derive wallet address from private key
fn get_wallet_address(private_key: &str) -> Option<String> {
    let wallet: LocalWallet = private_key.parse().ok()?;
    Some(format!("{:?}", wallet.address()))
}

/// Enum of all valid API key identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr)]
pub enum ApiKeyId {
    #[strum(serialize = "GITHUB_TOKEN")]
    GithubToken,
    #[strum(serialize = "BANKR_API_KEY")]
    BankrApiKey,
    #[strum(serialize = "MOLTX_API_KEY")]
    MoltxApiKey,
    #[strum(serialize = "DISCORD_BOT_TOKEN")]
    DiscordBotToken,
    #[strum(serialize = "TELEGRAM_BOT_TOKEN")]
    TelegramBotToken,
    #[strum(serialize = "SLACK_BOT_TOKEN")]
    SlackBotToken,
    #[strum(serialize = "MOLTBOOK_TOKEN")]
    MoltbookToken,
    #[strum(serialize = "FOURCLAW_TOKEN")]
    FourclawToken,
    #[strum(serialize = "X402BOOK_TOKEN")]
    X402bookToken,
    #[strum(serialize = "TWITTER_CONSUMER_KEY")]
    TwitterConsumerKey,
    #[strum(serialize = "TWITTER_CONSUMER_SECRET")]
    TwitterConsumerSecret,
    #[strum(serialize = "TWITTER_ACCESS_TOKEN")]
    TwitterAccessToken,
    #[strum(serialize = "TWITTER_ACCESS_TOKEN_SECRET")]
    TwitterAccessTokenSecret,
}

impl ApiKeyId {
    /// The key name as stored in the database
    pub fn as_str(&self) -> &'static str {
        // AsRefStr from strum provides static string references
        match self {
            Self::GithubToken => "GITHUB_TOKEN",
            Self::BankrApiKey => "BANKR_API_KEY",
            Self::MoltxApiKey => "MOLTX_API_KEY",
            Self::DiscordBotToken => "DISCORD_BOT_TOKEN",
            Self::TelegramBotToken => "TELEGRAM_BOT_TOKEN",
            Self::SlackBotToken => "SLACK_BOT_TOKEN",
            Self::MoltbookToken => "MOLTBOOK_TOKEN",
            Self::FourclawToken => "FOURCLAW_TOKEN",
            Self::X402bookToken => "X402BOOK_TOKEN",
            Self::TwitterConsumerKey => "TWITTER_CONSUMER_KEY",
            Self::TwitterConsumerSecret => "TWITTER_CONSUMER_SECRET",
            Self::TwitterAccessToken => "TWITTER_ACCESS_TOKEN",
            Self::TwitterAccessTokenSecret => "TWITTER_ACCESS_TOKEN_SECRET",
        }
    }

    /// Environment variable names to set when this key is available
    pub fn env_vars(&self) -> Option<&'static [&'static str]> {
        match self {
            Self::GithubToken => Some(&["GH_TOKEN", "GITHUB_TOKEN"]),
            Self::BankrApiKey => Some(&["BANKR_API_KEY"]),
            Self::MoltxApiKey => Some(&["MOLTX_API_KEY"]),
            Self::DiscordBotToken => Some(&["DISCORD_BOT_TOKEN", "DISCORD_TOKEN"]),
            Self::TelegramBotToken => Some(&["TELEGRAM_BOT_TOKEN", "TELEGRAM_TOKEN"]),
            Self::SlackBotToken => Some(&["SLACK_BOT_TOKEN", "SLACK_TOKEN"]),
            Self::MoltbookToken => Some(&["MOLTBOOK_TOKEN"]),
            Self::FourclawToken => Some(&["FOURCLAW_TOKEN"]),
            Self::X402bookToken => Some(&["X402BOOK_TOKEN"]),
            Self::TwitterConsumerKey => Some(&["TWITTER_CONSUMER_KEY", "TWITTER_API_KEY"]),
            Self::TwitterConsumerSecret => Some(&["TWITTER_CONSUMER_SECRET", "TWITTER_API_SECRET"]),
            Self::TwitterAccessToken => Some(&["TWITTER_ACCESS_TOKEN"]),
            Self::TwitterAccessTokenSecret => Some(&["TWITTER_ACCESS_TOKEN_SECRET"]),
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

/// Configuration for a service group (e.g., "github" groups GITHUB_TOKEN)
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
            group: "moltx",
            label: "MoltX",
            description: "X for agents. Get an API key from moltx.io after registering your agent.",
            url: "https://moltx.io",
            keys: vec![KeyConfig {
                name: "MOLTX_API_KEY",
                label: "API Key",
                secret: true,
            }],
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
        ServiceConfig {
            group: "4claw",
            label: "4claw",
            description: "4claw network for AI agents. Get your API token from 4claw.org",
            url: "https://4claw.org",
            keys: vec![KeyConfig {
                name: "FOURCLAW_TOKEN",
                label: "API Token",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "x402book",
            label: "x402book",
            description: "x402book network for AI agents. Get your API token from x402book.com",
            url: "https://api.x402book.com",
            keys: vec![KeyConfig {
                name: "X402BOOK_TOKEN",
                label: "API Token",
                secret: true,
            }],
        },
        ServiceConfig {
            group: "twitter",
            label: "Twitter/X",
            description: "OAuth 1.0a credentials for posting tweets. Get all 4 keys from your Twitter Developer App's 'Keys and Tokens' tab.",
            url: "https://developer.twitter.com/en/portal/projects-and-apps",
            keys: vec![
                KeyConfig {
                    name: "TWITTER_CONSUMER_KEY",
                    label: "API Key (Consumer Key)",
                    secret: true,
                },
                KeyConfig {
                    name: "TWITTER_CONSUMER_SECRET",
                    label: "API Secret (Consumer Secret)",
                    secret: true,
                },
                KeyConfig {
                    name: "TWITTER_ACCESS_TOKEN",
                    label: "Access Token",
                    secret: true,
                },
                KeyConfig {
                    name: "TWITTER_ACCESS_TOKEN_SECRET",
                    label: "Access Token Secret",
                    secret: true,
                },
            ],
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
    pub has_settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_heartbeat: Option<bool>,
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
    pub has_settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_heartbeat: Option<bool>,
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
    let signature = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            wallet.sign_message(message).await
        })
    }).map_err(|e| format!("Failed to sign message: {}", e))?;

    Ok(format!("0x{}", hex::encode(signature.to_vec())))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/keys")
            .route("", web::get().to(list_api_keys))
            .route("", web::post().to(upsert_api_key))
            .route("", web::delete().to(delete_api_key))
            .route("/config", web::get().to(get_configs))
            .route("/cloud_backup", web::post().to(backup_to_cloud))
            .route("/cloud_restore", web::post().to(restore_from_cloud))
            .route("/cloud_preview", web::get().to(preview_cloud_keys)),
    );
}

async fn get_configs(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

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

/// Backup all user data to cloud (encrypted with burner wallet key)
async fn backup_to_cloud(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Get burner wallet private key from config
    let private_key = match &state.config.burner_wallet_private_key {
        Some(pk) => pk.clone(),
        None => {
            return HttpResponse::BadRequest().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Burner wallet not configured".to_string()),
            });
        }
    };

    // Get wallet address
    let wallet_address = match get_wallet_address(&private_key) {
        Some(addr) => addr,
        None => {
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Failed to derive wallet address".to_string()),
            });
        }
    };

    // Build BackupData with all user data
    let mut backup = BackupData::new(wallet_address);

    // Get all API keys with values
    let keys = match state.db.list_api_keys_with_values() {
        Ok(k) => k,
        Err(e) => {
            log::error!("Failed to list API keys: {}", e);
            return HttpResponse::InternalServerError().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Failed to export API keys".to_string()),
            });
        }
    };

    backup.api_keys = keys
        .iter()
        .map(|(name, value)| ApiKeyEntry {
            key_name: name.clone(),
            key_value: value.clone(),
        })
        .collect();

    // Get mind map nodes
    match state.db.list_mind_nodes() {
        Ok(nodes) => {
            backup.mind_map_nodes = nodes
                .iter()
                .map(|n| MindNodeEntry {
                    id: n.id,
                    body: n.body.clone(),
                    position_x: n.position_x,
                    position_y: n.position_y,
                    is_trunk: n.is_trunk,
                    created_at: n.created_at.to_rfc3339(),
                    updated_at: n.updated_at.to_rfc3339(),
                })
                .collect();
        }
        Err(e) => {
            log::warn!("Failed to list mind nodes for backup: {}", e);
        }
    }

    // Get mind map connections
    match state.db.list_mind_node_connections() {
        Ok(connections) => {
            backup.mind_map_connections = connections
                .iter()
                .map(|c| MindConnectionEntry {
                    parent_id: c.parent_id,
                    child_id: c.child_id,
                })
                .collect();
        }
        Err(e) => {
            log::warn!("Failed to list mind connections for backup: {}", e);
        }
    }

    // Get bot settings
    match state.db.get_bot_settings() {
        Ok(settings) => {
            // Serialize custom_rpc_endpoints as JSON string for backup
            let custom_rpc_json = settings
                .custom_rpc_endpoints
                .as_ref()
                .and_then(|h| serde_json::to_string(h).ok());

            backup.bot_settings = Some(BotSettingsEntry {
                bot_name: settings.bot_name.clone(),
                bot_email: settings.bot_email.clone(),
                web3_tx_requires_confirmation: settings.web3_tx_requires_confirmation,
                rpc_provider: Some(settings.rpc_provider.clone()),
                custom_rpc_endpoints: custom_rpc_json,
                max_tool_iterations: Some(settings.max_tool_iterations),
                rogue_mode_enabled: settings.rogue_mode_enabled,
                safe_mode_max_queries_per_10min: Some(settings.safe_mode_max_queries_per_10min),
            });
        }
        Err(e) => {
            log::warn!("Failed to get bot settings for backup: {}", e);
        }
    }

    // Get cron jobs
    match state.db.list_cron_jobs() {
        Ok(jobs) => {
            backup.cron_jobs = jobs
                .iter()
                .map(|j| CronJobEntry {
                    name: j.name.clone(),
                    description: j.description.clone(),
                    schedule_type: j.schedule_type.clone(),
                    schedule_value: j.schedule_value.clone(),
                    timezone: j.timezone.clone(),
                    session_mode: j.session_mode.clone(),
                    message: j.message.clone(),
                    system_event: j.system_event.clone(),
                    channel_id: j.channel_id,
                    deliver_to: j.deliver_to.clone(),
                    deliver: j.deliver,
                    model_override: j.model_override.clone(),
                    thinking_level: j.thinking_level.clone(),
                    timeout_seconds: j.timeout_seconds,
                    delete_after_run: j.delete_after_run,
                    status: j.status.clone(),
                })
                .collect();
        }
        Err(e) => {
            log::warn!("Failed to list cron jobs for backup: {}", e);
        }
    }

    // Get heartbeat config (we only backup the first/primary one if it exists)
    match state.db.list_heartbeat_configs() {
        Ok(configs) => {
            if let Some(config) = configs.into_iter().next() {
                backup.heartbeat_config = Some(HeartbeatConfigEntry {
                    channel_id: config.channel_id,
                    interval_minutes: config.interval_minutes,
                    target: config.target.clone(),
                    active_hours_start: config.active_hours_start.clone(),
                    active_hours_end: config.active_hours_end.clone(),
                    active_days: config.active_days.clone(),
                    enabled: config.enabled,
                });
            } else {
                log::debug!("No heartbeat config to backup");
            }
        }
        Err(e) => {
            log::warn!("Failed to get heartbeat config for backup: {}", e);
        }
    }

    // Get channel settings
    match state.db.get_all_channel_settings() {
        Ok(settings) => {
            backup.channel_settings = settings
                .iter()
                .map(|s| ChannelSettingEntry {
                    channel_id: s.channel_id,
                    setting_key: s.setting_key.clone(),
                    setting_value: s.setting_value.clone(),
                })
                .collect();
        }
        Err(e) => {
            log::warn!("Failed to get channel settings for backup: {}", e);
        }
    }

    // Get channels (non-safe-mode only)
    match state.db.list_channels_for_backup() {
        Ok(channels) => {
            backup.channels = channels
                .iter()
                .map(|c| ChannelEntry {
                    id: c.id,
                    channel_type: c.channel_type.clone(),
                    name: c.name.clone(),
                    enabled: c.enabled,
                    bot_token: c.bot_token.clone(),
                    app_token: c.app_token.clone(),
                })
                .collect();
        }
        Err(e) => {
            log::warn!("Failed to get channels for backup: {}", e);
        }
    }

    // Check if there's anything to backup
    if backup.api_keys.is_empty() && backup.mind_map_nodes.is_empty() && backup.cron_jobs.is_empty() && backup.bot_settings.is_none() && backup.heartbeat_config.is_none() && backup.channel_settings.is_empty() && backup.channels.is_empty() {
        return HttpResponse::BadRequest().json(BackupResponse {
            success: false,
            key_count: None,
            node_count: None,
            connection_count: None,
            cron_job_count: None,
            channel_count: None,
            channel_setting_count: None,
            has_settings: None,
            has_heartbeat: None,
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
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Failed to serialize backup".to_string()),
            });
        }
    };

    // Encrypt with ECIES using the burner wallet's public key
    let encrypted_data = match encrypt_with_private_key(&private_key, &backup_json) {
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
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Failed to encrypt backup".to_string()),
            });
        }
    };

    // Upload to keystore API (with SIWE authentication)
    match KEYSTORE_CLIENT.store_keys(&private_key, &encrypted_data, item_count).await {
        Ok(resp) if resp.success => {
            // Record backup in local state
            if let Err(e) = state.db.record_keystore_backup(&backup.wallet_address, backup.version, item_count) {
                log::warn!("Failed to record backup: {}", e);
            }

            HttpResponse::Ok().json(BackupResponse {
                success: true,
                key_count: Some(key_count),
                node_count: Some(node_count),
                connection_count: Some(connection_count),
                cron_job_count: Some(cron_job_count),
                channel_count: Some(channel_count),
                channel_setting_count: Some(channel_setting_count),
                has_settings: Some(has_settings),
                has_heartbeat: Some(has_heartbeat),
                message: Some(format!(
                    "Backed up {} items ({} keys, {} nodes, {} connections, {} cron jobs, {} channels, {} channel settings{}{})",
                    item_count,
                    key_count,
                    node_count,
                    connection_count,
                    cron_job_count,
                    channel_count,
                    channel_setting_count,
                    if has_settings { ", settings" } else { "" },
                    if has_heartbeat { ", heartbeat" } else { "" }
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
                has_settings: None,
                has_heartbeat: None,
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
                has_settings: None,
                has_heartbeat: None,
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

    // Get burner wallet private key from config
    let private_key = match &state.config.burner_wallet_private_key {
        Some(pk) => pk.clone(),
        None => {
            return HttpResponse::BadRequest().json(BackupResponse {
                success: false,
                key_count: None,
                node_count: None,
                connection_count: None,
                cron_job_count: None,
                channel_count: None,
                channel_setting_count: None,
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Burner wallet not configured".to_string()),
            });
        }
    };

    // Fetch from keystore API (with SIWE authentication)
    let keystore_resp = match KEYSTORE_CLIENT.get_keys(&private_key).await {
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
                has_settings: None,
                has_heartbeat: None,
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
                has_settings: None,
                has_heartbeat: None,
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
            has_settings: None,
            has_heartbeat: None,
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
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("No encrypted data in response".to_string()),
            });
        }
    };

    // Decrypt with ECIES using the burner wallet's private key
    let decrypted_json = match decrypt_with_private_key(&private_key, &encrypted_data) {
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
                has_settings: None,
                has_heartbeat: None,
                message: None,
                error: Some("Failed to decrypt backup (wrong wallet?)".to_string()),
            });
        }
    };

    // Try to parse as new BackupData format first, fall back to legacy Vec<BackupKey>
    let backup_data: BackupData = match serde_json::from_str(&decrypted_json) {
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
                        has_settings: None,
                        has_heartbeat: None,
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
        if get_valid_key_names().contains(&key.key_name.as_str()) {
            if let Err(e) = state.db.upsert_api_key(&key.key_name, &key.key_value) {
                log::error!("Failed to restore key {}: {}", key.key_name, e);
            } else {
                restored_keys += 1;
            }
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
        ) {
            log::warn!("Failed to restore bot settings: {}", e);
        }
    }

    // Restore cron jobs (already cleared above)
    let mut restored_cron_jobs = 0;
    for job in &backup_data.cron_jobs {
        match state.db.create_cron_job(
            &job.name,
            job.description.as_deref(),
            &job.schedule_type,
            &job.schedule_value,
            job.timezone.as_deref(),
            &job.session_mode,
            job.message.as_deref(),
            job.system_event.as_deref(),
            job.channel_id,
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

    // Restore heartbeat config if present
    let has_heartbeat = backup_data.heartbeat_config.is_some();
    if let Some(hb_config) = &backup_data.heartbeat_config {
        // Get or create the heartbeat config for the channel
        match state.db.get_or_create_heartbeat_config(hb_config.channel_id) {
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

    // Restore channels first (we need ID mapping for channel settings)
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
        has_settings: Some(has_settings),
        has_heartbeat: Some(has_heartbeat),
        message: Some(format!(
            "Restored {} keys, {} nodes, {} connections, {} cron jobs, {} channels, {} channel settings{}{}",
            restored_keys,
            restored_nodes,
            restored_connections,
            restored_cron_jobs,
            restored_channels,
            restored_channel_settings,
            if has_settings { ", settings" } else { "" },
            if has_heartbeat { ", heartbeat" } else { "" }
        )),
        error: None,
    })
}

/// Encrypt data using ECIES with the public key derived from private key
fn encrypt_with_private_key(private_key: &str, data: &str) -> Result<String, String> {
    use ecies::{encrypt, PublicKey, SecretKey};

    // Parse private key (remove 0x prefix if present)
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;

    // Create secret key and derive public key
    let secret_key = SecretKey::parse_slice(&pk_bytes)
        .map_err(|e| format!("Invalid private key: {:?}", e))?;
    let public_key = PublicKey::from_secret_key(&secret_key);

    // Encrypt the data
    let encrypted = encrypt(&public_key.serialize(), data.as_bytes())
        .map_err(|e| format!("Encryption failed: {:?}", e))?;

    Ok(hex::encode(encrypted))
}

/// Decrypt data using ECIES with the private key
fn decrypt_with_private_key(private_key: &str, encrypted_hex: &str) -> Result<String, String> {
    use ecies::{decrypt, SecretKey};

    // Parse private key (remove 0x prefix if present)
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;

    // Parse encrypted data
    let encrypted = hex::decode(encrypted_hex).map_err(|e| format!("Invalid encrypted data: {}", e))?;

    // Create secret key
    let secret_key = SecretKey::parse_slice(&pk_bytes)
        .map_err(|e| format!("Invalid private key: {:?}", e))?;

    // Decrypt the data
    let decrypted = decrypt(&secret_key.serialize(), &encrypted)
        .map_err(|e| format!("Decryption failed: {:?}", e))?;

    String::from_utf8(decrypted).map_err(|e| format!("Invalid UTF-8 in decrypted data: {}", e))
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

    // Get burner wallet private key from config
    let private_key = match &state.config.burner_wallet_private_key {
        Some(pk) => pk.clone(),
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
                has_settings: None,
                has_heartbeat: None,
                backup_version: None,
                message: None,
                error: Some("Burner wallet not configured".to_string()),
            });
        }
    };

    // Fetch from keystore API (with SIWE authentication)
    let keystore_resp = match KEYSTORE_CLIENT.get_keys(&private_key).await {
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
                has_settings: None,
                has_heartbeat: None,
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
                has_settings: None,
                has_heartbeat: None,
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
            has_settings: None,
            has_heartbeat: None,
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
                has_settings: None,
                has_heartbeat: None,
                backup_version: None,
                message: None,
                error: Some("No encrypted data in response".to_string()),
            });
        }
    };

    // Decrypt with ECIES using the burner wallet's private key
    let decrypted_json = match decrypt_with_private_key(&private_key, &encrypted_data) {
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
                has_settings: None,
                has_heartbeat: None,
                backup_version: None,
                message: None,
                error: Some("Failed to decrypt backup (wrong wallet?)".to_string()),
            });
        }
    };

    // Try to parse as new BackupData format first, fall back to legacy Vec<BackupKey>
    let valid_keys = get_valid_key_names();

    // Try new format first
    if let Ok(backup_data) = serde_json::from_str::<BackupData>(&decrypted_json) {
        let previews: Vec<CloudKeyPreview> = backup_data
            .api_keys
            .iter()
            .filter(|k| valid_keys.contains(&k.key_name.as_str()))
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
            has_settings: Some(backup_data.bot_settings.is_some()),
            has_heartbeat: Some(backup_data.heartbeat_config.is_some()),
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
                has_settings: None,
                has_heartbeat: None,
                backup_version: None,
                message: None,
                error: Some("Invalid backup data format".to_string()),
            });
        }
    };

    // Convert to previews (only valid key names, with masked values)
    let previews: Vec<CloudKeyPreview> = cloud_keys
        .iter()
        .filter(|k| valid_keys.contains(&k.key_name.as_str()))
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
        has_settings: None,
        has_heartbeat: None,
        backup_version: None,
        message: Some("Cloud keys retrieved successfully (legacy format)".to_string()),
        error: None,
    })
}
