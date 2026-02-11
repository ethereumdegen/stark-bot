//! SIWA (Sign In With Agent) authentication tool
//!
//! Performs the SIWA nonce→sign→verify handshake against a target server,
//! then stores the auth receipt in a register for use by `erc8128_fetch`.

use crate::siwa::{build_siwa_message, SiwaMessageFields};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct SiwaAuthTool {
    definition: ToolDefinition,
}

impl SiwaAuthTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "server_url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Base URL of the SIWA-expecting service (e.g. https://api.example.com)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "nonce_path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path for the nonce endpoint (default: /siwa/nonce)".to_string(),
                default: Some(json!("/siwa/nonce")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "verify_path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path for the verify endpoint (default: /siwa/verify)".to_string(),
                default: Some(json!("/siwa/verify")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "domain".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Domain for the SIWA message (e.g. example.com)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "uri".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "URI for the SIWA message (e.g. https://example.com)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "agent_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Agent ID (ERC-8004 NFT ID). Falls back to agent_identity table if omitted."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "agent_registry".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Agent registry contract address. Falls back to agent_identity table if omitted."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "chain_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Chain ID for the SIWA message (default: 8453 for Base)".to_string(),
                default: Some(json!(8453)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "statement".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional statement to include in the SIWA message.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Register name to store the auth receipt (default: siwa_receipt)".to_string(),
                default: Some(json!("siwa_receipt")),
                items: None,
                enum_values: None,
            },
        );

        SiwaAuthTool {
            definition: ToolDefinition {
                name: "siwa_auth".to_string(),
                description: "Authenticate with a service using SIWA (Sign In With Agent). \
                    Performs the nonce→sign→verify handshake and stores the auth receipt \
                    in a register for subsequent requests via erc8128_fetch."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![
                        "server_url".to_string(),
                        "domain".to_string(),
                        "uri".to_string(),
                    ],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }
}

impl Default for SiwaAuthTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SiwaAuthParams {
    server_url: String,
    #[serde(default = "default_nonce_path")]
    nonce_path: String,
    #[serde(default = "default_verify_path")]
    verify_path: String,
    domain: String,
    uri: String,
    agent_id: Option<String>,
    agent_registry: Option<String>,
    #[serde(default = "default_chain_id")]
    chain_id: u64,
    statement: Option<String>,
    #[serde(default = "default_cache_as")]
    cache_as: String,
}

fn default_nonce_path() -> String {
    "/siwa/nonce".to_string()
}
fn default_verify_path() -> String {
    "/siwa/verify".to_string()
}
fn default_chain_id() -> u64 {
    8453
}
fn default_cache_as() -> String {
    "siwa_receipt".to_string()
}

/// Response from the nonce endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NonceResponse {
    nonce: String,
    issued_at: String,
    expiration_time: String,
}

