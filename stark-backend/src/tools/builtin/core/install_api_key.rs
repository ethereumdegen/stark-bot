use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for installing API keys at runtime.
/// Persists to the database and injects into the current session so that
/// subsequent tool calls (e.g., web_fetch with $ALLIUM_API_KEY) can use them.
pub struct InstallApiKeyTool {
    definition: ToolDefinition,
}

impl InstallApiKeyTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "service_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The service name for this API key (e.g., 'ALLIUM_API_KEY'). Must be alphanumeric/underscores, max 64 chars. Will be normalized to UPPER_SNAKE_CASE.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "api_key".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The API key value to store.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        InstallApiKeyTool {
            definition: ToolDefinition {
                name: "install_api_key".to_string(),
                description: "Install an API key for a service. Persists to the database and makes it available in the current session for tools like web_fetch (via $SERVICE_NAME header expansion) and api_keys_check.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["service_name".to_string(), "api_key".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for InstallApiKeyTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct InstallApiKeyParams {
    service_name: String,
    api_key: String,
}

/// Validate and normalize a service name to UPPER_SNAKE_CASE.
/// Returns Ok(normalized) or Err(message).
fn validate_service_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("service_name cannot be empty".to_string());
    }
    if name.len() > 64 {
        return Err("service_name must be at most 64 characters".to_string());
    }

    // Check that it's only alphanumeric + underscores
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(
            "service_name must contain only letters, digits, and underscores (A-Za-z0-9_)"
                .to_string(),
        );
    }

    // Normalize to UPPER_SNAKE_CASE
    Ok(name.to_ascii_uppercase())
}

/// Check if a name collides with a built-in ApiKeyId name or any of its env_var aliases.
fn is_builtin_key(name: &str) -> bool {
    for key_id in ApiKeyId::iter() {
        if key_id.as_str().eq_ignore_ascii_case(name) {
            return true;
        }
        if let Some(env_vars) = key_id.env_vars() {
            for alias in env_vars {
                if alias.eq_ignore_ascii_case(name) {
                    return true;
                }
            }
        }
    }
    false
}

