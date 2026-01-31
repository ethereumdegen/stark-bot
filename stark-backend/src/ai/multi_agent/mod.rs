//! Multi-agent system for task handling
//!
//! This module implements an agent architecture with tool execution support.
//!
//! ## Tools
//!
//! - `add_note` - Track important observations during complex tasks
//!
//! ## Flow
//!
//! ```text
//! Request → Tool Execution → Response
//! ```
//!
//! The agent executes tools as needed and uses `add_note` to track
//! important information during multi-step tasks.

pub mod orchestrator;
pub mod subagent_manager;
pub mod tools;
pub mod types;

pub use orchestrator::{Orchestrator, ProcessResult};
pub use subagent_manager::SubAgentManager;
pub use types::{AgentContext, AgentMode, SubAgentContext, SubAgentStatus};
