use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;

/// Tool for checking which API keys are configured
/// Returns which keys are set (not their values) so the agent can decide what actions are available
pub struct ApiKeysCheckTool {
    definition: ToolDefinition,
}

impl ApiKeysCheckTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        // Build enum values from ApiKeyId variants
        let key_names: Vec<String> = ApiKeyId::all_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        properties.insert(
            "key_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional: Check a specific key. If omitted, returns status of all keys.".to_string(),
                default: None,
                items: None,
                enum_values: Some(key_names),
            },
        );

        ApiKeysCheckTool {
            definition: ToolDefinition {
                name: "api_keys_check".to_string(),
                description: "Check which API keys are configured. Returns whether keys are set (not their values). Use this before operations that require specific API keys to verify they're available.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::System,
            },
        }
    }

    fn check_key(&self, key_id: ApiKeyId, context: &ToolContext) -> bool {
        // Check in context first
        if let Some(key) = context.get_api_key_by_id(key_id) {
            if !key.is_empty() {
                return true;
            }
        }

        // Also check environment as fallback
        if let Some(env_vars) = key_id.env_vars() {
            for var in env_vars {
                if let Ok(val) = std::env::var(var) {
                    if !val.is_empty() {
                        return true;
                    }
                }
            }
        }

        false
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
            // Check specific key - use strum's FromStr
            let key_id = match ApiKeyId::from_str(&key_name) {
                Ok(id) => id,
                Err(_) => {
                    let valid_keys = ApiKeyId::all_names().join(", ");
                    return ToolResult::error(format!(
                        "Unknown key: {}. Valid keys: {}",
                        key_name, valid_keys
                    ));
                }
            };

            let is_set = self.check_key(key_id, context);

            ToolResult::success(json!({
                "key": key_name,
                "configured": is_set,
                "message": if is_set {
                    format!("{} is configured and ready to use", key_name)
                } else {
                    format!("{} is NOT configured. Ask the user to add it in Settings > API Keys.", key_name)
                }
            }).to_string())
        } else {
            // Check all keys using iterator
            let mut results = Vec::new();
            let mut configured_count = 0;

            for key_id in ApiKeyId::iter() {
                let is_set = self.check_key(key_id, context);
                if is_set {
                    configured_count += 1;
                }
                results.push(json!({
                    "key": key_id.as_str(),
                    "configured": is_set
                }));
            }

            let total = results.len();
            let summary = if configured_count == 0 {
                "No API keys configured. User needs to add keys in Settings > API Keys.".to_string()
            } else {
                format!("{} of {} API keys configured", configured_count, total)
            };

            ToolResult::success(json!({
                "keys": results,
                "summary": summary
            }).to_string())
        }
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

        // Verify enum values are populated from ApiKeyId
        let enum_vals = def.input_schema.properties.get("key_name")
            .and_then(|p| p.enum_values.as_ref())
            .expect("enum_values should exist");
        assert!(enum_vals.contains(&"GITHUB_TOKEN".to_string()));
        assert!(enum_vals.contains(&"MOLTBOOK_TOKEN".to_string()));
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
    async fn test_check_specific_key() {
        let tool = ApiKeysCheckTool::new();
        let context = ToolContext::new();

        let result = tool.execute(json!({"key_name": "GITHUB_TOKEN"}), &context).await;
        assert!(result.success);
        assert!(result.content.contains("GITHUB_TOKEN"));
    }

    #[tokio::test]
    async fn test_invalid_key_name() {
        let tool = ApiKeysCheckTool::new();
        let context = ToolContext::new();

        let result = tool.execute(json!({"key_name": "INVALID_KEY"}), &context).await;
        assert!(!result.success);
        assert!(result.content.contains("Unknown key"));
    }
}
