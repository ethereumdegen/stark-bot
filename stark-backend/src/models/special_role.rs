use serde::{Deserialize, Serialize};

/// A named special role that grants additional tools/skills to safe-mode users.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialRole {
    pub name: String,
    pub allowed_tools: Vec<String>,
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
    pub created_at: String,
}

/// Grant set for a specific user — the single role's tools/skills (one role per user/channel).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecialRoleGrants {
    pub role_name: Option<String>,
    pub extra_tools: Vec<String>,
    pub extra_skills: Vec<String>,
}

impl SpecialRoleGrants {
    pub fn is_empty(&self) -> bool {
        self.extra_tools.is_empty() && self.extra_skills.is_empty()
    }
}
