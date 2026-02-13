use crate::ai::multi_agent::SubAgentManager;
use crate::controllers::api_keys::ApiKeyId;
use crate::db::Database;
use crate::disk_quota::DiskQuotaManager;
use crate::execution::ProcessManager;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::qmd_memory::MemoryStore;
use crate::skills::SkillRegistry;
use crate::tools::register::RegisterStore;
use crate::tx_queue::TxQueueManager;
use crate::wallet::WalletProvider;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use strum::{EnumIter, IntoEnumIterator};

/// Describes what kind of rich content a channel can render
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelOutputType {
    /// Plain text only — Discord, Telegram, Twitter, Slack
    #[default]
    TextOnly,
    /// Supports HTML rendering — Web UI (iframes, embeds, etc.)
    RichHtml,
}

/// Safety level for tool access in restricted contexts.
/// Determines where a tool can be used. Higher levels are available in more contexts.
/// Defaults to Standard — new tools must explicitly opt in to be available in restricted modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum ToolSafetyLevel {
    /// Only available in normal (unrestricted) mode. NOT available to read-only subagents or safe mode.
    /// This is the default — new tools start here so they can't accidentally leak into restricted contexts.
    #[default]
    Standard,
    /// Available in normal mode AND read-only subagents. Can observe but no side effects.
    /// Examples: read_file, grep, glob, web_fetch, dexscreener
    ReadOnly,
    /// Available everywhere: normal, read-only subagents, AND safe mode (untrusted users).
    /// SECURITY: Only add tools here that are safe for completely untrusted external input.
    /// Examples: say_to_user, token_lookup, memory_read, discord_read
    SafeMode,
}

impl ToolSafetyLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolSafetyLevel::Standard => "standard",
            ToolSafetyLevel::ReadOnly => "read_only",
            ToolSafetyLevel::SafeMode => "safe_mode",
        }
    }
}

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
    /// Safe mode - severely restricted for untrusted external input (e.g., Twitter)
    /// Only allows Web tools with no exec, filesystem, or dangerous operations
    SafeMode,
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
            // SafeMode: Only Web tools - no exec, filesystem, finance, or any dangerous operations
            // Used for untrusted external input like Twitter mentions
            ToolProfile::SafeMode => vec![ToolGroup::Web],
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
            "safemode" | "safe_mode" | "safe" => Some(ToolProfile::SafeMode),
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
    /// Hidden tools are excluded from normal tool lists.
    /// They can only be activated when a skill declares them in `requires_tools`.
    #[serde(skip)]
    pub hidden: bool,
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
    pub output_type: ChannelOutputType,
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
    /// Context bank for key terms extracted from user input
    /// Contains ETH addresses, token symbols, etc. found in the original query
    pub context_bank: crate::tools::ContextBank,
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
    /// Currently selected network from the UI (e.g., "base", "polygon", "mainnet")
    /// Web3 tools should use this as default unless user explicitly specifies otherwise
    pub selected_network: Option<String>,
    /// QMD Memory store for markdown-based memory system
    pub memory_store: Option<Arc<MemoryStore>>,
    /// Wallet provider for signing transactions (Standard or Flash mode)
    pub wallet_provider: Option<Arc<dyn WalletProvider>>,
    /// Platform-specific chat/conversation ID (e.g., Telegram chat_id)
    /// Allows tools to query data by chat without going through sessions
    pub platform_chat_id: Option<String>,
    /// Runtime API key store (interior-mutable so install_api_key can write via &self)
    /// Keys are stored as UPPER_SNAKE_CASE names → values
    pub api_keys: Arc<RwLock<HashMap<String, String>>>,
    /// Optional HTTP proxy URL for tool requests (does not affect AI model API calls)
    pub proxy_url: Option<String>,
    /// Pre-built HTTP client configured with the proxy (if proxy_url is set)
    pub tool_http_client: Option<reqwest::Client>,
    /// Disk quota manager for enforcing disk usage limits
    pub disk_quota: Option<Arc<DiskQuotaManager>>,
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
            .field("context_bank", &self.context_bank.len())
            .field("database", &self.database.is_some())
            .field("subagent_manager", &self.subagent_manager.is_some())
            .field("process_manager", &self.process_manager.is_some())
            .field("skill_registry", &self.skill_registry.is_some())
            .field("tx_queue", &self.tx_queue.is_some())
            .field("selected_network", &self.selected_network)
            .field("memory_store", &self.memory_store.is_some())
            .field("wallet_provider", &self.wallet_provider.is_some())
            .field("platform_chat_id", &self.platform_chat_id)
            .field("api_keys", &self.api_keys.read().ok().map(|m| m.len()))
            .field("proxy_url", &self.proxy_url)
            .field("tool_http_client", &self.tool_http_client.is_some())
            .field("disk_quota", &self.disk_quota.is_some())
            .finish()
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        ToolContext {
            channel_id: None,
            channel_type: None,
            output_type: ChannelOutputType::TextOnly,
            user_id: None,
            session_id: None,
            identity_id: None,
            workspace_dir: None,
            extra: HashMap::new(),
            broadcaster: None,
            registers: RegisterStore::new(),
            context_bank: crate::tools::ContextBank::new(),
            database: None,
            subagent_manager: None,
            process_manager: None,
            skill_registry: None,
            tx_queue: None,
            selected_network: None,
            memory_store: None,
            wallet_provider: None,
            platform_chat_id: None,
            api_keys: Arc::new(RwLock::new(HashMap::new())),
            proxy_url: None,
            tool_http_client: None,
            disk_quota: None,
        }
    }
}