#[async_trait]
impl Tool for InstallApiKeyTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        // Block in safe mode
        if context
            .extra
            .get("safe_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return ToolResult::error(
                "install_api_key is not available in safe mode",
            );
        }

        let params: InstallApiKeyParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate & normalize
        let normalized_name = match validate_service_name(&params.service_name) {
            Ok(n) => n,
            Err(e) => return ToolResult::error(e),
        };

        if params.api_key.is_empty() {
            return ToolResult::error("api_key cannot be empty");
        }

        // Block overriding built-in keys — those must be set via Settings > API Keys
        if is_builtin_key(&normalized_name) {
            return ToolResult::error(format!(
                "'{}' is a built-in API key and cannot be installed via this tool. Ask the user to configure it in Settings > API Keys.",
                normalized_name
            ));
        }

        // Persist to database
        if let Some(db) = &context.database {
            if let Err(e) = db.upsert_api_key(&normalized_name, &params.api_key) {
                return ToolResult::error(format!("Failed to persist API key: {}", e));
            }
        }

        // Inject into current session
        context.install_api_key_runtime(&normalized_name, params.api_key);

        // Never echo the key value in the response
        ToolResult::success(
            json!({
                "installed": true,
                "service_name": normalized_name,
                "message": format!("API key '{}' installed successfully. It is now available for use in web_fetch headers (via ${}) and will appear in api_keys_check.", normalized_name, normalized_name)
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::sync::Arc;

    #[test]
    fn test_tool_definition() {
        let tool = InstallApiKeyTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "install_api_key");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"service_name".to_string()));
        assert!(def.input_schema.required.contains(&"api_key".to_string()));
    }

    #[test]
    fn test_validate_service_name() {
        assert_eq!(validate_service_name("allium_api_key").unwrap(), "ALLIUM_API_KEY");
        assert_eq!(validate_service_name("MyKey123").unwrap(), "MYKEY123");
        assert_eq!(validate_service_name("ALREADY_UPPER").unwrap(), "ALREADY_UPPER");

        assert!(validate_service_name("").is_err());
        assert!(validate_service_name("has-dashes").is_err());
        assert!(validate_service_name("has spaces").is_err());
        assert!(validate_service_name("has.dots").is_err());
        assert!(validate_service_name(&"a".repeat(65)).is_err());
    }

    #[tokio::test]
    async fn test_safe_mode_blocked() {
        let tool = InstallApiKeyTool::new();
        let mut context = ToolContext::new();
        context.extra.insert("safe_mode".to_string(), json!(true));

        let result = tool
            .execute(
                json!({"service_name": "TEST_KEY", "api_key": "secret123"}),
                &context,
            )
            .await;
        assert!(!result.success);
        assert!(result.content.contains("safe mode"));
    }

    #[tokio::test]
    async fn test_install_and_verify_in_context() {
        let tool = InstallApiKeyTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({"service_name": "test_service", "api_key": "my_secret_key"}),
                &context,
            )
            .await;
        assert!(result.success);
        assert!(result.content.contains("TEST_SERVICE"));
        // Key value must NOT appear in response
        assert!(!result.content.contains("my_secret_key"));

        // Verify key is available in context
        assert_eq!(
            context.get_api_key("TEST_SERVICE"),
            Some("my_secret_key".to_string())
        );

        // Verify it appears in list
        let names = context.list_api_key_names();
        assert!(names.contains(&"TEST_SERVICE".to_string()));
    }

    #[tokio::test]
    async fn test_install_persists_to_db() {
        let tool = InstallApiKeyTool::new();
        let db = Database::new(":memory:").unwrap();
        let mut context = ToolContext::new();
        context.database = Some(Arc::new(db));

        let result = tool
            .execute(
                json!({"service_name": "ALLIUM_API_KEY", "api_key": "allium_secret"}),
                &context,
            )
            .await;
        assert!(result.success);

        // Verify DB persistence
        let db = context.database.as_ref().unwrap();
        let stored = db.get_api_key("ALLIUM_API_KEY").unwrap();
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().api_key, "allium_secret");
    }

    #[tokio::test]
    async fn test_empty_api_key_rejected() {
        let tool = InstallApiKeyTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({"service_name": "TEST_KEY", "api_key": ""}),
                &context,
            )
            .await;
        assert!(!result.success);
        assert!(result.content.contains("empty"));
    }

    #[tokio::test]
    async fn test_invalid_service_name_rejected() {
        let tool = InstallApiKeyTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({"service_name": "invalid-name!", "api_key": "secret"}),
                &context,
            )
            .await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_builtin_key_blocked() {
        let tool = InstallApiKeyTool::new();
        let context = ToolContext::new();

        // Direct built-in name
        let result = tool
            .execute(
                json!({"service_name": "GITHUB_TOKEN", "api_key": "secret"}),
                &context,
            )
            .await;
        assert!(!result.success);
        assert!(result.content.contains("built-in"));

        // Env var alias of a built-in
        let result = tool
            .execute(
                json!({"service_name": "GH_TOKEN", "api_key": "secret"}),
                &context,
            )
            .await;
        assert!(!result.success);
        assert!(result.content.contains("built-in"));

        // Case-insensitive
        let result = tool
            .execute(
                json!({"service_name": "twitter_consumer_key", "api_key": "secret"}),
                &context,
            )
            .await;
        assert!(!result.success);
        assert!(result.content.contains("built-in"));
    }

    #[test]
    fn test_is_builtin_key() {
        assert!(is_builtin_key("GITHUB_TOKEN"));
        assert!(is_builtin_key("GH_TOKEN"));
        assert!(is_builtin_key("TWITTER_API_KEY")); // alias of TWITTER_CONSUMER_KEY
        assert!(is_builtin_key("MOLTX_API_KEY"));
        assert!(!is_builtin_key("ALLIUM_API_KEY"));
        assert!(!is_builtin_key("MY_CUSTOM_KEY"));
    }
}
