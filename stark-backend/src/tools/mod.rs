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
    PropertySchema, ToolConfig, ToolContext, ToolDefinition, ToolExecution, ToolGroup,
    ToolInputSchema, ToolProfile, ToolResult,
};

use std::sync::Arc;

/// Register all built-in tools to a registry
fn register_all_tools(registry: &mut ToolRegistry) {
    // System tools (always available)
    registry.register(Arc::new(builtin::SubagentTool::new()));
    registry.register(Arc::new(builtin::SubagentStatusTool::new()));
    registry.register(Arc::new(builtin::SetAgentSubtypeTool::new()));
    registry.register(Arc::new(builtin::AskUserTool::new()));
    registry.register(Arc::new(builtin::SayToUserTool::new()));
    registry.register(Arc::new(builtin::MultiMemorySearchTool::new()));
    // registry.register(Arc::new(builtin::MemoryGetTool::new())); // temporarily disabled
    registry.register(Arc::new(builtin::MemoryStoreTool::new()));
    registry.register(Arc::new(builtin::ModifySoulTool::new()));
    registry.register(Arc::new(builtin::ApiKeysCheckTool::new()));
    registry.register(Arc::new(builtin::TaskFullyCompletedTool::new()));
    registry.register(Arc::new(builtin::ManageSkillsTool::new()));

    // Web tools (shared)
    registry.register(Arc::new(builtin::WebFetchTool::new()));

    // Finance tools (crypto/DeFi operations)
    registry.register(Arc::new(builtin::X402RpcTool::new()));
    registry.register(Arc::new(builtin::X402FetchTool::new()));
    registry.register(Arc::new(builtin::X402AgentInvokeTool::new()));
    registry.register(Arc::new(builtin::X402PostTool::new()));
    registry.register(Arc::new(builtin::Web3TxTool::new()));
    registry.register(Arc::new(builtin::BroadcastWeb3TxTool::new()));
    registry.register(Arc::new(builtin::ListQueuedWeb3TxTool::new()));
    registry.register(Arc::new(builtin::Web3FunctionCallTool::new()));
    registry.register(Arc::new(builtin::TokenLookupTool::new()));
    registry.register(Arc::new(builtin::ToRawAmountTool::new()));
    registry.register(Arc::new(builtin::RegisterSetTool::new()));

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

    // Advanced development tools (scoped commits, deployment, PR quality)
    registry.register(Arc::new(builtin::CommitterTool::new()));
    registry.register(Arc::new(builtin::DeployTool::new()));
    registry.register(Arc::new(builtin::PrQualityTool::new()));

    // Exec tool (Development mode)
    registry.register(Arc::new(builtin::ExecTool::new()));

    // Messaging tools
    registry.register(Arc::new(builtin::AgentSendTool::new()));
    registry.register(Arc::new(builtin::DiscordLookupTool::new()));
    registry.register(Arc::new(builtin::TwitterPostTool::new()));

    // Discord hooks tools
    registry.register(Arc::new(crate::discord_hooks::tools::DiscordResolveUserTool::new()));
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
