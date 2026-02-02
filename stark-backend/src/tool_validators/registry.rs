//! Registry for managing tool validators

use std::sync::Arc;
use super::traits::ToolValidator;
use super::types::{ValidationContext, ValidationResult};

/// Registry that holds all tool validators
pub struct ValidatorRegistry {
    validators: Vec<Arc<dyn ToolValidator>>,
}

impl ValidatorRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    /// Register a validator
    ///
    /// Validators are automatically sorted by priority after registration.
    pub fn register(&mut self, validator: Arc<dyn ToolValidator>) {
        let id = validator.id().to_string();
        let name = validator.name().to_string();
        let priority = validator.priority();

        self.validators.push(validator);

        // Sort by priority (lower = first)
        self.validators.sort_by_key(|v| v.priority() as u32);

        log::info!(
            "[VALIDATOR_REGISTRY] Registered validator '{}' ({}) with priority {:?}",
            id,
            name,
            priority
        );
    }

    /// Run all applicable validators against a tool call
    ///
    /// Validators are run in priority order. The first validator to return
    /// a Block result will stop execution and return that result.
    pub async fn validate(&self, ctx: &ValidationContext) -> ValidationResult {
        for validator in &self.validators {
            // Skip disabled validators
            if !validator.enabled() {
                continue;
            }

            // Skip if validator doesn't apply to this tool
            if let Some(tools) = validator.applies_to() {
                if !tools.contains(&ctx.tool_name.as_str()) {
                    continue;
                }
            }

            // Run the validation
            let result = validator.validate(ctx).await;

            if result.is_blocked() {
                log::info!(
                    "[VALIDATOR] '{}' blocked tool '{}': {}",
                    validator.id(),
                    ctx.tool_name,
                    result.block_reason().unwrap_or("unknown reason")
                );
                return result;
            }
        }

        ValidationResult::Allow
    }

    /// Get the number of registered validators
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }

    /// List all registered validators
    pub fn list(&self) -> Vec<&Arc<dyn ToolValidator>> {
        self.validators.iter().collect()
    }

    /// Get a validator by ID
    pub fn get(&self, id: &str) -> Option<&Arc<dyn ToolValidator>> {
        self.validators.iter().find(|v| v.id() == id)
    }
}

impl Default for ValidatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::types::ToolContext;
    use async_trait::async_trait;
    use serde_json::json;

    struct AlwaysAllowValidator;

    #[async_trait]
    impl ToolValidator for AlwaysAllowValidator {
        fn id(&self) -> &str { "always_allow" }
        fn name(&self) -> &str { "Always Allow" }
        fn applies_to(&self) -> Option<Vec<&str>> { None }

        async fn validate(&self, _ctx: &ValidationContext) -> ValidationResult {
            ValidationResult::Allow
        }
    }

    struct AlwaysBlockValidator;

    #[async_trait]
    impl ToolValidator for AlwaysBlockValidator {
        fn id(&self) -> &str { "always_block" }
        fn name(&self) -> &str { "Always Block" }
        fn applies_to(&self) -> Option<Vec<&str>> { Some(vec!["test_tool"]) }

        async fn validate(&self, _ctx: &ValidationContext) -> ValidationResult {
            ValidationResult::Block("Test block".into())
        }
    }

    #[tokio::test]
    async fn test_registry_allows_by_default() {
        let registry = ValidatorRegistry::new();
        let ctx = ValidationContext::new(
            "some_tool".into(),
            json!({}),
            Arc::new(ToolContext::new()),
        );

        let result = registry.validate(&ctx).await;
        assert!(result.is_allowed());
    }

    #[tokio::test]
    async fn test_registry_blocks_when_validator_blocks() {
        let mut registry = ValidatorRegistry::new();
        registry.register(Arc::new(AlwaysBlockValidator));

        let ctx = ValidationContext::new(
            "test_tool".into(),
            json!({}),
            Arc::new(ToolContext::new()),
        );

        let result = registry.validate(&ctx).await;
        assert!(result.is_blocked());
        assert_eq!(result.block_reason(), Some("Test block"));
    }

    #[tokio::test]
    async fn test_validator_only_applies_to_specified_tools() {
        let mut registry = ValidatorRegistry::new();
        registry.register(Arc::new(AlwaysBlockValidator));

        // Should block test_tool
        let ctx1 = ValidationContext::new(
            "test_tool".into(),
            json!({}),
            Arc::new(ToolContext::new()),
        );
        assert!(registry.validate(&ctx1).await.is_blocked());

        // Should allow other_tool (validator doesn't apply)
        let ctx2 = ValidationContext::new(
            "other_tool".into(),
            json!({}),
            Arc::new(ToolContext::new()),
        );
        assert!(registry.validate(&ctx2).await.is_allowed());
    }
}