impl ToolContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_channel(mut self, channel_id: i64, channel_type: String) -> Self {
        self.channel_id = Some(channel_id);
        self.output_type = match channel_type.as_str() {
            "web" => ChannelOutputType::RichHtml,
            _ => ChannelOutputType::TextOnly,
        };
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

    pub fn with_platform_chat_id(mut self, chat_id: String) -> Self {
        self.platform_chat_id = Some(chat_id);
        self
    }

    /// Add an API key to the context by string name (for backwards compatibility)
    /// Keys are stored by their exact name (e.g., "GITHUB_TOKEN", "MOLTX_API_KEY")
    pub fn with_api_key(mut self, key_name: &str, key_value: String) -> Self {
        // Write to extra for backward compat
        self.extra.insert(
            format!("api_key_{}", key_name),
            serde_json::json!(key_value),
        );
        // Also write to the api_keys store
        if let Ok(mut store) = self.api_keys.write() {
            store.insert(key_name.to_string(), key_value);
        }
        self
    }

    /// Add an API key to the context using the type-safe ApiKeyId enum
    pub fn with_api_key_id(self, key_id: ApiKeyId, key_value: String) -> Self {
        self.with_api_key(key_id.as_str(), key_value)
    }

    /// Get an API key from the context by its exact string name
    /// Example: get_api_key("GITHUB_TOKEN")
    /// Checks the api_keys store first, falls back to extra
    pub fn get_api_key(&self, key_name: &str) -> Option<String> {
        // Check api_keys store first
        if let Ok(store) = self.api_keys.read() {
            if let Some(val) = store.get(key_name) {
                if !val.is_empty() {
                    return Some(val.clone());
                }
            }
        }
        // Fall back to extra
        self.extra.get(&format!("api_key_{}", key_name))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get an API key from the context using the type-safe ApiKeyId enum
    /// This is the preferred method as it prevents typos in key names.
    /// Falls back to legacy key names for backward compatibility after renames.
    pub fn get_api_key_by_id(&self, key_id: ApiKeyId) -> Option<String> {
        self.get_api_key(key_id.as_str()).or_else(|| {
            key_id.legacy_name().and_then(|legacy| self.get_api_key(legacy))
        })
    }

    /// Install an API key at runtime (takes &self, writes via RwLock)
    /// Used by the install_api_key tool to inject keys into the current session
    pub fn install_api_key_runtime(&self, key_name: &str, key_value: String) {
        if let Ok(mut store) = self.api_keys.write() {
            store.insert(key_name.to_string(), key_value);
        }
    }

    /// List all API key names currently in the runtime store
    pub fn list_api_key_names(&self) -> Vec<String> {
        self.api_keys
            .read()
            .ok()
            .map(|store| store.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Find a bot token from channel settings for a given channel type.
    /// First checks the current channel (if it matches the type), then falls back to any channel of that type.
    /// `setting_key` is the channel setting name (e.g. "discord_bot_token", "telegram_bot_token", "slack_bot_token").
    pub fn find_channel_bot_token(&self, channel_type: &str, setting_key: &str) -> Option<String> {
        let db = self.database.as_ref()?;

        // If we're currently in a channel of the right type, use its token
        if let Some(channel_id) = self.channel_id {
            if let Ok(Some(token)) = db.get_channel_setting(channel_id, setting_key) {
                if !token.is_empty() {
                    return Some(token);
                }
            }
        }

        // Fall back: find any channel of this type and use its token
        if let Ok(channels) = db.list_channels() {
            for ch in channels {
                if ch.channel_type == channel_type {
                    if let Ok(Some(token)) = db.get_channel_setting(ch.id, setting_key) {
                        if !token.is_empty() {
                            return Some(token);
                        }
                    }
                    // Also check legacy bot_token field
                    if !ch.bot_token.is_empty() {
                        return Some(ch.bot_token);
                    }
                }
            }
        }

        None
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

    /// Set the selected network from the UI (for web3 tools to use as default)
    pub fn with_selected_network(mut self, network: Option<String>) -> Self {
        self.selected_network = network;
        self
    }

    /// Add a MemoryStore to the context (for QMD memory tools)
    pub fn with_memory_store(mut self, store: Arc<MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    /// Add a DiskQuotaManager to the context (for enforcing disk usage limits)
    pub fn with_disk_quota(mut self, dq: Arc<DiskQuotaManager>) -> Self {
        self.disk_quota = Some(dq);
        self
    }

    /// Check disk quota before a write. Returns Ok(()) or a human-readable error string.
    pub fn check_disk_quota(&self, bytes: usize) -> Result<(), String> {
        if let Some(ref dq) = self.disk_quota {
            dq.check_quota(bytes as u64).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    /// Record a successful write with the disk quota manager.
    pub fn record_disk_write(&self, bytes: usize) {
        if let Some(ref dq) = self.disk_quota {
            dq.record_write(bytes as u64);
        }
    }

    /// Set an HTTP proxy URL for tool requests. Builds a proxy-configured HTTP client.
    /// Does not affect AI model API calls (those use the global shared client directly).
    pub fn with_proxy_url(mut self, url: String) -> Self {
        match crate::http::build_proxy_client(&url) {
            Ok(client) => {
                log::info!("Tool HTTP proxy configured: {}", url);
                self.tool_http_client = Some(client);
                self.proxy_url = Some(url);
            }
            Err(e) => {
                log::error!("Failed to build proxy client for '{}': {}. Tools will connect directly.", url, e);
            }
        }
        self
    }

    /// Returns an HTTP client for tool use. If a proxy is configured, returns the proxy client;
    /// otherwise falls back to the global shared client.
    pub fn http_client(&self) -> reqwest::Client {
        if let Some(ref client) = self.tool_http_client {
            client.clone()
        } else {
            crate::http::shared_client().clone()
        }
    }

    /// Add a WalletProvider to the context (for x402 payments in Flash mode)
    /// Also pre-populates the wallet_address register for tools that need it
    pub fn with_wallet_provider(mut self, wallet_provider: Arc<dyn WalletProvider>) -> Self {
        // Pre-populate wallet_address register so tools don't need to compute it
        let wallet_address = wallet_provider.get_address();
        self.registers.set("wallet_address", serde_json::json!(wallet_address), "wallet_provider");

        self.wallet_provider = Some(wallet_provider);
        self
    }

    /// Populate context bank with extracted terms from user input and broadcast update
    pub fn scan_and_set_context_bank(&mut self, text: &str) {
        let items = crate::tools::scan_input(text);
        if !items.is_empty() {
            self.context_bank.add_all(items);

            // Broadcast context bank update if we have a broadcaster
            if let (Some(broadcaster), Some(channel_id)) = (&self.broadcaster, self.channel_id) {
                broadcaster.broadcast(GatewayEvent::context_bank_update(
                    channel_id,
                    self.context_bank.to_json(),
                ));
            }
        }
    }

    /// Get the context bank formatted for agent context
    pub fn get_context_bank_for_agent(&self) -> Option<String> {
        self.context_bank.format_for_agent()
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

/// The definitive list of tools allowed in safe mode.
/// This is the ONLY place safe mode tool permissions are defined.
/// Safe mode is used for untrusted input (non-admin Telegram users, Twitter mentions, etc.)
/// SECURITY: Adding tools here grants them to ALL untrusted users. Be extremely careful.
pub const SAFE_MODE_ALLOW_LIST: &[&str] = &[
    "set_agent_subtype",    // Changes agent mode per-session (safe, no persistence)
    "token_lookup",         // Read-only token info lookup (safe)
    "say_to_user",          // Send message to user (safe)
    "task_fully_completed", // Mark task done (safe)
    "define_tasks",         // Organize tasks into queue (safe, no side effects)
    "memory_read",          // Read-only memory retrieval (sandboxed to safemode/ in safe mode)
    "memory_search",        // Read-only memory search (sandboxed to safemode/ in safe mode)
    "discord_read",         // Read-only Discord operations (safe)
    "discord_lookup",       // Read-only Discord server/channel lookup (safe)
    "telegram_read",        // Read-only Telegram operations (safe)
];

/// Tools whose sessions must NEVER be written to memory files.
/// SECURITY: Prevents API keys and secrets from persisting in memory markdown files.
pub const MEMORY_EXCLUDE_TOOL_LIST: &[&str] = &[
    "install_api_key",
    "api_keys_check",
];

pub fn is_memory_excluded_tool(tool_name: &str) -> bool {
    MEMORY_EXCLUDE_TOOL_LIST.contains(&tool_name)
}

impl ToolConfig {
    /// Create a safe mode tool config.
    /// This is the ONLY way to create a safe mode config - enforced at the type level.
    /// Only Web group tools + the explicit SAFE_MODE_ALLOW_LIST tools are permitted.
    pub fn safe_mode() -> Self {
        ToolConfig {
            id: None,
            channel_id: None,
            profile: ToolProfile::SafeMode,
            allow_list: SAFE_MODE_ALLOW_LIST.iter().map(|s| s.to_string()).collect(),
            deny_list: vec![],
            allowed_groups: vec!["web".to_string()],
            denied_groups: vec![],
        }
    }

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
