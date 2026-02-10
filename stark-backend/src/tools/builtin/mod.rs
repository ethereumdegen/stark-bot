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
mod process_status;
mod qmd_memory_read;
mod qmd_memory_search;
mod web_fetch;

// Re-exports from submodules
pub use bash::{
    ApplyPatchTool, DeleteFileTool, EditFileTool, ExecTool, GitTool, GlobTool, GrepTool,
    ListFilesTool, ReadFileTool, RenameFileTool, WriteFileTool,
};
pub use code::{CommitterTool, DeployTool, PrQualityTool};
pub use core::{
    AddTaskTool, DefineTasksTool, AgentSendTool, ApiKeysCheckTool, AskUserTool, ImportIdentityTool,
    InstallApiKeyTool, ManageSkillsTool, ModifyIdentityTool, ModifySoulTool, SayToUserTool,
    SetAgentSubtypeTool, SubagentStatusTool, SubagentTool, TaskFullyCompletedTool,
};
pub use cryptocurrency::{
    load_networks, load_tokens, BridgeUsdcTool, BroadcastWeb3TxTool, DecodeCalldataTool,
    DexScreenerTool, Erc8128FetchTool, GeckoTerminalTool, ListQueuedWeb3TxTool, PolymarketTradeTool,
    SelectWeb3NetworkTool, SendEthTool, SetAddressTool, ToRawAmountTool, TokenLookupTool,
    VerifyTxBroadcastTool, Web3PresetFunctionCallTool, X402AgentInvokeTool, X402FetchTool,
    X402PostTool, X402RpcTool,
};
pub use social_media::{DiscordLookupTool, DiscordReadTool, DiscordWriteTool, GithubUserTool, TelegramReadTool, TwitterPostTool};

// Re-exports from individual tools
pub use process_status::ProcessStatusTool;
pub use qmd_memory_read::QmdMemoryReadTool;
pub use qmd_memory_search::QmdMemorySearchTool;
pub use web_fetch::WebFetchTool;
