use ethers::core::k256::ecdsa::SigningKey;
use ethers::signers::{LocalWallet, Signer};
use std::env;
use std::path::PathBuf;

/// Environment variable names
pub mod env_vars {
    pub const LOGIN_ADMIN_PUBLIC_ADDRESS: &str = "LOGIN_ADMIN_PUBLIC_ADDRESS";
    pub const BURNER_WALLET_PRIVATE_KEY: &str = "BURNER_WALLET_BOT_PRIVATE_KEY";
    pub const PORT: &str = "PORT";
    pub const DATABASE_URL: &str = "DATABASE_URL";
    pub const PUBLIC_URL: &str = "STARK_PUBLIC_URL";
    pub const AUTO_SYNC_FROM_KEYSTORE: &str = "AUTO_SYNC_FROM_KEYSTORE";
}

/// Default values
pub mod defaults {
    pub const PORT: u16 = 8080;
    pub const DATABASE_URL: &str = "./.db/stark.db";
}

/// Returns the absolute path to the stark-backend directory.
pub fn backend_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Returns the absolute path to the monorepo root.
pub fn repo_root() -> PathBuf {
    backend_dir().parent().expect("backend_dir has no parent").to_path_buf()
}

/// Get the bot's own public URL
pub fn self_url() -> String {
    if let Ok(url) = env::var(env_vars::PUBLIC_URL) {
        return url.trim_end_matches('/').to_string();
    }
    let port = env::var(env_vars::PORT)
        .unwrap_or_else(|_| defaults::PORT.to_string());
    format!("http://localhost:{}", port)
}

/// Get the bot config directory (inside stark-backend)
pub fn bot_config_dir() -> PathBuf {
    backend_dir().join("config")
}

/// Get the runtime bot_config.ron path
pub fn bot_config_path() -> PathBuf {
    bot_config_dir().join("bot_config.ron")
}

/// Get the seed bot_config.ron path (repo root config/)
pub fn bot_config_seed_path() -> PathBuf {
    repo_root().join("config").join("bot_config.ron")
}

/// Get the runtime agent_preset.ron path
pub fn agent_preset_path() -> PathBuf {
    bot_config_dir().join("agent_preset.ron")
}

/// Derive the public address from a private key
fn derive_address_from_private_key(private_key: &str) -> Result<String, String> {
    let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
    let key_bytes = hex::decode(key_hex)
        .map_err(|e| format!("Invalid private key hex: {}", e))?;
    let signing_key = SigningKey::from_bytes(key_bytes.as_slice().into())
        .map_err(|e| format!("Invalid private key: {}", e))?;
    let wallet = LocalWallet::from(signing_key);
    Ok(format!("{:?}", wallet.address()).to_lowercase())
}

#[derive(Clone)]
pub struct Config {
    pub login_admin_public_address: Option<String>,
    pub burner_wallet_private_key: Option<String>,
    pub port: u16,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let burner_wallet_private_key = env::var(env_vars::BURNER_WALLET_PRIVATE_KEY).ok();

        let login_admin_public_address = env::var(env_vars::LOGIN_ADMIN_PUBLIC_ADDRESS)
            .ok()
            .or_else(|| {
                burner_wallet_private_key.as_ref().and_then(|pk| {
                    derive_address_from_private_key(pk)
                        .map_err(|e| log::warn!("Failed to derive address from private key: {}", e))
                        .ok()
                })
            });

        Self {
            login_admin_public_address,
            burner_wallet_private_key,
            port: env::var(env_vars::PORT)
                .unwrap_or_else(|_| defaults::PORT.to_string())
                .parse()
                .expect("PORT must be a valid number"),
            database_url: env::var(env_vars::DATABASE_URL)
                .unwrap_or_else(|_| defaults::DATABASE_URL.to_string()),
        }
    }
}
