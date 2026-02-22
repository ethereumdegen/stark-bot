//! Discord user profile operations — delegates to the discord-tipping-service via RPC.
//!
//! All functions are async and call the standalone service directly using raw reqwest.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A Discord user profile (local definition — mirrors discord-tipping-service schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUserProfile {
    pub id: i64,
    pub discord_user_id: String,
    pub discord_username: Option<String>,
    pub public_address: Option<String>,
    pub registration_status: String,
    pub registered_at: Option<String>,
    pub last_interaction_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Generic RPC response wrapper
#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

fn service_url() -> String {
    std::env::var("DISCORD_TIPPING_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9101".to_string())
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Initialize the discord_user_profiles table.
/// Now a no-op — the service manages its own schema.
pub fn init_tables(_conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    Ok(())
}

/// Get or create a Discord user profile
pub async fn get_or_create_profile(
    _db: &crate::db::Database,
    discord_user_id: &str,
    username: &str,
) -> Result<DiscordUserProfile, String> {
    let url = format!("{}/rpc/profile/get_or_create", service_url());
    let body = json!({
        "discord_user_id": discord_user_id,
        "username": username,
    });
    let resp: RpcResponse<DiscordUserProfile> = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    resp.data.ok_or_else(|| resp.error.unwrap_or_else(|| "Unknown error".to_string()))
}

/// Get a Discord user profile by user ID
pub async fn get_profile(
    _db: &crate::db::Database,
    discord_user_id: &str,
) -> Result<Option<DiscordUserProfile>, String> {
    let url = format!("{}/rpc/profile/get", service_url());
    let body = json!({ "discord_user_id": discord_user_id });
    let resp: RpcResponse<Option<DiscordUserProfile>> = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    if resp.success {
        Ok(resp.data.flatten())
    } else {
        Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
    }
}

/// Get a Discord user profile by public address
pub async fn get_profile_by_address(
    _db: &crate::db::Database,
    address: &str,
) -> Result<Option<DiscordUserProfile>, String> {
    let url = format!("{}/rpc/profile/get_by_address", service_url());
    let body = json!({ "address": address });
    let resp: RpcResponse<Option<DiscordUserProfile>> = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    if resp.success {
        Ok(resp.data.flatten())
    } else {
        Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
    }
}

/// Register a public address for a Discord user
pub async fn register_address(
    _db: &crate::db::Database,
    discord_user_id: &str,
    address: &str,
) -> Result<(), String> {
    let url = format!("{}/rpc/profile/register", service_url());
    let body = json!({
        "discord_user_id": discord_user_id,
        "address": address,
    });
    let resp: RpcResponse<Value> = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    if resp.success {
        Ok(())
    } else {
        Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
    }
}

/// Unregister a public address for a Discord user
pub async fn unregister_address(
    _db: &crate::db::Database,
    discord_user_id: &str,
) -> Result<(), String> {
    let url = format!("{}/rpc/profile/unregister", service_url());
    let body = json!({ "discord_user_id": discord_user_id });
    let resp: RpcResponse<Value> = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    if resp.success {
        Ok(())
    } else {
        Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
    }
}

/// List all registered profiles (those with a public address)
pub async fn list_registered_profiles(
    _db: &crate::db::Database,
) -> Result<Vec<DiscordUserProfile>, String> {
    let url = format!("{}/rpc/profiles/registered", service_url());
    let resp: RpcResponse<Vec<DiscordUserProfile>> = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    resp.data.ok_or_else(|| resp.error.unwrap_or_else(|| "Unknown error".to_string()))
}

/// List all profiles (registered and unregistered) — used for module dashboard
pub async fn list_all_profiles(
    _db: &crate::db::Database,
) -> Result<Vec<DiscordUserProfile>, String> {
    let url = format!("{}/rpc/profiles/all", service_url());
    let resp: RpcResponse<Vec<DiscordUserProfile>> = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Discord tipping service unavailable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Invalid response from discord tipping service: {}", e))?;
    resp.data.ok_or_else(|| resp.error.unwrap_or_else(|| "Unknown error".to_string()))
}

/// Clear all discord user registrations (for restore)
pub fn clear_registrations_for_restore(_db: &crate::db::Database) -> Result<usize, String> {
    // This is handled by the backup/restore endpoint now
    Ok(0)
}

#[cfg(test)]
mod tests {
    fn is_valid_address(addr: &str) -> bool {
        addr.starts_with("0x")
            && addr.len() >= 42
            && addr.len() <= 66
            && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn test_address_validation() {
        assert!(is_valid_address("0x1234567890123456789012345678901234567890"));
        assert!(is_valid_address(
            "0x0123456789012345678901234567890123456789012345678901234567890123"
        ));
        assert!(!is_valid_address("0x123"));
        assert!(!is_valid_address("1234567890123456789012345678901234567890"));
        assert!(!is_valid_address("0xGGGG567890123456789012345678901234567890"));
    }
}
