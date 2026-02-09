//! Core agent tools
//!
//! Essential tools for agent operation, user interaction, and task management.

mod add_task;
mod define_tasks;
mod agent_send;
mod api_keys_check;
mod ask_user;
mod import_identity;
mod install_api_key;
mod manage_skills;
mod modify_identity;
mod modify_soul;
mod say_to_user;
mod set_agent_subtype;
mod subagent;
mod task_complete;

pub use add_task::AddTaskTool;
pub use define_tasks::DefineTasksTool;
pub use agent_send::AgentSendTool;
pub use api_keys_check::ApiKeysCheckTool;
pub use ask_user::AskUserTool;
pub use import_identity::ImportIdentityTool;
pub use install_api_key::InstallApiKeyTool;
pub use manage_skills::ManageSkillsTool;
pub use modify_identity::ModifyIdentityTool;
pub use modify_soul::ModifySoulTool;
pub use say_to_user::SayToUserTool;
pub use set_agent_subtype::SetAgentSubtypeTool;
pub use subagent::{SubagentStatusTool, SubagentTool};
pub use task_complete::TaskFullyCompletedTool;
