use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Identity link - maps platform users to a unified identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityLink {
    pub id: i64,
    pub identity_id: String,
    pub channel_type: String,
    pub platform_user_id: String,
    pub platform_user_name: Option<String>,
    pub is_verified: bool,
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to get or create an identity link
#[derive(Debug, Clone, Deserialize)]
pub struct GetOrCreateIdentityRequest {
    pub channel_type: String,
    pub platform_user_id: String,
    pub platform_user_name: Option<String>,
}

/// Request to link an existing identity to another platform
#[derive(Debug, Clone, Deserialize)]
pub struct LinkIdentityRequest {
    pub identity_id: String,
    pub channel_type: String,
    pub platform_user_id: String,
    pub platform_user_name: Option<String>,
}

/// Information about a linked account
#[derive(Debug, Clone, Serialize)]
pub struct LinkedAccountInfo {
    pub channel_type: String,
    pub platform_user_id: String,
    pub platform_user_name: Option<String>,
    pub is_verified: bool,
}

/// Response containing identity information
#[derive(Debug, Clone, Serialize)]
pub struct IdentityResponse {
    pub identity_id: String,
    pub linked_accounts: Vec<LinkedAccountInfo>,
    pub created_at: DateTime<Utc>,
}

impl From<&IdentityLink> for LinkedAccountInfo {
    fn from(link: &IdentityLink) -> Self {
        LinkedAccountInfo {
            channel_type: link.channel_type.clone(),
            platform_user_id: link.platform_user_id.clone(),
            platform_user_name: link.platform_user_name.clone(),
            is_verified: link.is_verified,
        }
    }
}
