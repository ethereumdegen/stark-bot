//! Built-in tools for the agent
//!
//! Tools are organized into submodules by category:
//! - `bash`: Shell operations and filesystem tools (grep, glob, exec, git, file ops)
//! - `code`: Development tools (committer, deploy, pr_quality)
//! - `core`: Essential agent tools (ask_user, subagent, task management)
//! - `cryptocurrency`: Web3, x402, and blockchain tools
//! - `social_media`: Platform integrations (Twitter, Discord, GitHub)

// Submodules
pub mod bash;
pub mod code;
pub mod core;
pub mod cryptocurrency;
pub mod social_media;

// Individual tools (remaining uncategorized)
mod local_rpc;
mod memory_associate;
mod memory_graph;
mod process_status;
mod qmd_memory_read;
mod qmd_memory_search;
mod web_fetch;

// Re-exports from submodules
pub use bash::{
    ApplyPatchTool, ClaudeCodeRemoteTool, DeleteFileTool, EditFileTool, ExecTool, GitTool,
    GlobTool, GrepTool, ListFilesTool, ReadFileTool, ReadSymbolTool, RenameFileTool, WriteFileTool,
};
pub use code::{CommitterTool, DeployTool, IndexProjectTool, PrQualityTool, VerifyChangesTool};
pub use core::{
    AddTaskTool, BranchTool, DefineTasksTool, AgentSendTool, ApiKeysCheckTool, AskUserTool, HeartbeatConfigTool,
    ImportIdentityTool, InstallApiKeyTool, ManageModulesTool, ManageSkillsTool, MindmapManageTool,
    ReadSkillTool, RegisterNewIdentityTool, WorkstreamTool, ModifySoulTool, ModifySpecialRoleTool, SayToUserTool,
    SetAgentSubtypeTool, SubagentStatusTool, SpawnSubagentsTool, TaskFullyCompletedTool, UseSkillTool,
    // Meta tools (self-management)
    CloudBackupTool, ManageGatewayChannelsTool, ReadOperatingModeTool, ReadRecentTransactionsTool,
    SetThemeAccentTool,
};
pub use cryptocurrency::{
    load_networks, load_tokens, BridgeUsdcTool, BroadcastWeb3TxTool, DecodeCalldataTool,
    DexScreenerTool, Erc8128FetchTool, GeckoTerminalTool, ListQueuedWeb3TxTool, PolymarketTradeTool,
    SelectWeb3NetworkTool, SendEthTool, SetAddressTool, SiwaAuthTool, ToRawAmountTool, TokenLookupTool,
    VerifyTxBroadcastTool, Web3PresetFunctionCallTool, X402AgentInvokeTool, X402FetchTool,
    X402PostTool, X402RpcTool,
};
pub use social_media::{DiscordLookupTool, DiscordReadTool, DiscordWriteTool, FigmaTool, GithubUserTool, TelegramReadTool, TwitterPostTool};

// Re-exports from individual tools
pub use local_rpc::LocalRpcTool;
pub use memory_associate::MemoryAssociateTool;
pub use memory_graph::MemoryGraphTool;
pub use process_status::ProcessStatusTool;
pub use qmd_memory_read::QmdMemoryReadTool;
pub use qmd_memory_search::QmdMemorySearchTool;
pub use web_fetch::WebFetchTool;