#[async_trait]
impl Tool for SiwaAuthTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SiwaAuthParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // 1. Get wallet provider (same pattern as erc8128_fetch)
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp.clone(),
            None => {
                let pk = match crate::config::burner_wallet_private_key() {
                    Some(pk) => pk,
                    None => {
                        return ToolResult::error(
                            "No wallet provider available. Set BURNER_WALLET_BOT_PRIVATE_KEY or configure a wallet provider.",
                        )
                    }
                };
                match crate::wallet::EnvWalletProvider::from_private_key(&pk) {
                    Ok(p) => std::sync::Arc::new(p),
                    Err(e) => {
                        return ToolResult::error(format!("Failed to create wallet: {}", e))
                    }
                }
            }
        };

        let address = wallet_provider.get_address();

        // 2. Resolve agent_id and agent_registry
        let (agent_id, agent_registry) = resolve_agent_identity(
            &params,
            context,
        );
        let agent_id = match agent_id {
            Some(id) => id,
            None => return ToolResult::error(
                "No agent_id available. Provide it as a parameter or register an ERC-8004 identity first.",
            ),
        };
        let agent_registry = match agent_registry {
            Some(reg) => reg,
            None => return ToolResult::error(
                "No agent_registry available. Provide it as a parameter or register an ERC-8004 identity first.",
            ),
        };

        let client = context.http_client();
        let server_url = params.server_url.trim_end_matches('/');

        // 3. POST to nonce endpoint
        let nonce_url = format!("{}{}", server_url, params.nonce_path);
        log::info!("[SIWA] Requesting nonce from {}", nonce_url);

        let nonce_body = json!({
            "address": address,
            "agentId": agent_id,
            "agentRegistry": agent_registry,
        });

        let nonce_resp = match client
            .post(&nonce_url)
            .header("Content-Type", "application/json")
            .body(nonce_body.to_string())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to request nonce: {}", e)),
        };

        let nonce_status = nonce_resp.status();
        let nonce_text = match nonce_resp.text().await {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("Failed to read nonce response: {}", e)),
        };

        if !nonce_status.is_success() {
            return ToolResult::error(format!(
                "Nonce request failed (HTTP {}): {}",
                nonce_status.as_u16(),
                nonce_text,
            ));
        }

        let nonce_data: NonceResponse = match serde_json::from_str(&nonce_text) {
            Ok(d) => d,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to parse nonce response: {}. Body: {}",
                    e, nonce_text,
                ))
            }
        };

        // 4. Build SIWA message
        let message = build_siwa_message(&SiwaMessageFields {
            domain: params.domain.clone(),
            address: address.clone(),
            uri: params.uri.clone(),
            agent_id: agent_id.clone(),
            agent_registry: agent_registry.clone(),
            chain_id: params.chain_id,
            nonce: nonce_data.nonce.clone(),
            issued_at: nonce_data.issued_at.clone(),
            expiration_time: nonce_data.expiration_time.clone(),
            statement: params.statement.clone(),
        });

        // 5. Sign message with EIP-191
        let signature = match wallet_provider.sign_message(message.as_bytes()).await {
            Ok(sig) => format!("0x{}", hex::encode(sig.to_vec())),
            Err(e) => return ToolResult::error(format!("Failed to sign SIWA message: {}", e)),
        };

        // 6. POST to verify endpoint
        let verify_url = format!("{}{}", server_url, params.verify_path);
        log::info!("[SIWA] Verifying signature at {}", verify_url);

        let verify_body = json!({
            "message": message,
            "signature": signature,
        });

        let verify_resp = match client
            .post(&verify_url)
            .header("Content-Type", "application/json")
            .body(verify_body.to_string())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to send verify request: {}", e)),
        };

        let verify_status = verify_resp.status();
        let verify_text = match verify_resp.text().await {
            Ok(t) => t,
            Err(e) => {
                return ToolResult::error(format!("Failed to read verify response: {}", e))
            }
        };

        if !verify_status.is_success() {
            return ToolResult::error(format!(
                "Verify request failed (HTTP {}): {}",
                verify_status.as_u16(),
                verify_text,
            ));
        }

        // 7. Store receipt in register
        let receipt_value = match serde_json::from_str::<Value>(&verify_text) {
            Ok(v) => v,
            Err(_) => json!(verify_text),
        };
        context.set_register(&params.cache_as, receipt_value.clone(), "siwa_auth");
        log::info!(
            "[SIWA] Auth successful, receipt stored in register '{}'",
            params.cache_as
        );

        // 8. Return success
        let metadata = json!({
            "server_url": server_url,
            "address": address,
            "agent_id": agent_id,
            "agent_registry": agent_registry,
            "chain_id": params.chain_id,
            "register": params.cache_as,
        });

        ToolResult::success(format!(
            "SIWA authentication successful.\n\
             Address: {}\n\
             Agent ID: {}\n\
             Receipt stored in register: '{}'\n\n\
             Use erc8128_fetch with header X-SIWA-Receipt to make authenticated requests.\n\n\
             Server response: {}",
            address,
            agent_id,
            params.cache_as,
            if verify_text.len() > 2000 {
                format!("{}...", &verify_text[..2000])
            } else {
                verify_text
            },
        ))
        .with_metadata(metadata)
    }
}

/// Resolve agent_id and agent_registry from params → registers → DB
fn resolve_agent_identity(
    params: &SiwaAuthParams,
    context: &ToolContext,
) -> (Option<String>, Option<String>) {
    // Try params first
    let mut agent_id = params.agent_id.clone();
    let mut agent_registry = params.agent_registry.clone();

    // Fall back to registers
    if agent_id.is_none() {
        if let Some(val) = context.registers.get("agent_id") {
            agent_id = val.as_str().map(|s| s.to_string());
        }
    }
    if agent_registry.is_none() {
        if let Some(val) = context.registers.get("agent_registry") {
            agent_registry = val.as_str().map(|s| s.to_string());
        }
    }

    // Fall back to DB (agent_identity table)
    if agent_id.is_none() || agent_registry.is_none() {
        if let Some(ref db) = context.database {
            let conn = db.conn();
            if let Ok(row) = conn.query_row(
                "SELECT agent_id, agent_registry FROM agent_identity ORDER BY id DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0).ok(),
                        row.get::<_, String>(1).ok(),
                    ))
                },
            ) {
                if agent_id.is_none() {
                    if let Some(id) = row.0 {
                        agent_id = Some(id.to_string());
                    }
                }
                if agent_registry.is_none() {
                    agent_registry = row.1;
                }
            }
        }
    }

    (agent_id, agent_registry)
}
