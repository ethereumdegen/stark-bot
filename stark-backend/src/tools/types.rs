use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Tool groups for access control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolGroup {
    #[default]
    Web,
    Filesystem,
    Exec,
    Messaging,
    System,
}

impl ToolGroup {
    pub fn all() -> Vec<ToolGroup> {
        vec![
            ToolGroup::Web,
            ToolGroup::Filesystem,
            ToolGroup::Exec,
            ToolGroup::Messaging,
            ToolGroup::System,
        ]
    }

    pub fn from_str(s: &str) -> Option<ToolGroup> {
        match s.to_lowercase().as_str() {
            "web" => Some(ToolGroup::Web),
            "filesystem" | "fs" => Some(ToolGroup::Filesystem),
            "exec" => Some(ToolGroup::Exec),
            "messaging" => Some(ToolGroup::Messaging),
            "system" => Some(ToolGroup::System),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ToolGroup::Web => "web",
            ToolGroup::Filesystem => "filesystem",
            ToolGroup::Exec => "exec",
            ToolGroup::Messaging => "messaging",
            ToolGroup::System => "system",
        }
    }
}

/// Tool profiles for quick configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolProfile {
    /// No tools enabled
    None,
    /// Only web tools
    Minimal,
    /// Web + filesystem (read-only)
    Standard,
    /// Standard + messaging tools
    Messaging,
    /// All tools enabled
    Full,
    /// Custom configuration
    Custom,
}

impl Default for ToolProfile {
    fn default() -> Self {
        ToolProfile::Standard
    }
}

impl ToolProfile {
    pub fn allowed_groups(&self) -> Vec<ToolGroup> {
        match self {
            ToolProfile::None => vec![],
            ToolProfile::Minimal => vec![ToolGroup::Web],
            // Standard includes Exec because the exec tool has its own security restrictions
            // (deny list for dangerous commands, no shell metacharacters allowed)
            ToolProfile::Standard => vec![ToolGroup::Web, ToolGroup::Filesystem, ToolGroup::Exec],
            ToolProfile::Messaging => {
                vec![ToolGroup::Web, ToolGroup::Filesystem, ToolGroup::Exec, ToolGroup::Messaging]
            }
            ToolProfile::Full => ToolGroup::all(),
            ToolProfile::Custom => vec![], // Custom profile uses explicit allow/deny lists
        }
    }

    pub fn from_str(s: &str) -> Option<ToolProfile> {
        match s.to_lowercase().as_str() {
            "none" => Some(ToolProfile::None),
            "minimal" => Some(ToolProfile::Minimal),
            "standard" => Some(ToolProfile::Standard),
            "messaging" => Some(ToolProfile::Messaging),
            "full" => Some(ToolProfile::Full),
            "custom" => Some(ToolProfile::Custom),
            _ => None,
        }
    }
}

/// JSON Schema property definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<PropertySchema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
}

/// Tool input schema using JSON Schema format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: HashMap<String, PropertySchema>,
    #[serde(default)]
    pub required: Vec<String>,
}

impl Default for ToolInputSchema {
    fn default() -> Self {
        ToolInputSchema {
            schema_type: "object".to_string(),
            properties: HashMap::new(),
            required: vec![],
        }
    }
}

/// Tool definition that gets sent to the AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
    #[serde(skip)]
    pub group: ToolGroup,
}

/// Result of tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        ToolResult {
            success: true,
            content: content.into(),
            error: None,
            metadata: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let msg = message.into();
        ToolResult {
            success: false,
            content: msg.clone(),
            error: Some(msg),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Context provided to tools during execution
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub channel_id: Option<i64>,
    pub channel_type: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<i64>,
    pub identity_id: Option<String>,
    /// Base directory for file operations (sandbox root)
    pub workspace_dir: Option<String>,
    /// Additional context data
    pub extra: HashMap<String, Value>,
}

impl Default for ToolContext {
    fn default() -> Self {
        ToolContext {
            channel_id: None,
            channel_type: None,
            user_id: None,
            session_id: None,
            identity_id: None,
            workspace_dir: None,
            extra: HashMap::new(),
        }
    }
}

impl ToolContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_channel(mut self, channel_id: i64, channel_type: String) -> Self {
        self.channel_id = Some(channel_id);
        self.channel_type = Some(channel_type);
        self
    }

    pub fn with_user(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_session(mut self, session_id: i64) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn with_identity(mut self, identity_id: String) -> Self {
        self.identity_id = Some(identity_id);
        self
    }

    pub fn with_workspace(mut self, workspace_dir: String) -> Self {
        self.workspace_dir = Some(workspace_dir);
        self
    }

    /// Add an API key to the context (for use by tools)
    pub fn with_api_key(mut self, service: &str, key: String) -> Self {
        self.extra.insert(format!("api_key_{}", service), serde_json::json!(key));
        self
    }

    /// Get an API key from the context
    pub fn get_api_key(&self, service: &str) -> Option<String> {
        self.extra.get(&format!("api_key_{}", service))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Add bot config to the context (for use by tools like exec for git commits)
    pub fn with_bot_config(mut self, bot_name: String, bot_email: String) -> Self {
        self.extra.insert("bot_name".to_string(), serde_json::json!(bot_name));
        self.extra.insert("bot_email".to_string(), serde_json::json!(bot_email));
        self
    }

    /// Get bot name from the context
    pub fn get_bot_name(&self) -> String {
        self.extra.get("bot_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "StarkBot".to_string())
    }

    /// Get bot email from the context
    pub fn get_bot_email(&self) -> String {
        self.extra.get("bot_email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "starkbot@users.noreply.github.com".to_string())
    }
}

/// Tool configuration stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub id: Option<i64>,
    pub channel_id: Option<i64>, // NULL for global
    pub profile: ToolProfile,
    pub allow_list: Vec<String>,    // Specific tools to allow
    pub deny_list: Vec<String>,     // Specific tools to deny
    pub allowed_groups: Vec<String>, // Tool groups to allow
    pub denied_groups: Vec<String>,  // Tool groups to deny
}

impl Default for ToolConfig {
    fn default() -> Self {
        ToolConfig {
            id: None,
            channel_id: None,
            profile: ToolProfile::Standard,
            allow_list: vec![],
            deny_list: vec![],
            allowed_groups: vec!["web".to_string(), "filesystem".to_string(), "exec".to_string()],
            denied_groups: vec![],
        }
    }
}

impl ToolConfig {
    /// Check if a tool is allowed by this configuration
    pub fn is_tool_allowed(&self, tool_name: &str, tool_group: ToolGroup) -> bool {
        // Explicit deny takes precedence
        if self.deny_list.contains(&tool_name.to_string()) {
            return false;
        }

        // Explicit allow overrides group settings
        if self.allow_list.contains(&tool_name.to_string()) {
            return true;
        }

        // Check group denial
        let group_str = tool_group.as_str().to_string();
        if self.denied_groups.contains(&group_str) {
            return false;
        }

        // Check profile or custom group allowance
        match &self.profile {
            ToolProfile::Custom => self.allowed_groups.contains(&group_str),
            _ => self.profile.allowed_groups().contains(&tool_group),
        }
    }
}

/// Tool execution record for audit logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub id: Option<i64>,
    pub channel_id: i64,
    pub tool_name: String,
    pub parameters: Value,
    pub success: bool,
    pub result: Option<String>,
    pub duration_ms: Option<i64>,
    pub executed_at: String,
}
