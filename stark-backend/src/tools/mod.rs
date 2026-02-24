pub mod builtin;
pub mod context_bank;
pub mod http_retry;
pub mod presets;
pub mod register;
pub mod registry;
pub mod rpc_config;
pub mod types;

pub use context_bank::{scan_input, ContextBank, ContextBankItem};
pub use register::{PresetOrCustom, RegisterStore};
pub use registry::{Tool, ToolRegistry};
pub use types::{
    ChannelOutputType, PropertySchema, ToolConfig, ToolContext, ToolDefinition, ToolExecution,
    ToolGroup, ToolInputSchema, ToolProfile, ToolResult, ToolSafetyLevel, SAFE_MODE_ALLOW_LIST,
};

use std::sync::Arc;

/// Register all built-in tools to a registry
fn register_all_tools(registry: &mut ToolRegistry) {
    // System tools (always available)
    registry.register(Arc::new(builtin::SpawnSubagentsTool::new()));
    registry.register(Arc::new(builtin::SubagentStatusTool::new()));
    registry.register(Arc::new(builtin::SetAgentSubtypeTool::new()));
    registry.register(Arc::new(builtin::UseSkillTool::new()));
    registry.register(Arc::new(builtin::AskUserTool::new()));
    registry.register(Arc::new(builtin::SayToUserTool::new()));
    // Memory tools (DB-backed unified memory system)
    registry.register(Arc::new(builtin::MemorySearchTool::new()));
    registry.register(Arc::new(builtin::MemoryReadTool::new()));
    // Memory graph tools (associations + knowledge graph)
    registry.register(Arc::new(builtin::MemoryAssociateTool::new()));
    registry.register(Arc::new(builtin::MemoryGraphTool::new()));
    registry.register(Arc::new(builtin::MemoryMergeTool::new()));
    // Notes tool (Obsidian-compatible notes with FTS5)
    registry.register(Arc::new(builtin::NotesTool::new()));
    registry.register(Arc::new(builtin::ModifySoulTool::new()));
    registry.register(Arc::new(builtin::RegisterNewIdentityTool::new()));
    registry.register(Arc::new(builtin::ImportIdentityTool::new()));
    registry.register(Arc::new(builtin::ApiKeysCheckTool::new()));
    registry.register(Arc::new(builtin::TaskFullyCompletedTool::new()));
    registry.register(Arc::new(builtin::AddTaskTool::new()));
    registry.register(Arc::new(builtin::DefineTasksTool::new()));
    registry.register(Arc::new(builtin::ManageSkillsTool::new()));
    registry.register(Arc::new(builtin::ReadSkillTool::new()));
    registry.register(Arc::new(builtin::ManageModulesTool::new()));
    registry.register(Arc::new(builtin::WorkstreamTool::new()));
    registry.register(Arc::new(builtin::InstallApiKeyTool::new()));
    registry.register(Arc::new(builtin::HeartbeatConfigTool::new()));
    registry.register(Arc::new(builtin::ImpulseMapManageTool::new()));

    // Special roles (enriched safe mode management)
    registry.register(Arc::new(builtin::ModifySpecialRoleTool::new()));

    // Meta tools (self-management)
    registry.register(Arc::new(builtin::CloudBackupTool::new()));
    registry.register(Arc::new(builtin::SetThemeAccentTool::new()));
    registry.register(Arc::new(builtin::ReadOperatingModeTool::new()));
    registry.register(Arc::new(builtin::ReadRecentTransactionsTool::new()));
    registry.register(Arc::new(builtin::ManageGatewayChannelsTool::new()));

    // Web tools (shared)
    registry.register(Arc::new(builtin::WebFetchTool::new()));
    // Local RPC — localhost-only HTTP for microservice APIs
    registry.register(Arc::new(builtin::LocalRpcTool::new()));

    // Finance tools (crypto/DeFi operations)
    registry.register(Arc::new(builtin::X402RpcTool::new()));
    registry.register(Arc::new(builtin::X402FetchTool::new()));
    registry.register(Arc::new(builtin::X402AgentInvokeTool::new()));
    registry.register(Arc::new(builtin::X402PostTool::new()));
    // send_eth for simple native ETH transfers (no ABI needed)
    registry.register(Arc::new(builtin::SendEthTool::new()));
    registry.register(Arc::new(builtin::BroadcastWeb3TxTool::new()));
    registry.register(Arc::new(builtin::ListQueuedWeb3TxTool::new()));
    registry.register(Arc::new(builtin::Web3PresetFunctionCallTool::new()));
    registry.register(Arc::new(builtin::DecodeCalldataTool::new()));
    registry.register(Arc::new(builtin::TokenLookupTool::new()));
    registry.register(Arc::new(builtin::ToRawAmountTool::new()));
    // Composite swap tool (token lookup + allowance + quote + execute in one call)
    registry.register(Arc::new(builtin::SwapTokenTool::new()));
    registry.register(Arc::new(builtin::SetAddressTool::new()));
    // Post-broadcast transaction verification (AI-based)
    registry.register(Arc::new(builtin::VerifyTxBroadcastTool::new()));
    // Network selection for chain-specific operations
    registry.register(Arc::new(builtin::SelectWeb3NetworkTool::new()));
    // Cross-chain USDC bridging via Across Protocol
    registry.register(Arc::new(builtin::BridgeUsdcTool::new()));
    // ERC-8128 signed HTTP requests (Ethereum identity)
    registry.register(Arc::new(builtin::Erc8128FetchTool::new()));
    // SIWA/SIWE authentication (Sign In With Agent/Ethereum)
    registry.register(Arc::new(builtin::SiwaAuthTool::new()));

    // Filesystem tools (read-only, shared)
    registry.register(Arc::new(builtin::ReadFileTool::new()));
    registry.register(Arc::new(builtin::ListFilesTool::new()));

    // Development tools (code editing, git, search)
    registry.register(Arc::new(builtin::WriteFileTool::new()));
    registry.register(Arc::new(builtin::ApplyPatchTool::new()));
    registry.register(Arc::new(builtin::EditFileTool::new()));
    registry.register(Arc::new(builtin::DeleteFileTool::new()));
    registry.register(Arc::new(builtin::RenameFileTool::new()));
    registry.register(Arc::new(builtin::GrepTool::new()));
    registry.register(Arc::new(builtin::GlobTool::new()));
    registry.register(Arc::new(builtin::GitTool::new()));
    registry.register(Arc::new(builtin::GithubUserTool::new()));
    registry.register(Arc::new(builtin::ReadSymbolTool::new()));

    // Advanced development tools (scoped commits, deployment, PR quality)
    registry.register(Arc::new(builtin::CommitterTool::new()));
    registry.register(Arc::new(builtin::DeployTool::new()));
    registry.register(Arc::new(builtin::PrQualityTool::new()));
    // CodeEngineer boost: project indexing and verification
    registry.register(Arc::new(builtin::VerifyChangesTool::new()));
    registry.register(Arc::new(builtin::IndexProjectTool::new()));

    // Exec tool (Development mode)
    registry.register(Arc::new(builtin::ExecTool::new()));
    // Skill script execution (hidden — only available when a skill declares requires_tools)
    registry.register(Arc::new(builtin::RunSkillScriptTool::new()));
    // Claude Code Remote — SSH into remote machine running Claude Code CLI
    registry.register(Arc::new(builtin::ClaudeCodeRemoteTool::new()));

    // Messaging tools
    registry.register(Arc::new(builtin::AgentSendTool::new()));
    registry.register(Arc::new(builtin::DiscordReadTool::new()));
    registry.register(Arc::new(builtin::DiscordWriteTool::new()));
    registry.register(Arc::new(builtin::DiscordLookupTool::new()));
    registry.register(Arc::new(builtin::TwitterPostTool::new()));
    registry.register(Arc::new(builtin::TelegramReadTool::new()));
    registry.register(Arc::new(builtin::TelegramWriteTool::new()));

    // Design tools
    registry.register(Arc::new(builtin::FigmaTool::new()));

    // NOTE: discord_resolve_user is registered via module system (see main.rs).
    // All built-in module tools are registered unconditionally at startup.
}

/// Create a new ToolRegistry with all built-in tools registered
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    registry
}

/// Create a registry with specific configuration
pub fn create_registry_with_config(config: ToolConfig) -> ToolRegistry {
    let mut registry = ToolRegistry::with_config(config);
    register_all_tools(&mut registry);
    registry
}
