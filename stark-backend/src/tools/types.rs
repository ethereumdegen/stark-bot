use crate::ai::multi_agent::SubAgentManager;
use crate::controllers::api_keys::ApiKeyId;
use crate::db::Database;
use crate::execution::ProcessManager;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::skills::SkillRegistry;
use crate::tools::register::RegisterStore;
use crate::tx_queue::TxQueueManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use strum::{EnumIter, IntoEnumIterator};

/// Tool groups for access control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, EnumIter)]
#[serde(rename_all = "lowercase")]
pub enum ToolGroup {
    /// System tools - always available (set_agent_subtype, ask_user, etc.)
    System,
    /// Web tools - web_fetch, etc.
    #[default]
    Web,
    /// Filesystem tools - read_file, list_files, etc.
    Filesystem,
    /// Finance/DeFi tools - web3_tx, x402_*, token_lookup, etc.
    Finance,
    /// Development tools - grep, glob, edit_file, git, etc.
    Development,
    /// Exec tools - exec command
    Exec,
    /// Messaging tools - agent_send, etc.
    Messaging,
    /// Social/Marketing tools - moltx, scheduling, etc.
    Social,
    /// Memory tools - long-term memory storage and retrieval
    Memory,
}

impl ToolGroup {
    /// Get all tool groups (using strum iterator)
    pub fn all() -> Vec<ToolGroup> {
        ToolGroup::iter().collect()
    }

    /// Human-readable label for UI display
    pub fn label(&self) -> &'static str {
        match self {
            ToolGroup::System => "System Tools",
            ToolGroup::Web => "Web Tools",
            ToolGroup::Filesystem => "Filesystem Tools",
            ToolGroup::Finance => "Finance/DeFi Tools",
            ToolGroup::Development => "Development Tools",
            ToolGroup::Exec => "Execution Tools",
            ToolGroup::Messaging => "Messaging Tools",
            ToolGroup::Social => "Social/Marketing Tools",
            ToolGroup::Memory => "Memory Tools",
        }
    }

    /// Description of what this group contains
    pub fn description(&self) -> &'static str {
        match self {
            ToolGroup::System => "Core agent tools like subtype switching and user interaction",
            ToolGroup::Web => "HTTP requests and web fetching",
            ToolGroup::Filesystem => "File reading and directory listing",
            ToolGroup::Finance => "Crypto transactions, DeFi operations, token lookups",
            ToolGroup::Development => "Code editing, git, grep, glob, and development utilities",
            ToolGroup::Exec => "Shell command execution",
            ToolGroup::Messaging => "Inter-agent and external messaging",
            ToolGroup::Social => "Social media and marketing integrations",
            ToolGroup::Memory => "Long-term memory storage and retrieval",
        }
    }

    /// Parse from string (case-insensitive, with aliases)
    pub fn from_str(s: &str) -> Option<ToolGroup> {
        match s.to_lowercase().as_str() {
            "system" => Some(ToolGroup::System),
            "web" => Some(ToolGroup::Web),
            "filesystem" | "fs" => Some(ToolGroup::Filesystem),
            "finance" | "defi" | "crypto" => Some(ToolGroup::Finance),
            "development" | "dev" | "code" => Some(ToolGroup::Development),
            "exec" => Some(ToolGroup::Exec),
            "messaging" => Some(ToolGroup::Messaging),
            "social" | "marketing" | "moltx" => Some(ToolGroup::Social),
            "memory" => Some(ToolGroup::Memory),
            _ => None,
        }
    }

    /// Get the string key (for serialization/API)
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolGroup::System => "system",
            ToolGroup::Web => "web",
            ToolGroup::Filesystem => "filesystem",
            ToolGroup::Finance => "finance",
            ToolGroup::Development => "development",
            ToolGroup::Exec => "exec",
            ToolGroup::Messaging => "messaging",
            ToolGroup::Social => "social",
            ToolGroup::Memory => "memory",
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
    /// Finance specialist - Web + Filesystem + Finance tools
    Finance,
    /// Developer specialist - Web + Filesystem + Exec + Development tools
    Developer,
    /// Secretary specialist - Web + Filesystem + Messaging + Social tools
    Secretary,
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
                vec![
                    ToolGroup::Web,
                    ToolGroup::Filesystem,
                    ToolGroup::Exec,
                    ToolGroup::Messaging,
                ]
            }
            ToolProfile::Finance => {
                vec![
                    ToolGroup::Web,
                    ToolGroup::Filesystem,
                    ToolGroup::Finance,
                    ToolGroup::System,
                ]
            }
            ToolProfile::Developer => {
                vec![
                    ToolGroup::Web,
                    ToolGroup::Filesystem,
                    ToolGroup::Exec,
                    ToolGroup::Development,
                    ToolGroup::System,
                ]
            }
            ToolProfile::Secretary => {
                vec![
                    ToolGroup::Web,
                    ToolGroup::Filesystem,
                    ToolGroup::Exec,
                    ToolGroup::Messaging,
                    ToolGroup::Social,
                    ToolGroup::System,
                ]
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
            "finance" => Some(ToolProfile::Finance),
            "developer" | "dev" => Some(ToolProfile::Developer),
            "secretary" | "social" | "marketing" => Some(ToolProfile::Secretary),
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
    /// If set, indicates the agent should retry after this many seconds.
    /// Used for transient network errors with exponential backoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        ToolResult {
            success: true,
            content: content.into(),
            error: None,
            metadata: None,
            retry_after_secs: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let msg = message.into();
        ToolResult {
            success: false,
            content: msg.clone(),
            error: Some(msg),
            metadata: None,
            retry_after_secs: None,
        }
    }

    /// Create a retryable error result with exponential backoff hint.
    /// The agent will be instructed to wait and retry after the specified seconds.
    pub fn retryable_error(message: impl Into<String>, retry_after_secs: u64) -> Self {
        let msg = message.into();
        ToolResult {
            success: false,
            content: format!(
                "{}\n\n⏳ This appears to be a temporary network error. Waiting {} seconds before retrying...",
                msg, retry_after_secs
            ),
            error: Some(msg),
            metadata: None,
            retry_after_secs: Some(retry_after_secs),
        }
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_retry_after(mut self, secs: u64) -> Self {
        self.retry_after_secs = Some(secs);
        self
    }

    /// Check if this result indicates the tool should be retried
    pub fn should_retry(&self) -> bool {
        self.retry_after_secs.is_some()
    }
}

