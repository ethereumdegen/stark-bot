//! Import Identity tool
//!
//! If identity already exists in the DB, returns it (read mode).
//! Otherwise imports an existing EIP-8004 identity NFT from on-chain.
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
                description: "Specific agent ID to import. If omitted and no identity exists in DB, auto-discovers NFTs owned by this wallet.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ImportIdentityTool {
            definition: ToolDefinition {
                name: "import_identity".to_string(),
                description: "Get your agent identity. If identity already exists in the database, returns it. \
                    Otherwise imports an on-chain EIP-8004 identity NFT. \
                    If agent_id is provided, always imports/re-imports that specific identity from chain. \
                    If omitted and identity exists in DB, returns existing identity (read mode)."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Finance,
                hidden: false,
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

        // If no agent_id provided and a real identity exists in DB → return it (read mode)
        if params.agent_id.is_none() {
            if let Some(db) = &context.database {
                if let Some(row) = db.get_agent_identity_full() {
                    if row.agent_id > 0 {
                        let reg = row.to_registration_file();
                        let json_str = serde_json::to_string_pretty(&reg).unwrap_or_default();
                        log::info!("[import_identity] Returning identity from DB (agent_id={})", row.agent_id);
                        return ToolResult::success(format!(
                            "=== Your Agent Identity ===\n\
                            Agent ID: {}\n\
                            Registry: {}\n\
                            Chain ID: {}\n\
                            Registration URI: {}\n\n\
                            === Identity File ===\n{}",
                            row.agent_id,
                            &row.agent_registry,
                            row.chain_id,
                            row.registration_uri.as_deref().unwrap_or("(none)"),
                            json_str,
                        )).with_metadata(json!({
                            "agent_id": row.agent_id,
                            "name": row.name,
                            "description": row.description,
                            "registered": true,
                            "registration_uri": row.registration_uri,
                            "read_from_db": true,
                        }));
                    }
                }
            }
        }

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
        let rpc = match config.build_rpc(Some(wallet_provider.clone())) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to build RPC: {}", e)),
        };
        let registry = IdentityRegistry::new(config.clone(), rpc);

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

        // Fetch the agent URI (tokenURI on-chain) to find the hosted identity file
        let agent_uri = match registry.get_agent_uri(agent_id).await {
            Ok(uri) => Some(uri),
            Err(e) => {
                log::warn!("[import_identity] Could not fetch agent URI for #{}: {}", agent_id, e);
                None
            }
        };

        // Fetch the identity metadata from the URI
        let mut identity_hash: Option<String> = None;
        let mut fetched_reg: Option<crate::eip8004::types::RegistrationFile> = None;
        if let Some(ref uri) = agent_uri {
            if let Some(hash) = extract_identity_hash(uri) {
                identity_hash = Some(hash);
            }

            match registry.fetch_registration(uri).await {
                Ok(registration) => {
                    log::info!("[import_identity] Fetched identity metadata from {}", uri);
                    fetched_reg = Some(registration);
                }
                Err(e) => {
                    log::warn!("[import_identity] Could not fetch identity file from {}: {}", uri, e);
                }
            }
        }

        // Persist to SQLite (with full metadata)
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

        // Build metadata from fetched registration (or defaults)
        let (name, description, image, x402_support, active, services_json, supported_trust_json) =
            if let Some(ref reg) = fetched_reg {
                (
                    Some(reg.name.as_str()),
                    Some(reg.description.as_str()),
                    reg.image.as_deref(),
                    reg.x402_support,
                    reg.active,
                    serde_json::to_string(&reg.services).unwrap_or_else(|_| "[]".to_string()),
                    serde_json::to_string(&reg.supported_trust).unwrap_or_else(|_| "[]".to_string()),
                )
            } else {
                (None, None, None, true, true, "[]".to_string(), "[\"reputation\",\"x402-payments\"]".to_string())
            };

        match db.upsert_agent_identity(
            agent_id as i64,
            &agent_registry,
            config.chain_id as i64,
            name,
            description,
            image,
            x402_support,
            active,
            &services_json,
            &supported_trust_json,
            agent_uri.as_deref(),
        ) {
            Ok(_) => {
                log::info!(
                    "[import_identity] Persisted agent_id={} with metadata to agent_identity table",
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

        // Set agent_id register so subsequent preset calls (identity_get_uri, etc.) work
        context.set_register("agent_id", json!(agent_id), "import_identity");
        if let Some(ref uri) = agent_uri {
            context.set_register("agent_uri", json!(uri), "import_identity");
        }

        // Emit tool-result event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tool_result(
                ch_id, None, "import_identity",
                true, 0,
                &format!("Agent #{} imported successfully", agent_id),
                false, None,
            ));
        }

        let msg = format!(
            "IDENTITY IMPORTED ✓\n\n\
            Agent ID: {}\n\
            Owner: {}\n\
            URI: {}\n\
            Identity hash: {}\n\
            Registry: {}\n\
            Chain: {} ({})\n\n\
            The identity NFT has been imported and saved to the database.\n\
            The frontend dashboard will now show your registered identity.",
            agent_id,
            wallet_address,
            agent_uri.as_deref().unwrap_or("(unknown)"),
            identity_hash.as_deref().unwrap_or("(none)"),
            config.identity_registry,
            config.chain_name,
            config.chain_id,
        );

        ToolResult::success(msg).with_metadata(json!({
            "agent_id": agent_id,
            "owner": wallet_address,
            "agent_uri": agent_uri,
            "identity_hash": identity_hash,
            "agent_registry": agent_registry,
            "chain_id": config.chain_id,
            "persisted": true,
        }))
    }
}

/// Extract identity hash from identity.defirelay.com URLs.
/// URL format: https://identity.defirelay.com/api/identity/<hash>/raw
fn extract_identity_hash(uri: &str) -> Option<String> {
    let parts: Vec<&str> = uri.split('/').collect();
    // Look for the pattern: .../identity/<hash>/raw or .../identity/<hash>
    for window in parts.windows(3) {
        if window[0] == "identity" && (window[2] == "raw" || !window[1].is_empty()) {
            let hash = window[1];
            // Sanity check: hex hashes are typically 64 chars
            if hash.len() >= 32 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hash.to_string());
            }
        }
    }
    None
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

    #[test]
    fn test_extract_identity_hash() {
        let url = "https://identity.defirelay.com/api/identity/9148161cbf5bc8600533a462a8f84dcceb31b8f5714a403d6122ba7ae774217e/raw";
        assert_eq!(
            extract_identity_hash(url),
            Some("9148161cbf5bc8600533a462a8f84dcceb31b8f5714a403d6122ba7ae774217e".to_string())
        );

        // No hash in URL
        assert_eq!(extract_identity_hash("https://example.com/foo"), None);

        // IPFS URL — no hash to extract
        assert_eq!(extract_identity_hash("ipfs://QmFoo"), None);
    }
}
