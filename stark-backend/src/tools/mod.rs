pub mod builtin;
pub mod registry;
pub mod types;

pub use registry::{Tool, ToolRegistry};
pub use types::{
    PropertySchema, ToolConfig, ToolContext, ToolDefinition, ToolExecution, ToolGroup,
    ToolInputSchema, ToolProfile, ToolResult,
};

use std::sync::Arc;

/// Create a new ToolRegistry with all built-in tools registered
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    // Register web tools
    registry.register(Arc::new(builtin::WebFetchTool::new()));

    // Register filesystem tools
    registry.register(Arc::new(builtin::ReadFileTool::new()));
    registry.register(Arc::new(builtin::WriteFileTool::new()));
    registry.register(Arc::new(builtin::ListFilesTool::new()));
    registry.register(Arc::new(builtin::ApplyPatchTool::new()));

    // Register exec tool
    registry.register(Arc::new(builtin::ExecTool::new()));

    // Register messaging tools
    registry.register(Arc::new(builtin::AgentSendTool::new()));

    // Register system tools (subagents)
    registry.register(Arc::new(builtin::SubagentTool::new()));
    registry.register(Arc::new(builtin::SubagentStatusTool::new()));

    registry
}

/// Create a registry with specific configuration
pub fn create_registry_with_config(config: ToolConfig) -> ToolRegistry {
    let mut registry = ToolRegistry::with_config(config);

    // Register web tools
    registry.register(Arc::new(builtin::WebFetchTool::new()));

    // Register filesystem tools
    registry.register(Arc::new(builtin::ReadFileTool::new()));
    registry.register(Arc::new(builtin::WriteFileTool::new()));
    registry.register(Arc::new(builtin::ListFilesTool::new()));
    registry.register(Arc::new(builtin::ApplyPatchTool::new()));

    // Register exec tool
    registry.register(Arc::new(builtin::ExecTool::new()));

    // Register messaging tools
    registry.register(Arc::new(builtin::AgentSendTool::new()));

    // Register system tools (subagents)
    registry.register(Arc::new(builtin::SubagentTool::new()));
    registry.register(Arc::new(builtin::SubagentStatusTool::new()));

    registry
}
