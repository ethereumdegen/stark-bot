//! Shared types for the discord tipping service and its RPC clients.

use serde::{Deserialize, Serialize};

// =====================================================
// Domain Types
// =====================================================

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

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileStats {
    pub total_profiles: i64,
    pub registered_count: i64,
    pub unregistered_count: i64,
}

// =====================================================
// RPC Request Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct GetOrCreateProfileRequest {
    pub discord_user_id: String,
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterAddressRequest {
    pub discord_user_id: String,
    pub address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnregisterAddressRequest {
    pub discord_user_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetProfileRequest {
    pub discord_user_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetProfileByAddressRequest {
    pub address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupRestoreRequest {
    pub profiles: Vec<BackupEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupEntry {
    pub discord_user_id: String,
    pub discord_username: Option<String>,
    pub public_address: String,
    pub registered_at: Option<String>,
}

// =====================================================
// RPC Response Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> RpcResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// =====================================================
// Service Status
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub uptime_secs: u64,
    pub total_profiles: i64,
    pub registered_count: i64,
}
