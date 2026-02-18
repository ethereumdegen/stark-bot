use serde::{Deserialize, Serialize};

/// A named special role that grants additional tools/skills to safe-mode users.
/// Tools and skills are granted by their exact names (not tags).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialRole {
    pub name: String,
    /// Individual tool names granted to this role (e.g. ["x402_preset_fetch", "web_fetch"])
    pub allowed_tools: Vec<String>,
    /// Individual skill names granted to this role (e.g. ["image_generation", "weather"])
    pub allowed_skills: Vec<String>,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Links a (channel_type, user_id) pair to a special role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialRoleAssignment {
    pub id: i64,
    pub channel_type: String,
    pub user_id: String,
    pub special_role_name: String,
    pub label: Option<String>,
    pub created_at: String,
}

/// Grant set for a specific user — the single role's tools/skills (one role per user/channel).
/// Both tools and skills are referenced by exact name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecialRoleGrants {
    pub role_name: Option<String>,
    pub description: Option<String>,
    /// Individual tool names granted (e.g. ["x402_preset_fetch"])
    pub extra_tools: Vec<String>,
    /// Individual skill names granted (e.g. ["image_generation"])
    pub extra_skills: Vec<String>,
}

impl SpecialRoleGrants {
    pub fn is_empty(&self) -> bool {
        self.extra_tools.is_empty() && self.extra_skills.is_empty()
    }
}
