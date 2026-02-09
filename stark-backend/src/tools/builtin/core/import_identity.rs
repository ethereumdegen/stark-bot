//! Import Identity tool
//!
//! Imports an existing EIP-8004 identity NFT that was transferred to this wallet.
//! Queries the StarkLicense contract for identity NFTs owned by the current wallet,
//! verifies ownership, and persists the agent_id locally.

use crate::eip8004::config::Eip8004Config;
use crate::eip8004::identity::IdentityRegistry;
use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ImportIdentityTool {
    definition: ToolDefinition,
}

impl ImportIdentityTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "agent_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Specific agent ID to import. If omitted, auto-discovers identity NFTs owned by this wallet.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ImportIdentityTool {
            definition: ToolDefinition {
                name: "import_identity".to_string(),
                description: "Import an existing EIP-8004 identity NFT that was transferred to this wallet. \
                    If agent_id is provided, verifies ownership and imports that specific identity. \
                    If omitted, auto-discovers identity NFTs owned by this wallet via balanceOf + tokenOfOwnerByIndex."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Finance,
            },
        }
    }
}

impl Default for ImportIdentityTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ImportIdentityParams {
    agent_id: Option<u64>,
}

#[async_trait]
impl Tool for ImportIdentityTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[import_identity] Raw params: {}", params);

        let params: ImportIdentityParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get wallet address
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => return ToolResult::error("Wallet not configured. Cannot determine ownership."),
        };
        let wallet_address = wallet_provider.get_address();

        // Emit tool-call event for UI
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::agent_tool_call(
                ch_id, None, "import_identity",
                &json!({"agent_id": params.agent_id, "wallet": &wallet_address}),
            ));
        }

        // Create IdentityRegistry
        let config = Eip8004Config::from_env();
        let registry = IdentityRegistry::new_with_wallet_provider(
            config.clone(),
            wallet_provider.clone(),
        );

        if !registry.is_deployed() {
            return ToolResult::error("Identity Registry not deployed on this chain.");
        }

        // Resolve agent_id — either from param or auto-discover
        let agent_id = match params.agent_id {
            Some(id) => {
                // Verify ownership
                match registry.get_owner(id).await {
                    Ok(owner) => {
                        if !owner.eq_ignore_ascii_case(&wallet_address) {
                            return ToolResult::error(format!(
                                "Agent #{} is not owned by this wallet.\nOwner: {}\nYour wallet: {}",
                                id, owner, wallet_address
                            ));
                        }
                    }
                    Err(e) => {
                        return ToolResult::error(format!(
                            "Failed to query owner of agent #{}: {}", id, e
                        ));
                    }
                }
                id
            }
            None => {
                // Auto-discover via balanceOf + tokenOfOwnerByIndex
                let balance = match registry.balance_of(&wallet_address).await {
                    Ok(b) => b,
                    Err(e) => {
                        return ToolResult::error(format!(
                            "Failed to query balanceOf for {}: {}", wallet_address, e
                        ));
                    }
                };

                if balance == 0 {
                    return ToolResult::error(format!(
                        "No identity NFTs found for wallet {}. \
                        Make sure the NFT has been transferred to this address.",
                        wallet_address
                    ));
                }

                if balance == 1 {
                    match registry.token_of_owner_by_index(&wallet_address, 0).await {
                        Ok(id) => id,
                        Err(e) => {
                            return ToolResult::error(format!(
                                "Failed to query tokenOfOwnerByIndex: {}", e
                            ));
                        }
                    }
                } else {
                    // Multiple NFTs — list them and ask user to specify
                    let max_display = balance.min(10);
                    let mut ids = Vec::new();
                    for i in 0..max_display {
                        match registry.token_of_owner_by_index(&wallet_address, i).await {
                            Ok(id) => ids.push(id),
                            Err(e) => {
                                log::warn!("[import_identity] Failed to query index {}: {}", i, e);
                            }
                        }
                    }

                    let id_list: Vec<String> = ids.iter().map(|id| format!("  - Agent #{}", id)).collect();
                    let suffix = if balance > max_display {
                        format!("\n  ... and {} more", balance - max_display)
                    } else {
                        String::new()
                    };

                    return ToolResult::error(format!(
                        "Multiple identity NFTs found ({} total) for wallet {}:\n{}{}\n\n\
                        Please call import_identity with a specific agent_id parameter.",
                        balance, wallet_address, id_list.join("\n"), suffix
                    ));
                }
            }
        };

        // Verify the agent exists on-chain by fetching its URI
        let agent_uri = match registry.get_agent_uri(agent_id).await {
            Ok(uri) => Some(uri),
            Err(e) => {
                log::warn!("[import_identity] Could not fetch agent URI for #{}: {}", agent_id, e);
                None
            }
        };

        // Persist to SQLite
        let agent_registry = config.agent_registry_string();

        let db = match &context.database {
            Some(db) => db,
            None => {
                let msg = format!(
                    "IDENTITY IMPORTED (DB unavailable)\n\n\
                    Agent ID: {}\n\
                    Owner: {}\n\
                    URI: {}\n\n\
                    Warning: Could not persist to local database.",
                    agent_id, wallet_address,
                    agent_uri.as_deref().unwrap_or("(unknown)"),
                );
                return ToolResult::success(msg).with_metadata(json!({
                    "agent_id": agent_id,
                    "owner": wallet_address,
                    "agent_uri": agent_uri,
                    "persisted": false,
                }));
            }
        };

        let conn = db.conn();

        // Upsert: delete any existing rows first (only one identity per agent)
        let _ = conn.execute("DELETE FROM agent_identity", []);

        let insert_result = conn.execute(
            "INSERT INTO agent_identity (agent_id, agent_registry, chain_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                agent_id as i64,
                agent_registry,
                config.chain_id as i64,
            ],
        );

        match insert_result {
            Ok(_) => {
                log::info!(
                    "[import_identity] Persisted agent_id={} to agent_identity table",
                    agent_id
                );
            }
            Err(e) => {
                log::error!("[import_identity] Failed to persist: {}", e);
                let msg = format!(
                    "IDENTITY IMPORTED (DB write failed)\n\n\
                    Agent ID: {}\nOwner: {}\nURI: {}\n\n\
                    Error persisting to database: {}",
                    agent_id, wallet_address,
                    agent_uri.as_deref().unwrap_or("(unknown)"), e
                );
                return ToolResult::success(msg).with_metadata(json!({
                    "agent_id": agent_id,
                    "owner": wallet_address,
                    "agent_uri": agent_uri,
                    "persisted": false,
                }));
            }
        }

        // Emit tool-result event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tool_result(
                ch_id, None, "import_identity",
                true, 0,
                &format!("Agent #{} imported successfully", agent_id),
                false,
            ));
        }

        let msg = format!(
            "IDENTITY IMPORTED ✓\n\n\
            Agent ID: {}\n\
            Owner: {}\n\
            URI: {}\n\
            Registry: {}\n\
            Chain: {} ({})\n\n\
            The identity NFT has been imported and saved locally.\n\
            The frontend dashboard will now show your registered identity.",
            agent_id,
            wallet_address,
            agent_uri.as_deref().unwrap_or("(unknown)"),
            config.identity_registry,
            config.chain_name,
            config.chain_id,
        );

        ToolResult::success(msg).with_metadata(json!({
            "agent_id": agent_id,
            "owner": wallet_address,
            "agent_uri": agent_uri,
            "agent_registry": agent_registry,
            "chain_id": config.chain_id,
            "persisted": true,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_creation() {
        let tool = ImportIdentityTool::new();
        assert_eq!(tool.definition().name, "import_identity");
        assert_eq!(tool.definition().group, ToolGroup::Finance);
    }

    #[test]
    fn test_tool_has_agent_id_param() {
        let tool = ImportIdentityTool::new();
        let def = tool.definition();
        assert!(def.input_schema.properties.contains_key("agent_id"));
        assert!(def.input_schema.required.is_empty());
    }
}
