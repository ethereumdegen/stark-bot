//! RON-based validator implementation
//!
//! This module provides the RonValidator struct that loads from `.ron` files
//! and implements the ToolValidator trait.

use std::path::Path;
use std::sync::Arc;
use async_trait::async_trait;

use crate::tool_validators::{
    ToolValidator, ValidationContext, ValidationResult, ValidatorPriority, ValidatorRegistry,
};
use super::schema::{Action, Priority, ValidatorDef};

/// A validator loaded from a RON file
pub struct RonValidator {
    def: ValidatorDef,
}

impl RonValidator {
    /// Load a validator from a RON file
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        Self::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
    }

    /// Parse a validator from a RON string
    pub fn from_str(content: &str) -> Result<Self, String> {
        // Use RON options that allow implicit Some() for Option fields
        let options = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let def: ValidatorDef = options.from_str(content)
            .map_err(|e| format!("Failed to parse RON: {}", e))?;
        Ok(Self { def })
    }
}

impl Action {
    /// Convert a RON Action to a ValidationResult
    fn into_result(self) -> ValidationResult {
        match self {
            Action::Allow => ValidationResult::Allow,
            Action::Block(reason) => ValidationResult::Block(reason),
            Action::BlockWithSuggestion { reason, suggestion } => {
                ValidationResult::BlockWithSuggestion { reason, suggestion }
            }
        }
    }
}

#[async_trait]
impl ToolValidator for RonValidator {
    fn id(&self) -> &str {
        &self.def.id
    }

    fn name(&self) -> &str {
        &self.def.name
    }

    fn description(&self) -> &str {
        self.def.description.as_deref().unwrap_or("")
    }

    fn applies_to(&self) -> Option<Vec<&str>> {
        if self.def.applies_to.is_empty() {
            None
        } else {
            Some(self.def.applies_to.iter().map(|s| s.as_str()).collect())
        }
    }

    fn priority(&self) -> ValidatorPriority {
        match self.def.priority {
            Priority::Critical => ValidatorPriority::Critical,
            Priority::High => ValidatorPriority::High,
            Priority::Normal => ValidatorPriority::Normal,
            Priority::Low => ValidatorPriority::Low,
        }
    }

    async fn validate(&self, ctx: &ValidationContext) -> ValidationResult {
        // Check each rule in order
        for rule in &self.def.rules {
            if rule.when.evaluate(ctx) {
                return rule.then.clone().into_result();
            }
        }
        // No rules matched, use default
        self.def.default.clone().into_result()
    }
}