/// Context provided to tools during execution
#[derive(Clone)]
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
    /// Event broadcaster for real-time events (e.g., tx.pending)
    pub broadcaster: Option<Arc<EventBroadcaster>>,
    /// Register store for passing data between tools safely
    /// This prevents hallucination of critical data (like tx params)
    pub registers: RegisterStore,
    /// Database access for tools that need it (memory tools, etc.)
    pub database: Option<Arc<Database>>,
    /// SubAgent manager for spawning and managing sub-agents
    pub subagent_manager: Option<Arc<SubAgentManager>>,
    /// Process manager for background command execution
    pub process_manager: Option<Arc<ProcessManager>>,
    /// Skill registry for managing skills
    pub skill_registry: Option<Arc<SkillRegistry>>,
    /// Transaction queue manager for queued web3 transactions
    pub tx_queue: Option<Arc<TxQueueManager>>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("channel_id", &self.channel_id)
            .field("channel_type", &self.channel_type)
            .field("user_id", &self.user_id)
            .field("session_id", &self.session_id)
            .field("identity_id", &self.identity_id)
            .field("workspace_dir", &self.workspace_dir)
            .field("extra", &self.extra)
            .field("broadcaster", &self.broadcaster.is_some())
            .field("registers", &self.registers.keys())
            .field("database", &self.database.is_some())
            .field("subagent_manager", &self.subagent_manager.is_some())
            .field("process_manager", &self.process_manager.is_some())
            .field("skill_registry", &self.skill_registry.is_some())
            .field("tx_queue", &self.tx_queue.is_some())
            .finish()
    }
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
            broadcaster: None,
            registers: RegisterStore::new(),
            database: None,
            subagent_manager: None,
            process_manager: None,
            skill_registry: None,
            tx_queue: None,
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

    /// Add an API key to the context by string name (for backwards compatibility)
    /// Keys are stored by their exact name (e.g., "GITHUB_TOKEN", "MOLTX_API_KEY")
    pub fn with_api_key(mut self, key_name: &str, key_value: String) -> Self {
        self.extra.insert(
            format!("api_key_{}", key_name),
            serde_json::json!(key_value),
        );
        self
    }

    /// Add an API key to the context using the type-safe ApiKeyId enum
    pub fn with_api_key_id(self, key_id: ApiKeyId, key_value: String) -> Self {
        self.with_api_key(key_id.as_str(), key_value)
    }

    /// Get an API key from the context by its exact string name
    /// Example: get_api_key("GITHUB_TOKEN")
    pub fn get_api_key(&self, key_name: &str) -> Option<String> {
        self.extra.get(&format!("api_key_{}", key_name))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get an API key from the context using the type-safe ApiKeyId enum
    /// This is the preferred method as it prevents typos in key names
    pub fn get_api_key_by_id(&self, key_id: ApiKeyId) -> Option<String> {
        self.get_api_key(key_id.as_str())
    }

    /// Add bot config to the context (for use by tools like exec for git commits)
    pub fn with_bot_config(mut self, bot_name: String, bot_email: String) -> Self {
        self.extra.insert("bot_name".to_string(), serde_json::json!(bot_name));
        self.extra.insert("bot_email".to_string(), serde_json::json!(bot_email));
        self
    }

    /// Add an event broadcaster to the context (for tools to emit real-time events)
    pub fn with_broadcaster(mut self, broadcaster: Arc<EventBroadcaster>) -> Self {
        self.broadcaster = Some(broadcaster);
        self
    }

    /// Add a register store to the context (for passing data between tools safely)
    pub fn with_registers(mut self, registers: RegisterStore) -> Self {
        self.registers = registers;
        self
    }

    /// Add database access to the context (for memory tools, etc.)
    pub fn with_database(mut self, database: Arc<Database>) -> Self {
        self.database = Some(database);
        self
    }

    /// Add a SubAgentManager to the context (for spawning sub-agents)
    pub fn with_subagent_manager(mut self, manager: Arc<SubAgentManager>) -> Self {
        self.subagent_manager = Some(manager);
        self
    }

    /// Add a ProcessManager to the context (for background command execution)
    pub fn with_process_manager(mut self, manager: Arc<ProcessManager>) -> Self {
        self.process_manager = Some(manager);
        self
    }

    /// Add a SkillRegistry to the context (for skill management tools)
    pub fn with_skill_registry(mut self, registry: Arc<SkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    /// Add a TxQueueManager to the context (for web3 transaction queuing)
    pub fn with_tx_queue(mut self, tx_queue: Arc<TxQueueManager>) -> Self {
        self.tx_queue = Some(tx_queue);
        self
    }

    /// Set a register value and broadcast the update to connected clients.
    /// This is the preferred way to set registers when you want real-time updates in the UI.
    pub fn set_register(&self, key: &str, value: Value, source_tool: &str) {
        // Set the register value
        self.registers.set(key, value, source_tool);

        // Broadcast the update if we have a broadcaster and channel
        if let (Some(broadcaster), Some(channel_id)) = (&self.broadcaster, self.channel_id) {
            let registers_snapshot = self.get_registers_snapshot();
            broadcaster.broadcast(GatewayEvent::register_update(channel_id, registers_snapshot));
        }
    }

    /// Get a snapshot of all registers as JSON for broadcasting
    pub fn get_registers_snapshot(&self) -> Value {
        let keys = self.registers.keys();
        let mut map = serde_json::Map::new();
        for key in keys {
            if let Some(entry) = self.registers.get_entry(&key) {
                map.insert(key, serde_json::json!({
                    "value": entry.value,
                    "source": entry.source_tool,
                    "age_secs": entry.created_at.elapsed().as_secs()
                }));
            }
        }
        Value::Object(map)
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
        // Default to Full profile since AgentSubtype already controls which tools
        // are visible to the agent. The ToolConfig should not block tools that
        // the subtype system expects to be available.
        ToolConfig {
            id: None,
            channel_id: None,
            profile: ToolProfile::Full,
            allow_list: vec![],
            deny_list: vec![],
            allowed_groups: ToolGroup::all().iter().map(|g| g.as_str().to_string()).collect(),
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
