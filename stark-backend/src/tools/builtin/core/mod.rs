//! Core agent tools
//!
//! Essential tools for agent operation, user interaction, and task management.

mod add_task;
mod define_tasks;
mod agent_send;
mod api_keys_check;
mod ask_user;
mod heartbeat_config;
mod import_identity;
mod install_api_key;
mod manage_modules;
mod manage_skills;
mod impulse_map_manage;
mod read_skill;
mod register_new_identity;
mod modify_kanban;
mod modify_soul;
mod modify_special_role;
mod say_to_user;
mod set_agent_subtype;
mod subagent;
mod use_skill;
mod task_complete;

// Meta tools (self-management)
mod cloud_backup;
mod manage_gateway_channels;
mod read_operating_mode;
mod read_recent_transactions;
mod set_theme_accent;

pub use add_task::AddTaskTool;
pub use define_tasks::DefineTasksTool;
pub use agent_send::AgentSendTool;
pub use api_keys_check::ApiKeysCheckTool;
pub use ask_user::AskUserTool;
pub use heartbeat_config::HeartbeatConfigTool;
pub use import_identity::ImportIdentityTool;
pub use install_api_key::InstallApiKeyTool;
pub use manage_modules::ManageModulesTool;
pub use manage_skills::ManageSkillsTool;
pub use impulse_map_manage::ImpulseMapManageTool;
pub use read_skill::ReadSkillTool;
pub use register_new_identity::RegisterNewIdentityTool;
pub use modify_kanban::WorkstreamTool;
pub use modify_soul::ModifySoulTool;
pub use modify_special_role::ModifySpecialRoleTool;
pub use say_to_user::SayToUserTool;
pub use set_agent_subtype::SetAgentSubtypeTool;
pub use subagent::{SubagentStatusTool, SpawnSubagentsTool};
pub use use_skill::UseSkillTool;
pub use task_complete::TaskFullyCompletedTool;

// Meta tools (self-management)
pub use cloud_backup::CloudBackupTool;
pub use manage_gateway_channels::ManageGatewayChannelsTool;
pub use read_operating_mode::ReadOperatingModeTool;
pub use read_recent_transactions::ReadRecentTransactionsTool;
pub use set_theme_accent::SetThemeAccentTool;
