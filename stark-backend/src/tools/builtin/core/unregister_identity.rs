//! Unregister Identity tool
//!
//! Wipes the locally stored agent identity from the database (and optionally
//! the IDENTITY.json file on disk). This does NOT burn or affect the on-chain
//! NFT — it only clears local state so the agent can re-import later via
//! `import_identity`.

use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct UnregisterIdentityTool {
    definition: ToolDefinition,
}

impl UnregisterIdentityTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "confirm".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Must be true to confirm the unregister action.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "delete_identity_file".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Also delete the IDENTITY.json file from disk. Defaults to false."
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        UnregisterIdentityTool {
            definition: ToolDefinition {
                name: "unregister_identity".to_string(),
                description: "Unregister your agent identity by wiping it from the local database. \
                    This does NOT burn or affect the on-chain NFT — it only clears local data. \
                    You can re-import the identity later with import_identity. \
                    Requires confirm=true."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["confirm".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for UnregisterIdentityTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct UnregisterIdentityParams {
    confirm: bool,
    delete_identity_file: Option<bool>,
}

#[async_trait]
impl Tool for UnregisterIdentityTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[unregister_identity] Raw params: {}", params);

        let params: UnregisterIdentityParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        if !params.confirm {
            return ToolResult::error(
                "You must pass confirm=true to unregister your identity. \
                This will wipe the agent identity from the local database. \
                The on-chain NFT will NOT be affected.",
            );
        }

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available."),
        };

        // Check that an identity actually exists
        let existing = db.get_agent_identity_full();
        if existing.is_none() {
            return ToolResult::error("No agent identity found in the database. Nothing to unregister.");
        }
        let existing = existing.unwrap();
        let agent_id = existing.agent_id;
        let name = existing.name.unwrap_or_default();

        // Emit tool-call event for UI
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::agent_tool_call(
                ch_id,
                None,
                "unregister_identity",
                &json!({"agent_id": agent_id}),
            ));
        }

        // Wipe from database
        if let Err(e) = db.delete_agent_identity() {
            return ToolResult::error(format!("Failed to delete identity from database: {}", e));
        }

        log::info!(
            "[unregister_identity] Deleted agent_id={} ('{}') from agent_identity table",
            agent_id, name
        );

        // Optionally delete IDENTITY.json from disk
        let mut file_deleted = false;
        if params.delete_identity_file.unwrap_or(false) {
            let identity_path = crate::config::identity_document_path();
            if identity_path.exists() {
                match std::fs::remove_file(&identity_path) {
                    Ok(_) => {
                        log::info!(
                            "[unregister_identity] Deleted {}",
                            identity_path.display()
                        );
                        file_deleted = true;
                    }
                    Err(e) => {
                        log::warn!(
                            "[unregister_identity] Failed to delete {}: {}",
                            identity_path.display(),
                            e
                        );
                    }
                }
            }
        }

        // Clear registers
        context.set_register("agent_id", json!(null), "unregister_identity");
        context.set_register("agent_uri", json!(null), "unregister_identity");

        // Emit tool-result event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tool_result(
                ch_id,
                None,
                "unregister_identity",
                true,
                0,
                &format!("Agent #{} unregistered locally", agent_id),
                false,
                None,
            ));
        }

        let file_note = if file_deleted {
            "\nIDENTITY.json has also been deleted from disk."
        } else {
            ""
        };

        let msg = format!(
            "IDENTITY UNREGISTERED ✓\n\n\
            Agent ID: {} ('{}')\n\n\
            The agent identity has been wiped from the local database.{}\n\
            The on-chain NFT is NOT affected — it still exists on-chain.\n\n\
            To re-import your identity, use: import_identity",
            agent_id, name, file_note
        );

        ToolResult::success(msg).with_metadata(json!({
            "agent_id": agent_id,
            "name": name,
            "deleted_from_db": true,
            "identity_file_deleted": file_deleted,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_creation() {
        let tool = UnregisterIdentityTool::new();
        assert_eq!(tool.definition().name, "unregister_identity");
        assert_eq!(tool.definition().group, ToolGroup::System);
    }

    #[test]
    fn test_tool_requires_confirm() {
        let tool = UnregisterIdentityTool::new();
        let def = tool.definition();
        assert!(def.input_schema.required.contains(&"confirm".to_string()));
    }
}