/// Load all validators from a directory into the registry
pub fn load_validators_from_dir(dir: &Path, registry: &mut ValidatorRegistry) -> usize {
    let mut count = 0;

    if !dir.exists() {
        log::debug!("[VALIDATORS] Directory does not exist: {}", dir.display());
        return count;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("[VALIDATORS] Failed to read directory {}: {}", dir.display(), e);
            return count;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip non-.ron files
        if path.extension().map(|e| e != "ron").unwrap_or(true) {
            continue;
        }

        match RonValidator::from_file(&path) {
            Ok(validator) => {
                log::info!("[VALIDATORS] Loaded {} from {}", validator.id(), path.display());
                registry.register(Arc::new(validator));
                count += 1;
            }
            Err(e) => {
                log::warn!("[VALIDATORS] Failed to load {}: {}", path.display(), e);
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::api_keys::ApiKeyId;
    use crate::tools::types::ToolContext;
    use serde_json::json;

    const TEST_VALIDATOR_RON: &str = r#"
ValidatorDef(
    id: "test_validator",
    name: "Test Validator",
    description: "A test validator",
    applies_to: ["test_tool"],
    priority: Normal,
    rules: [
        (
            when: UrlContains("blocked.com"),
            then: Block("This domain is blocked"),
        ),
        (
            when: ArgEquals("action", "dangerous"),
            then: BlockWithSuggestion(
                reason: "Dangerous action",
                suggestion: "Use safe_action instead",
            ),
        ),
    ],
    default: Allow,
)
"#;

    #[test]
    fn test_parse_validator() {
        let validator = RonValidator::from_str(TEST_VALIDATOR_RON).unwrap();

        assert_eq!(validator.id(), "test_validator");
        assert_eq!(validator.name(), "Test Validator");
        assert_eq!(validator.description(), "A test validator");
        assert_eq!(validator.applies_to(), Some(vec!["test_tool"]));
        assert_eq!(validator.priority(), ValidatorPriority::Normal);
    }

    #[tokio::test]
    async fn test_validate_allows_by_default() {
        let validator = RonValidator::from_str(TEST_VALIDATOR_RON).unwrap();
        let ctx = ValidationContext::new(
            "test_tool".into(),
            json!({
                "url": "https://allowed.com/api"
            }),
            Arc::new(ToolContext::new()),
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_allowed());
    }

    #[tokio::test]
    async fn test_validate_blocks_url() {
        let validator = RonValidator::from_str(TEST_VALIDATOR_RON).unwrap();
        let ctx = ValidationContext::new(
            "test_tool".into(),
            json!({
                "url": "https://blocked.com/api"
            }),
            Arc::new(ToolContext::new()),
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_blocked());
        assert_eq!(result.block_reason(), Some("This domain is blocked"));
    }

    #[tokio::test]
    async fn test_validate_blocks_with_suggestion() {
        let validator = RonValidator::from_str(TEST_VALIDATOR_RON).unwrap();
        let ctx = ValidationContext::new(
            "test_tool".into(),
            json!({
                "action": "dangerous"
            }),
            Arc::new(ToolContext::new()),
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_blocked());
        assert_eq!(result.block_reason(), Some("Dangerous action"));
        assert_eq!(result.suggestion(), Some("Use safe_action instead"));
    }

    const X402_DUPLICATE_REGISTER_RON: &str = r#"
ValidatorDef(
    id: "x402_duplicate_register",
    name: "X402 Duplicate Registration Blocker",
    description: "Prevents re-registration when API key exists",
    applies_to: ["x402_post"],
    priority: Critical,
    rules: [
        (
            when: All([
                UrlContains("x402book.com"),
                UrlContains("/register"),
                CredentialExists("X402BOOK_TOKEN"),
            ]),
            then: BlockWithSuggestion(
                reason: "Already registered on x402book. Your API key is already configured.",
                suggestion: "Use x402_post to create threads at /api/boards/{slug}/threads instead.",
            ),
        ),
    ],
    default: Allow,
)
"#;

    #[test]
    fn test_parse_x402_validator() {
        let validator = RonValidator::from_str(X402_DUPLICATE_REGISTER_RON).unwrap();

        assert_eq!(validator.id(), "x402_duplicate_register");
        assert_eq!(validator.priority(), ValidatorPriority::Critical);
    }

    #[tokio::test]
    async fn test_x402_allows_without_token() {
        let validator = RonValidator::from_str(X402_DUPLICATE_REGISTER_RON).unwrap();
        let ctx = ValidationContext::new(
            "x402_post".into(),
            json!({
                "url": "https://api.x402book.com/api/agents/register"
            }),
            Arc::new(ToolContext::new()),
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_allowed());
    }

    #[tokio::test]
    async fn test_x402_blocks_with_token() {
        let validator = RonValidator::from_str(X402_DUPLICATE_REGISTER_RON).unwrap();
        let tool_context = ToolContext::new()
            .with_api_key_id(ApiKeyId::X402bookToken, "ak_existing_key".into());

        let ctx = ValidationContext::new(
            "x402_post".into(),
            json!({
                "url": "https://api.x402book.com/api/agents/register"
            }),
            Arc::new(tool_context),
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_blocked());
        assert!(result.block_reason().unwrap().contains("Already registered"));
    }

    #[tokio::test]
    async fn test_x402_allows_non_register_with_token() {
        let validator = RonValidator::from_str(X402_DUPLICATE_REGISTER_RON).unwrap();
        let tool_context = ToolContext::new()
            .with_api_key_id(ApiKeyId::X402bookToken, "ak_existing_key".into());

        let ctx = ValidationContext::new(
            "x402_post".into(),
            json!({
                "url": "https://api.x402book.com/api/boards/tech/threads"
            }),
            Arc::new(tool_context),
        );

        let result = validator.validate(&ctx).await;
        assert!(result.is_allowed());
    }
}
