//! ERC-8128 Fetch tool — make HTTP requests signed with Ethereum identity
//!
//! Attaches RFC 9421 HTTP Message Signatures (signed via ERC-191) so that
//! the receiving server can verify the agent's Ethereum address via `ecrecover`.

use crate::erc8128::Erc8128Signer;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Erc8128FetchTool {
    definition: ToolDefinition,
}

impl Erc8128FetchTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Full URL to request (e.g. https://api.example.com/v1/data?q=test)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "method".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "HTTP method (GET, POST, PUT, DELETE). Defaults to GET.".to_string(),
                default: Some(json!("GET")),
                items: None,
                enum_values: Some(vec![
                    "GET".to_string(),
                    "POST".to_string(),
                    "PUT".to_string(),
                    "DELETE".to_string(),
                ]),
            },
        );

        properties.insert(
            "body".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Request body (JSON string) for POST/PUT requests.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "headers".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description:
                    "Additional custom headers as key-value pairs (e.g. {\"X-Custom\": \"value\"})."
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
                description: "Chain ID for the keyid field (default: 1 for Ethereum mainnet)."
                    .to_string(),
                default: Some(json!(1)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Register name to cache the response body (e.g. 'auth_response')."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        Erc8128FetchTool {
            definition: ToolDefinition {
                name: "erc8128_fetch".to_string(),
                description: "Make an HTTP request signed with ERC-8128 (Ethereum identity). \
                    Cryptographically proves the agent's Ethereum address to the target server \
                    using RFC 9421 HTTP Message Signatures with ERC-191 signing. \
                    Use this when calling APIs that require ERC-8128 authentication."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["url".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }
}

impl Default for Erc8128FetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct Erc8128FetchParams {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    body: Option<String>,
    headers: Option<HashMap<String, String>>,
    #[serde(default = "default_chain_id")]
    chain_id: u64,
    cache_as: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_chain_id() -> u64 {
    1
}

#[async_trait]
impl Tool for Erc8128FetchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: Erc8128FetchParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Parse the URL to extract authority, path, query
        let parsed = match url::Url::parse(&params.url) {
            Ok(u) => u,
            Err(e) => return ToolResult::error(format!("Invalid URL: {}", e)),
        };

        let authority = match parsed.host_str() {
            Some(host) => {
                if let Some(port) = parsed.port() {
                    format!("{}:{}", host, port)
                } else {
                    host.to_string()
                }
            }
            None => return ToolResult::error("URL has no host"),
        };
        let path = parsed.path().to_string();
        let query = parsed.query().map(|q| q.to_string());

        // Get wallet provider
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp.clone(),
            None => {
                // Fall back to env-based provider
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
                    Err(e) => return ToolResult::error(format!("Failed to create wallet: {}", e)),
                }
            }
        };

        let signer = Erc8128Signer::new(wallet_provider, params.chain_id);
        let method = params.method.to_uppercase();
        let body_bytes = params.body.as_ref().map(|b| b.as_bytes());

        // Sign the request
        let signed = match signer
            .sign_request(
                &method,
                &authority,
                &path,
                query.as_deref(),
                body_bytes,
            )
            .await
        {
            Ok(h) => h,
            Err(e) => return ToolResult::error(format!("ERC-8128 signing failed: {}", e)),
        };

        // Build the HTTP request
        let client = reqwest::Client::new();
        let mut req = match method.as_str() {
            "GET" => client.get(&params.url),
            "POST" => client.post(&params.url),
            "PUT" => client.put(&params.url),
            "DELETE" => client.delete(&params.url),
            _ => return ToolResult::error(format!("Unsupported HTTP method: {}", method)),
        };

        // Attach ERC-8128 signature headers
        req = req
            .header("Signature-Input", &signed.signature_input)
            .header("Signature", &signed.signature);

        if let Some(ref digest) = signed.content_digest {
            req = req.header("Content-Digest", digest.as_str());
        }

        // Attach custom headers
        if let Some(ref custom_headers) = params.headers {
            for (k, v) in custom_headers {
                req = req.header(k.as_str(), v.as_str());
            }
        }

        // Attach body
        if let Some(ref body) = params.body {
            req = req
                .header("Content-Type", "application/json")
                .body(body.clone());
        }

        // Send
        log::info!(
            "[ERC8128] {} {} (signed as {})",
            method,
            params.url,
            signer.address()
        );

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("HTTP request failed: {}", e)),
        };

        let status = response.status();
        let status_code = status.as_u16();

        let body_text = match response.text().await {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("Failed to read response body: {}", e)),
        };

        // Cache in register if requested
        if let Some(ref register_name) = params.cache_as {
            let cache_value = match serde_json::from_str::<Value>(&body_text) {
                Ok(v) => v,
                Err(_) => json!(body_text),
            };
            context.set_register(register_name, cache_value, "erc8128_fetch");
            log::info!("[ERC8128] Cached response in register '{}'", register_name);
        }

        // Build metadata
        let metadata = json!({
            "status": status_code,
            "method": method,
            "url": params.url,
            "wallet": signer.address(),
            "chain_id": params.chain_id,
            "signed_headers": {
                "Signature-Input": signed.signature_input,
                "has_content_digest": signed.content_digest.is_some(),
            },
        });

        if status.is_success() {
            // Truncate very large responses for the tool output
            let display_body = if body_text.len() > 8000 {
                format!("{}...\n\n(truncated, {} bytes total)", &body_text[..8000], body_text.len())
            } else {
                body_text
            };

            ToolResult::success(display_body).with_metadata(metadata)
        } else {
            ToolResult::error(format!(
                "HTTP {} {}: {}",
                status_code,
                status.canonical_reason().unwrap_or(""),
                if body_text.len() > 2000 {
                    format!("{}...", &body_text[..2000])
                } else {
                    body_text
                }
            ))
            .with_metadata(metadata)
        }
    }
}
