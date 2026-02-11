use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Tool for checking which API keys are configured
/// Returns which keys are set (not their values) so the agent can decide what actions are available
/// Supports both built-in (ApiKeyId enum) and custom keys (installed via install_api_key)
pub struct ApiKeysCheckTool {
    definition: ToolDefinition,
}

impl ApiKeysCheckTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "key_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional: Check a specific key by name (e.g., 'GITHUB_TOKEN' or 'ALLIUM_API_KEY'). Accepts both built-in and custom key names. If omitted, returns status of all keys.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ApiKeysCheckTool {
            definition: ToolDefinition {
                name: "api_keys_check".to_string(),
                description: "Check which API keys are configured. Returns whether keys are set (not their values). Supports both built-in keys and custom keys installed via install_api_key.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::System,
            },
        }
    }
}

impl Default for ApiKeysCheckTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ApiKeysCheckParams {
    key_name: Option<String>,
}

#[async_trait]
impl Tool for ApiKeysCheckTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ApiKeysCheckParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        if let Some(key_name) = params.key_name {
            // Check specific key — works for both built-in and custom keys.
            // Also checks legacy names for renamed keys (e.g. RAILWAY_TOKEN → RAILWAY_API_TOKEN).
            let is_set = context
                .get_api_key(&key_name)
                .map(|k| !k.is_empty())
                .unwrap_or(false)
                || ApiKeyId::iter()
                    .find(|k| k.as_str() == key_name)
                    .and_then(|k| k.legacy_name())
                    .and_then(|legacy| context.get_api_key(legacy))
                    .map(|k| !k.is_empty())
                    .unwrap_or(false);

            ToolResult::success(json!({
                "key": key_name,
                "configured": is_set,
                "message": if is_set {
                    format!("{} is configured and ready to use", key_name)
                } else {
                    format!("{} is NOT configured. Ask the user to add it in Settings > API Keys, or use install_api_key to set it.", key_name)
                }
            }).to_string())
        } else {
            // Check all keys: built-in + custom (deduped)
            let mut results = Vec::new();
            let mut configured_count = 0;
            let mut seen = HashSet::new();

            // Built-in keys
            for key_id in ApiKeyId::iter() {
                let name = key_id.as_str().to_string();
                seen.insert(name.clone());
                let is_set = context
                    .get_api_key(&name)
                    .map(|k| !k.is_empty())
                    .unwrap_or(false);
                if is_set {
                    configured_count += 1;
                }
                results.push(json!({
                    "key": name,
                    "configured": is_set
                }));
            }

            // Custom keys from runtime store
            for name in context.list_api_key_names() {
                if seen.contains(&name) {
                    continue;
                }
                seen.insert(name.clone());
                let is_set = context
                    .get_api_key(&name)
                    .map(|k| !k.is_empty())
                    .unwrap_or(false);
                if is_set {
                    configured_count += 1;
                }
                results.push(json!({
                    "key": name,
                    "configured": is_set,
                    "custom": true
                }));
            }

            let total = results.len();
            let summary = if configured_count == 0 {
                "No API keys configured. User can add keys in Settings > API Keys, or use install_api_key.".to_string()
            } else {
                format!("{} of {} API keys configured", configured_count, total)
            };

            ToolResult::success(json!({
                "keys": results,
                "summary": summary
            }).to_string())
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = ApiKeysCheckTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "api_keys_check");
        assert!(def.input_schema.required.is_empty());

        // key_name should accept any string (no enum_values)
        let prop = def.input_schema.properties.get("key_name").unwrap();
        assert!(prop.enum_values.is_none());
    }

    #[tokio::test]
    async fn test_check_all_keys() {
        let tool = ApiKeysCheckTool::new();
        let context = ToolContext::new();

        let result = tool.execute(json!({}), &context).await;
        assert!(result.success);
        assert!(result.content.contains("keys"));
    }

    #[tokio::test]
    async fn test_check_specific_builtin_key() {
        let tool = ApiKeysCheckTool::new();
        let context = ToolContext::new();

        let result = tool.execute(json!({"key_name": "GITHUB_TOKEN"}), &context).await;
        assert!(result.success);
        assert!(result.content.contains("GITHUB_TOKEN"));
    }

    #[tokio::test]
    async fn test_check_custom_key() {
        let tool = ApiKeysCheckTool::new();
        let context = ToolContext::new();

        // Custom key not yet installed → not configured
        let result = tool.execute(json!({"key_name": "ALLIUM_API_KEY"}), &context).await;
        assert!(result.success);
        assert!(result.content.contains("NOT configured"));

        // Install it at runtime
        context.install_api_key_runtime("ALLIUM_API_KEY", "secret123".to_string());

        let result = tool.execute(json!({"key_name": "ALLIUM_API_KEY"}), &context).await;
        assert!(result.success);
        assert!(result.content.contains("configured and ready"));
    }

    #[tokio::test]
    async fn test_check_all_includes_custom_keys() {
        let tool = ApiKeysCheckTool::new();
        let context = ToolContext::new();
        context.install_api_key_runtime("MY_CUSTOM_KEY", "val".to_string());

        let result = tool.execute(json!({}), &context).await;
        assert!(result.success);
        assert!(result.content.contains("MY_CUSTOM_KEY"));
        assert!(result.content.contains("\"custom\":true"));
    }
}
