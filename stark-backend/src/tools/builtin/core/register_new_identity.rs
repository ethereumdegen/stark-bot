use crate::eip8004::types::RegistrationFile;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for preparing a brand-new EIP-8004 agent identity.
/// Does NOT write to DB — the identity only gets persisted after on-chain registration
/// (via import_identity) returns a real agent_id.
pub struct RegisterNewIdentityTool {
    definition: ToolDefinition,
}

impl RegisterNewIdentityTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Agent name".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "description".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Agent description".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "image".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Image URL (optional)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        RegisterNewIdentityTool {
            definition: ToolDefinition {
                name: "register_new_identity".to_string(),
                description: "Prepare a brand-new EIP-8004 agent identity file (IDENTITY.json). \
                    This does NOT import an existing on-chain NFT — use import_identity for that. \
                    The identity is written to disk but NOT to the database until on-chain registration completes."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["name".to_string(), "description".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for RegisterNewIdentityTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct RegisterNewIdentityParams {
    name: String,
    description: String,
    image: Option<String>,
}

#[async_trait]
impl Tool for RegisterNewIdentityTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: RegisterNewIdentityParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // If identity already exists in DB, refuse
        if let Some(db) = &context.database {
            if db.get_agent_identity_full().is_some() {
                return ToolResult::error(
                    "Identity already exists in the database. Use import_identity to view or re-import it."
                );
            }
        }

        // If wallet already owns an NFT on-chain, refuse — use import_identity instead
        if let Some(wp) = &context.wallet_provider {
            let config = crate::eip8004::config::Eip8004Config::from_env();
            if config.is_identity_deployed() {
                if let Ok(rpc) = config.build_rpc(Some(wp.clone())) {
                    let registry = crate::eip8004::identity::IdentityRegistry::new(
                        config, rpc,
                    );
                    let wallet = wp.get_address();
                    if let Ok(balance) = registry.balance_of(&wallet).await {
                        if balance > 0 {
                            return ToolResult::error(format!(
                                "This wallet already owns {} identity NFT(s) on-chain. \
                                Use import_identity to import your existing identity.",
                                balance
                            ));
                        }
                    }
                }
            }
        }


        let mut reg = RegistrationFile::new(&params.name, &params.description);
        if let Some(ref img) = params.image {
            reg.image = Some(img.clone());
        }

        // Write to IDENTITY.json on disk (for upload to defirelay)
        let identity_path = crate::config::identity_document_path();
        let json_content = match serde_json::to_string_pretty(&reg) {
            Ok(j) => j,
            Err(e) => return ToolResult::error(format!("Failed to serialize identity: {}", e)),
        };

        if let Err(e) = std::fs::write(&identity_path, &json_content) {
            return ToolResult::error(format!("Failed to write {}: {}", identity_path.display(), e));
        }

        log::info!("Created IDENTITY.json for agent: {}", params.name);
        ToolResult::success(format!(
            "Identity file created at {}:\n{}\n\n\
            Next steps:\n\
            1. Upload to identity.defirelay.com (upload action needed)\n\
            2. Approve 1000 STARKBOT → identity_approve_registry preset\n\
            3. Register on-chain → identity_register preset (mints NFT)\n\
            4. Import the new NFT → import_identity (persists to DB)",
            identity_path.display(), json_content
        )).with_metadata(json!({
            "name": params.name,
            "path": identity_path,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_creation() {
        let tool = RegisterNewIdentityTool::new();
        assert_eq!(tool.definition().name, "register_new_identity");
        assert_eq!(tool.definition().group, ToolGroup::System);
    }
}
