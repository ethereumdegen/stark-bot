use crate::ai::multi_agent::types::{AgentSubtype, get_subtype_config};
use crate::tools::types::{ToolConfig, ToolContext, ToolDefinition, ToolGroup, ToolProfile, ToolResult, ToolSafetyLevel};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait that all tools must implement
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the tool definition for the AI API
    fn definition(&self) -> ToolDefinition;

    /// Executes the tool with the given parameters
    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult;

    /// Returns the tool's name
    fn name(&self) -> String {
        self.definition().name.clone()
    }

    /// Returns the tool's group for access control
    fn group(&self) -> ToolGroup {
        self.definition().group
    }

    /// The safety level of this tool — determines availability in restricted contexts.
    /// Defaults to Standard (only normal mode). Override to ReadOnly or SafeMode to
    /// make the tool available in those restricted contexts.
    /// New tools default to Standard so they can't accidentally leak into restricted modes.
    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Standard
    }
}

/// Registry that holds all available tools.
/// Uses interior mutability (RwLock) so tools can be registered/unregistered
/// at runtime without requiring &mut self (enables module hot-reload).
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    default_config: ToolConfig,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry {
            tools: RwLock::new(HashMap::new()),
            default_config: ToolConfig::default(),
        }
    }

    pub fn with_config(config: ToolConfig) -> Self {
        ToolRegistry {
            tools: RwLock::new(HashMap::new()),
            default_config: config,
        }
    }

    /// Register a tool (thread-safe, takes &self via interior mutability)
    pub fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.definition().name.clone();
        self.tools.write().insert(name, tool);
    }

    /// Unregister a tool by name. Returns true if it was present.
    pub fn unregister(&self, name: &str) -> bool {
        self.tools.write().remove(name).is_some()
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    /// List all registered tools
    pub fn list(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.read().values().cloned().collect()
    }

    /// Get tools at or above a minimum safety level, filtered by config.
    /// - ReadOnly: tools with safety_level >= ReadOnly (ReadOnly + SafeMode)
    /// - SafeMode: tools with safety_level >= SafeMode (SafeMode only)
    pub fn get_tools_at_safety_level(&self, config: &ToolConfig, min_level: ToolSafetyLevel) -> Vec<Arc<dyn Tool>> {
        self.tools
            .read()
            .values()
            .filter(|tool| {
                tool.safety_level() >= min_level
                    && config.is_tool_allowed(&tool.definition().name, tool.group())
            })
            .cloned()
            .collect()
    }

    /// Get tool definitions at or above a minimum safety level, filtered by config.
    pub fn get_tool_definitions_at_safety_level(&self, config: &ToolConfig, min_level: ToolSafetyLevel) -> Vec<ToolDefinition> {
        self.get_tools_at_safety_level(config, min_level)
            .iter()
            .map(|tool| tool.definition())
            .collect()
    }

    /// Get tools that are allowed by a configuration
    pub fn get_additional_tools(&self, config: &ToolConfig) -> Vec<Arc<dyn Tool>> {
        self.tools
            .read()
            .values()
            .filter(|tool| {
                let def = tool.definition();
                // Hidden tools are skill-only — excluded from normal lists
                !def.hidden && config.is_tool_allowed(&def.name, tool.group())
            })
            .cloned()
            .collect()
    }

    /// Get tools that are allowed for a specific agent subtype.
    /// Tools come from two sources (additive):
    /// 1. `tool_groups` — tools whose group is in the subtype's allowed groups
    /// 2. `additional_tools` — explicitly named tools added on top (regardless of group)
    ///
    /// When `additional_tools` is set, the automatic System/Web/Filesystem injection
    /// is skipped (the subtype must declare everything it needs explicitly).
    /// When `additional_tools` is empty, System tools are auto-included for convenience.
    pub fn get_allowed_tools_for_subtype(
        &self,
        config: &ToolConfig,
        subtype: AgentSubtype,
    ) -> Vec<Arc<dyn Tool>> {
        let additional: Vec<String> = get_subtype_config(subtype.as_str())
            .map(|c| c.additional_tools)
            .unwrap_or_default();
        let has_additional = !additional.is_empty();

        let allowed_groups = subtype.allowed_tool_groups();
        self.tools
            .read()
            .values()
            .filter(|tool| {
                let def = tool.definition();
                if def.hidden {
                    return false;
                }
                if !config.is_tool_allowed(&def.name, tool.group()) {
                    return false;
                }

                let group = tool.group();

                // Tool is allowed if its group is in the subtype's allowed groups
                if allowed_groups.contains(&group) {
                    return true;
                }

                // Tool is allowed if it's explicitly named in additional_tools
                if additional.iter().any(|name| name == &def.name) {
                    return true;
                }

                // Auto-include System tools only when there's no explicit additional_tools
                if !has_additional && group == ToolGroup::System {
                    return true;
                }

                false
            })
            .cloned()
            .collect()
    }

    /// Get tool definitions for a specific agent subtype
    pub fn get_tool_definitions_for_subtype(
        &self,
        config: &ToolConfig,
        subtype: AgentSubtype,
    ) -> Vec<ToolDefinition> {
        self.get_allowed_tools_for_subtype(config, subtype)
            .iter()
            .map(|tool| tool.definition())
            .collect()
    }

    /// Get tool definitions for a specific agent subtype, with additional required tools
    /// that are force-included regardless of config/profile restrictions.
    /// Used when a skill is activated that requires specific tools.
    pub fn get_tool_definitions_for_subtype_with_required(
        &self,
        config: &ToolConfig,
        subtype: AgentSubtype,
        required_tools: &[String],
    ) -> Vec<ToolDefinition> {
        // Start with the normal subtype-allowed tools
        let mut tools = self.get_allowed_tools_for_subtype(config, subtype);
        let mut tool_names: std::collections::HashSet<String> =
            tools.iter().map(|t| t.definition().name.clone()).collect();

        // Force-include required tools even if they're not normally allowed by
        // subtype/profile restrictions. In safe mode, still respect restrictions
        // (skills cannot bypass safe mode). Outside safe mode, only the deny_list blocks.
        let is_safe_mode = config.profile == ToolProfile::SafeMode;
        for tool_name in required_tools {
            if !tool_names.contains(tool_name) {
                if let Some(tool) = self.get(tool_name) {
                    let should_include = if is_safe_mode {
                        // Safe mode: respect full config restrictions
                        config.is_tool_allowed(&tool.definition().name, tool.group())
                    } else {
                        // Normal mode: only check deny_list (bypass profile/group restrictions)
                        !config.deny_list.contains(&tool.definition().name)
                    };
                    if should_include {
                        log::info!(
                            "[REGISTRY] Force-including required tool '{}' for active skill",
                            tool_name
                        );
                        tools.push(tool);
                        tool_names.insert(tool_name.clone());
                    } else {
                        log::warn!(
                            "[REGISTRY] Skipping required tool '{}' - blocked by {} config",
                            tool_name,
                            if is_safe_mode { "safe mode" } else { "deny_list" }
                        );
                    }
                } else {
                    log::warn!(
                        "[REGISTRY] Required tool '{}' not found in registry",
                        tool_name
                    );
                }
            }
        }

        tools.iter().map(|tool| tool.definition()).collect()
    }

    /// Get tool definitions for allowed tools (for sending to AI)
    pub fn get_tool_definitions(&self, config: &ToolConfig) -> Vec<ToolDefinition> {
        self.get_additional_tools(config)
            .iter()
            .map(|tool| tool.definition())
            .collect()
    }

    /// Get tool definitions using default config
    pub fn get_default_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.get_tool_definitions(&self.default_config)
    }

    /// Execute a tool by name
    pub async fn execute(
        &self,
        name: &str,
        params: Value,
        context: &ToolContext,
        config: Option<&ToolConfig>,
    ) -> ToolResult {
        let effective_config = config.unwrap_or(&self.default_config);

        // Get the tool
        let tool = match self.get(name) {
            Some(t) => t,
            None => return ToolResult::error(format!("Tool '{}' not found", name)),
        };

        // Check if tool is allowed
        if !effective_config.is_tool_allowed(name, tool.group()) {
            return ToolResult::error(format!("Tool '{}' is not allowed", name));
        }

        // Execute the tool
        tool.execute(params, context).await
    }

    /// Get default configuration
    pub fn default_config(&self) -> &ToolConfig {
        &self.default_config
    }

    /// Set default configuration
    pub fn set_default_config(&mut self, config: ToolConfig) {
        self.default_config = config;
    }

    /// Check if a tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.read().contains_key(name)
    }

    /// Get count of registered tools
    pub fn len(&self) -> usize {
        self.tools.read().len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::types::{PropertySchema, ToolInputSchema};

    struct MockTool {
        definition: ToolDefinition,
    }

    impl MockTool {
        fn new(name: &str, group: ToolGroup) -> Self {
            MockTool {
                definition: ToolDefinition {
                    name: name.to_string(),
                    description: format!("Mock {} tool", name),
                    input_schema: ToolInputSchema::default(),
                    group,
                    hidden: false,
                },
            }
        }
    }

    #[async_trait]
    impl Tool for MockTool {
        fn definition(&self) -> ToolDefinition {
            self.definition.clone()
        }

        async fn execute(&self, _params: Value, _context: &ToolContext) -> ToolResult {
            ToolResult::success("mock result")
        }
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("test_tool", ToolGroup::Web));
        registry.register(tool);

        assert!(registry.has_tool("test_tool"));
        assert!(!registry.has_tool("nonexistent"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_tool_config_allows() {
        let config = ToolConfig {
            profile: crate::tools::types::ToolProfile::Standard,
            ..Default::default()
        };

        // Web, filesystem, and exec are allowed in Standard profile
        assert!(config.is_tool_allowed("web_fetch", ToolGroup::Web));
        assert!(config.is_tool_allowed("read_file", ToolGroup::Filesystem));
        assert!(config.is_tool_allowed("exec", ToolGroup::Exec));
        // Messaging is not allowed in Standard profile
        assert!(!config.is_tool_allowed("send_message", ToolGroup::Messaging));
    }

    #[test]
    fn test_tool_config_deny_list() {
        let config = ToolConfig {
            profile: crate::tools::types::ToolProfile::Full,
            deny_list: vec!["dangerous_tool".to_string()],
            ..Default::default()
        };

        // Denied tool should be blocked even with Full profile
        assert!(!config.is_tool_allowed("dangerous_tool", ToolGroup::System));
        // Other tools should be allowed
        assert!(config.is_tool_allowed("safe_tool", ToolGroup::System));
    }

    // =========================================================================
    // SAFE MODE ENFORCEMENT TESTS
    //
    // These tests prove that in safe mode, ONLY the explicitly allowed tools
    // can be used. No bypass is possible — not through skills, not through
    // group tricks, not through allow_list manipulation, not through anything.
    // =========================================================================

    /// Build a registry with one tool per group so we can test every group.
    fn build_all_groups_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        // One representative tool per group
        registry.register(Arc::new(MockTool::new("system_tool", ToolGroup::System)));
        registry.register(Arc::new(MockTool::new("web_fetch", ToolGroup::Web)));
        registry.register(Arc::new(MockTool::new("read_file", ToolGroup::Filesystem)));
        registry.register(Arc::new(MockTool::new("web3_tx", ToolGroup::Finance)));
        registry.register(Arc::new(MockTool::new("edit_file", ToolGroup::Development)));
        registry.register(Arc::new(MockTool::new("exec", ToolGroup::Exec)));
        registry.register(Arc::new(MockTool::new("twitter_post", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("agent_send", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("discord_write", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("moltx", ToolGroup::Social)));
        registry.register(Arc::new(MockTool::new("memory_write", ToolGroup::Memory)));
        // Also register the safe-mode allowed tools (from non-Web groups)
        registry.register(Arc::new(MockTool::new("say_to_user", ToolGroup::System)));
        registry.register(Arc::new(MockTool::new("set_agent_subtype", ToolGroup::System)));
        registry.register(Arc::new(MockTool::new("task_fully_completed", ToolGroup::System)));
        registry.register(Arc::new(MockTool::new("token_lookup", ToolGroup::Finance)));
        registry.register(Arc::new(MockTool::new("memory_read", ToolGroup::Memory)));
        registry.register(Arc::new(MockTool::new("memory_search", ToolGroup::Memory)));
        registry.register(Arc::new(MockTool::new("discord_read", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("discord_lookup", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("telegram_read", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("define_tasks", ToolGroup::System)));
        registry
    }

    #[test]
    fn test_safe_mode_blocks_twitter_post() {
        let config = ToolConfig::safe_mode();
        assert!(
            !config.is_tool_allowed("twitter_post", ToolGroup::Messaging),
            "twitter_post MUST be blocked in safe mode"
        );
    }

    #[test]
    fn test_safe_mode_blocks_every_dangerous_group() {
        let config = ToolConfig::safe_mode();

        // These must ALL be blocked
        assert!(!config.is_tool_allowed("exec", ToolGroup::Exec), "Exec tools must be blocked");
        assert!(!config.is_tool_allowed("edit_file", ToolGroup::Development), "Development tools must be blocked");
        assert!(!config.is_tool_allowed("read_file", ToolGroup::Filesystem), "Filesystem tools must be blocked");
        assert!(!config.is_tool_allowed("web3_tx", ToolGroup::Finance), "Finance tools must be blocked");
        assert!(!config.is_tool_allowed("agent_send", ToolGroup::Messaging), "Messaging tools must be blocked");
        assert!(!config.is_tool_allowed("discord_write", ToolGroup::Messaging), "discord_write must be blocked");
        assert!(!config.is_tool_allowed("moltx", ToolGroup::Social), "Social tools must be blocked");
        assert!(!config.is_tool_allowed("memory_write", ToolGroup::Memory), "Memory write tools must be blocked");
        assert!(!config.is_tool_allowed("system_tool", ToolGroup::System), "Arbitrary System tools must be blocked");
    }

    #[test]
    fn test_safe_mode_allows_only_allowlisted_tools() {
        let config = ToolConfig::safe_mode();

        // These are the ONLY non-Web tools that should be allowed
        for tool_name in crate::tools::types::SAFE_MODE_ALLOW_LIST {
            assert!(
                config.is_tool_allowed(tool_name, ToolGroup::System) // group doesn't matter for allow_list
                    || config.is_tool_allowed(tool_name, ToolGroup::Finance)
                    || config.is_tool_allowed(tool_name, ToolGroup::Messaging)
                    || config.is_tool_allowed(tool_name, ToolGroup::Memory),
                "Allow-listed tool '{}' should be allowed in safe mode",
                tool_name
            );
        }

        // Web group tools should also be allowed
        assert!(config.is_tool_allowed("web_fetch", ToolGroup::Web), "Web tools should be allowed");
    }

    #[test]
    fn test_safe_mode_get_additional_tools_excludes_dangerous() {
        let registry = build_all_groups_registry();
        let config = ToolConfig::safe_mode();
        let allowed = registry.get_additional_tools(&config);
        let allowed_names: Vec<String> = allowed.iter().map(|t| t.definition().name.clone()).collect();

        // twitter_post must NOT be in the list
        assert!(
            !allowed_names.contains(&"twitter_post".to_string()),
            "twitter_post must not appear in safe mode tool list, got: {:?}",
            allowed_names
        );
        // agent_send must NOT be in the list
        assert!(
            !allowed_names.contains(&"agent_send".to_string()),
            "agent_send must not appear in safe mode tool list"
        );
        // discord_write must NOT be in the list
        assert!(
            !allowed_names.contains(&"discord_write".to_string()),
            "discord_write must not appear in safe mode tool list"
        );
        // exec must NOT be in the list
        assert!(
            !allowed_names.contains(&"exec".to_string()),
            "exec must not appear in safe mode tool list"
        );
        // web3_tx must NOT be in the list
        assert!(
            !allowed_names.contains(&"web3_tx".to_string()),
            "web3_tx must not appear in safe mode tool list"
        );
        // edit_file must NOT be in the list
        assert!(
            !allowed_names.contains(&"edit_file".to_string()),
            "edit_file must not appear in safe mode tool list"
        );

        // But safe tools SHOULD be in the list
        assert!(allowed_names.contains(&"web_fetch".to_string()), "web_fetch should be allowed");
        assert!(allowed_names.contains(&"say_to_user".to_string()), "say_to_user should be allowed");
        assert!(allowed_names.contains(&"discord_read".to_string()), "discord_read should be allowed");
        assert!(allowed_names.contains(&"telegram_read".to_string()), "telegram_read should be allowed");
    }

    #[test]
    fn test_safe_mode_skill_force_include_cannot_bypass() {
        // Even when a skill requires twitter_post, it must NOT be included in safe mode
        let registry = build_all_groups_registry();
        let config = ToolConfig::safe_mode();

        let tools = registry.get_tool_definitions_for_subtype_with_required(
            &config,
            AgentSubtype::Secretary, // Secretary normally has Messaging access
            &["twitter_post".to_string(), "agent_send".to_string(), "exec".to_string()],
        );
        let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();

        assert!(
            !tool_names.contains(&"twitter_post".to_string()),
            "Skill requires_tools must NOT bypass safe mode for twitter_post, got: {:?}",
            tool_names
        );
        assert!(
            !tool_names.contains(&"agent_send".to_string()),
            "Skill requires_tools must NOT bypass safe mode for agent_send, got: {:?}",
            tool_names
        );
        assert!(
            !tool_names.contains(&"exec".to_string()),
            "Skill requires_tools must NOT bypass safe mode for exec, got: {:?}",
            tool_names
        );
    }

    #[tokio::test]
    async fn test_safe_mode_execute_blocks_twitter_post() {
        // Even if the AI somehow calls twitter_post, execution must be blocked
        let registry = build_all_groups_registry();
        let config = ToolConfig::safe_mode();
        let context = ToolContext::default();

        let result = registry.execute(
            "twitter_post",
            serde_json::json!({"text": "hacked tweet"}),
            &context,
            Some(&config),
        ).await;

        assert!(!result.success, "twitter_post execution must fail in safe mode");
        assert!(
            result.error.as_ref().unwrap().contains("not allowed"),
            "Error should say tool is not allowed, got: {:?}",
            result.error
        );
    }

    #[tokio::test]
    async fn test_safe_mode_execute_blocks_all_dangerous_tools() {
        let registry = build_all_groups_registry();
        let config = ToolConfig::safe_mode();
        let context = ToolContext::default();

        let dangerous_tools = vec![
            "twitter_post", "agent_send", "discord_write", "exec",
            "edit_file", "web3_tx", "read_file", "moltx", "memory_write",
            "system_tool",
        ];

        for tool_name in dangerous_tools {
            let result = registry.execute(
                tool_name,
                serde_json::json!({}),
                &context,
                Some(&config),
            ).await;

            assert!(
                !result.success,
                "Tool '{}' must be BLOCKED at execution time in safe mode, but it succeeded",
                tool_name
            );
        }
    }

    #[tokio::test]
    async fn test_safe_mode_execute_allows_safe_tools() {
        let registry = build_all_groups_registry();
        let config = ToolConfig::safe_mode();
        let context = ToolContext::default();

        // Allow-listed tools should execute successfully
        for tool_name in crate::tools::types::SAFE_MODE_ALLOW_LIST {
            let result = registry.execute(
                tool_name,
                serde_json::json!({}),
                &context,
                Some(&config),
            ).await;

            assert!(
                result.success,
                "Safe tool '{}' should be ALLOWED in safe mode, but was blocked: {:?}",
                tool_name, result.error
            );
        }

        // Web tools should also work
        let result = registry.execute(
            "web_fetch",
            serde_json::json!({}),
            &context,
            Some(&config),
        ).await;
        assert!(result.success, "web_fetch should be allowed in safe mode");
    }

    #[test]
    fn test_safe_mode_exhaustive_no_tool_from_real_registry_leaks() {
        // Use the REAL full registry with ALL registered tools
        let registry = crate::tools::create_default_registry();
        let config = ToolConfig::safe_mode();

        let allowed = registry.get_additional_tools(&config);
        let allowed_names: Vec<String> = allowed.iter().map(|t| t.definition().name.clone()).collect();

        // Every allowed tool must be EITHER in the allow list OR in the Web group
        for tool in &allowed {
            let name = tool.definition().name;
            let group = tool.group();
            let in_allow_list = crate::tools::types::SAFE_MODE_ALLOW_LIST.contains(&name.as_str());
            let is_web = group == ToolGroup::Web;

            assert!(
                in_allow_list || is_web,
                "Tool '{}' (group: {:?}) leaked through safe mode! It is not in SAFE_MODE_ALLOW_LIST and not a Web tool. Allowed tools: {:?}",
                name, group, allowed_names
            );
        }
    }

    #[test]
    fn test_safe_mode_specifically_blocks_twitter_post_in_real_registry() {
        // Use the REAL registry — the actual twitter_post tool with its actual group
        let registry = crate::tools::create_default_registry();
        let config = ToolConfig::safe_mode();

        let allowed = registry.get_additional_tools(&config);
        let allowed_names: Vec<String> = allowed.iter().map(|t| t.definition().name.clone()).collect();

        assert!(
            !allowed_names.contains(&"twitter_post".to_string()),
            "twitter_post MUST NOT appear in safe mode real registry tools. Got: {:?}",
            allowed_names
        );
    }

    // =========================================================================
    // SKILL FORCE-INCLUDE TESTS
    //
    // Skills can force-include tools that are outside the current subtype's
    // allowed groups. This is how discord_tipping (finance subtype) can use
    // discord_resolve_user (Messaging group). BUT safe mode always trumps:
    // skills CANNOT bypass safe mode restrictions.
    // =========================================================================

    #[test]
    fn test_skill_force_includes_tool_across_groups_in_normal_mode() {
        // Finance subtype doesn't include Messaging group, but a skill's
        // requires_tools should force-include a Messaging tool anyway.
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("token_lookup", ToolGroup::Finance)));
        registry.register(Arc::new(MockTool::new("discord_resolve_user", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("web3_preset", ToolGroup::Finance)));

        // Full profile (admin, not safe mode)
        let config = ToolConfig::default(); // Full profile

        // Without skill requires_tools — discord_resolve_user excluded by subtype
        let tools_no_skill = registry.get_tool_definitions_for_subtype(
            &config,
            AgentSubtype::Finance,
        );
        let names_no_skill: Vec<String> = tools_no_skill.iter().map(|t| t.name.clone()).collect();
        assert!(
            !names_no_skill.contains(&"discord_resolve_user".to_string()),
            "Without skill, Messaging tool should NOT be in Finance subtype, got: {:?}",
            names_no_skill
        );

        // With skill requires_tools — discord_resolve_user force-included
        let tools_with_skill = registry.get_tool_definitions_for_subtype_with_required(
            &config,
            AgentSubtype::Finance,
            &["discord_resolve_user".to_string()],
        );
        let names_with_skill: Vec<String> = tools_with_skill.iter().map(|t| t.name.clone()).collect();
        assert!(
            names_with_skill.contains(&"discord_resolve_user".to_string()),
            "Skill requires_tools MUST force-include discord_resolve_user in Finance subtype, got: {:?}",
            names_with_skill
        );
    }

    #[test]
    fn test_skill_force_includes_across_all_non_safe_profiles() {
        // Test that force-include works regardless of channel profile (Finance, Developer, etc.)
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("token_lookup", ToolGroup::Finance)));
        registry.register(Arc::new(MockTool::new("discord_resolve_user", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("exec", ToolGroup::Exec)));

        // Finance profile — doesn't include Messaging or Exec
        let finance_config = ToolConfig {
            profile: crate::tools::types::ToolProfile::Finance,
            ..Default::default()
        };
        let tools = registry.get_tool_definitions_for_subtype_with_required(
            &finance_config,
            AgentSubtype::Finance,
            &["discord_resolve_user".to_string(), "exec".to_string()],
        );
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        assert!(
            names.contains(&"discord_resolve_user".to_string()),
            "Finance profile + skill must include discord_resolve_user, got: {:?}",
            names
        );
        assert!(
            names.contains(&"exec".to_string()),
            "Finance profile + skill must include exec, got: {:?}",
            names
        );
    }

    #[test]
    fn test_skill_force_include_blocked_by_safe_mode() {
        // Safe mode ALWAYS trumps skill requires_tools
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("discord_resolve_user", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("web3_preset", ToolGroup::Finance)));
        registry.register(Arc::new(MockTool::new("broadcast_web3_tx", ToolGroup::Finance)));
        // Also add a safe-mode allowed tool for comparison
        registry.register(Arc::new(MockTool::new("say_to_user", ToolGroup::System)));
        registry.register(Arc::new(MockTool::new("token_lookup", ToolGroup::Finance)));

        let config = ToolConfig::safe_mode();

        let tools = registry.get_tool_definitions_for_subtype_with_required(
            &config,
            AgentSubtype::Finance,
            &[
                "discord_resolve_user".to_string(),
                "web3_preset".to_string(),
                "broadcast_web3_tx".to_string(),
                "say_to_user".to_string(),  // This IS in safe mode allow list
                "token_lookup".to_string(), // This IS in safe mode allow list
            ],
        );
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();

        // NOT in safe mode allow list — must be blocked
        assert!(
            !names.contains(&"discord_resolve_user".to_string()),
            "Safe mode must block discord_resolve_user even with skill requires_tools, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"web3_preset".to_string()),
            "Safe mode must block web3_preset even with skill requires_tools, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"broadcast_web3_tx".to_string()),
            "Safe mode must block broadcast_web3_tx even with skill requires_tools, got: {:?}",
            names
        );

        // IN safe mode allow list — should be included
        assert!(
            names.contains(&"say_to_user".to_string()),
            "Safe mode allow-listed tool should still work with skill, got: {:?}",
            names
        );
        assert!(
            names.contains(&"token_lookup".to_string()),
            "Safe mode allow-listed tool should still work with skill, got: {:?}",
            names
        );
    }

    #[test]
    fn test_skill_force_include_respects_deny_list() {
        // Even in normal mode, deny_list should block skill requires_tools
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("discord_resolve_user", ToolGroup::Messaging)));
        registry.register(Arc::new(MockTool::new("dangerous_tool", ToolGroup::Finance)));

        let config = ToolConfig {
            deny_list: vec!["dangerous_tool".to_string()],
            ..Default::default()
        };

        let tools = registry.get_tool_definitions_for_subtype_with_required(
            &config,
            AgentSubtype::Finance,
            &["discord_resolve_user".to_string(), "dangerous_tool".to_string()],
        );
        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();

        assert!(
            names.contains(&"discord_resolve_user".to_string()),
            "Non-denied skill tool should be included, got: {:?}",
            names
        );
        assert!(
            !names.contains(&"dangerous_tool".to_string()),
            "Deny-listed tool must be blocked even with skill requires_tools, got: {:?}",
            names
        );
    }

    #[test]
    fn test_safe_mode_config_is_immutable_from_channel_overrides() {
        // Prove that ToolConfig::safe_mode() produces a fixed config
        // regardless of what the channel had before
        let config = ToolConfig::safe_mode();

        // Profile must be SafeMode
        assert_eq!(config.profile, crate::tools::types::ToolProfile::SafeMode);
        // Deny list must be empty (so allow_list + profile are the only gates)
        assert!(config.deny_list.is_empty());
        // Allow list must match SAFE_MODE_ALLOW_LIST exactly
        let expected: Vec<String> = crate::tools::types::SAFE_MODE_ALLOW_LIST.iter().map(|s| s.to_string()).collect();
        assert_eq!(config.allow_list, expected);
        // Allowed groups must be only "web"
        assert_eq!(config.allowed_groups, vec!["web".to_string()]);
    }
}
