//! x402 Agent Invoke tool for making paid requests to x402-enabled AI agents
//!
//! Unlike x402_fetch (preset-based), this tool works with any x402 agent endpoint.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::x402::X402Signer;
use async_trait::async_trait;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// x402 Agent Invoke tool
pub struct X402AgentInvokeTool {
    definition: ToolDefinition,
}

impl X402AgentInvokeTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "agent_url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Base URL of the x402 agent (e.g., https://dad-jokes-agent-production.up.railway.app)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "entrypoint".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Entrypoint name to invoke (e.g., 'joke', 'health')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "input".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "Input object to send to the entrypoint (will be wrapped in {\"input\": ...})".to_string(),
                default: Some(json!({})),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network for payment: 'base' (mainnet) or 'base-sepolia' (testnet)".to_string(),
                default: Some(json!("base")),
                items: None,
                enum_values: Some(vec!["base".to_string(), "base-sepolia".to_string()]),
            },
        );

        X402AgentInvokeTool {
            definition: ToolDefinition {
                name: "x402_agent_invoke".to_string(),
                description: "Invoke an x402-enabled AI agent endpoint with automatic USDC payment on Base. Handles 402 Payment Required flow automatically.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["agent_url".to_string(), "entrypoint".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }

    /// Get or create the x402 signer
    /// Uses wallet_provider from context if available (Flash mode), otherwise falls back to env var
    fn get_signer(&self, context: &ToolContext) -> Result<X402Signer, String> {
        // Try wallet_provider from context first (works in both Standard and Flash mode)
        if let Some(ref wallet_provider) = context.wallet_provider {
            return Ok(X402Signer::new(wallet_provider.clone()));
        }

        // Fall back to private key from environment (Standard mode only)
        let private_key = crate::config::burner_wallet_private_key()
            .ok_or("No wallet provider in context and BURNER_WALLET_BOT_PRIVATE_KEY not set")?;

        X402Signer::from_private_key(&private_key)
    }
}

impl Default for X402AgentInvokeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct X402AgentInvokeParams {
    agent_url: String,
    entrypoint: String,
    #[serde(default)]
    input: Value,
    #[serde(default = "default_network")]
    network: String,
}

fn default_network() -> String {
    "base".to_string()
}

/// 402 response from agent (JSON body format)
#[derive(Debug, Deserialize)]
struct Agent402Response {
    #[allow(dead_code)]
    error: Option<serde_json::Value>,
    accepts: Vec<AgentPaymentOption>,
    #[serde(rename = "x402Version", default = "default_x402_version")]
    x402_version: u8,
}

fn default_x402_version() -> u8 {
    1
}

/// Extra token metadata from 402 response
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentPaymentExtra {
    token: Option<String>,
    address: Option<String>,
    decimals: Option<u8>,
    name: Option<String>,
    version: Option<String>,
    facilitator_signer: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentPaymentOption {
    scheme: String,
    network: String,
    #[serde(alias = "maxAmountRequired")]
    max_amount_required: String,
    #[serde(alias = "payTo")]
    pay_to: String,
    asset: String,
    #[serde(default)]
    max_timeout_seconds: Option<u64>,
    resource: Option<String>,
    description: Option<String>,
    #[serde(default)]
    extra: Option<AgentPaymentExtra>,
}

/// Payment payload for X-PAYMENT header (matches x402 spec)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaymentPayload {
    x402_version: u8,
    scheme: String,
    network: String,
    payload: ExactEvmPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExactEvmPayload {
    signature: String,
    authorization: EvmAuthorization,
}

/// Authorization types for different EIP standards
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum EvmAuthorization {
    /// EIP-2612 Permit authorization (for "permit" scheme)
    Eip2612(Eip2612Authorization),
    /// EIP-3009 TransferWithAuthorization (for "exact" scheme)
    Eip3009(Eip3009Authorization),
}

/// EIP-2612 Permit authorization fields
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Eip2612Authorization {
    owner: String,
    spender: String,
    value: String,
    nonce: String,
    deadline: String,
}

/// EIP-3009 TransferWithAuthorization fields
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Eip3009Authorization {
    from: String,
    to: String,
    value: String,
    valid_after: String,
    valid_before: String,
    nonce: String,
}

#[async_trait]
impl Tool for X402AgentInvokeTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: X402AgentInvokeParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Build the invoke URL
        let url = format!(
            "{}/entrypoints/{}/invoke",
            params.agent_url.trim_end_matches('/'),
            params.entrypoint
        );

        // Build request body
        let body = json!({ "input": params.input });

        log::info!("[x402_agent] Invoking {} with input: {:?}", url, params.input);

        let client = context.http_client();

        // Make initial request
        let initial_response = match client
            .post(&url)
            .timeout(Duration::from_secs(60))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Request failed: {}", e)),
        };

        let status = initial_response.status();

        // If not 402, return the response directly
        if status.as_u16() != 402 {
            let response_body = initial_response.text().await.unwrap_or_default();

            if status.is_success() {
                // Try to parse as JSON for pretty output
                if let Ok(json_val) = serde_json::from_str::<Value>(&response_body) {
                    return ToolResult::success(
                        serde_json::to_string_pretty(&json_val).unwrap_or(response_body)
                    ).with_metadata(json!({
                        "url": url,
                        "status": status.as_u16(),
                        "payment_required": false,
                    }));
                }
                return ToolResult::success(response_body).with_metadata(json!({
                    "url": url,
                    "status": status.as_u16(),
                    "payment_required": false,
                }));
            } else {
                return ToolResult::error(format!("HTTP {}: {}", status, response_body));
            }
        }

        log::info!("[x402_agent] Received 402 Payment Required, parsing payment options");

        // Parse 402 response body
        let response_body = match initial_response.text().await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to read 402 response: {}", e)),
        };

        let payment_info: Agent402Response = match serde_json::from_str(&response_body) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to parse 402 response: {}. Body: {}",
                    e, response_body
                ))
            }
        };

        // Find a matching payment option for our network
        let payment_option = payment_info
            .accepts
            .iter()
            .find(|opt| {
                opt.network == params.network ||
                opt.network == "base" && params.network == "base" ||
                opt.network == "base-sepolia" && params.network == "base-sepolia"
            })
            .or_else(|| payment_info.accepts.first());

        let payment_option = match payment_option {
            Some(opt) => opt.clone(),
            None => return ToolResult::error("No compatible payment option found in 402 response"),
        };

        log::info!(
            "[x402_agent] Payment required: {} units to {} on {}",
            payment_option.max_amount_required,
            payment_option.pay_to,
            payment_option.network
        );

        // Check payment limit before signing
        if let Err(e) = crate::x402::payment_limits::check_payment_limit(
            &payment_option.asset,
            &payment_option.max_amount_required,
        ) {
            return ToolResult::error(e);
        }

        // Get signer
        let signer = match self.get_signer(context) {
            Ok(s) => s,
            Err(e) => return ToolResult::error(e),
        };

        let wallet_address = signer.address();
        log::info!("[x402_agent] Signing payment with wallet: {}", wallet_address);

        // Sign the payment using EIP-3009
        let x402_version = payment_info.x402_version;
        let payment_payload = match sign_agent_payment(&signer, &payment_option, x402_version).await {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Failed to sign payment: {}", e)),
        };

        // Encode payment as base64
        let payment_json = match serde_json::to_string(&payment_payload) {
            Ok(j) => j,
            Err(e) => return ToolResult::error(format!("Failed to serialize payment: {}", e)),
        };
        let payment_header = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &payment_json,
        );

        log::info!("[x402_agent] Retrying request with X-PAYMENT header");
        log::info!("[x402_agent] Payment JSON: {}", payment_json);
        log::info!("[x402_agent] Payment header (first 100 chars): {}...", &payment_header[..payment_header.len().min(100)]);

        // Retry with payment
        let paid_response = match client
            .post(&url)
            .timeout(Duration::from_secs(60))
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-PAYMENT", &payment_header)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Paid request failed: {}", e)),
        };

        let paid_status = paid_response.status();
        let paid_body = paid_response.text().await.unwrap_or_default();

        if !paid_status.is_success() {
            return ToolResult::error(format!(
                "Payment request failed with HTTP {}: {}",
                paid_status, paid_body
            ));
        }

        log::info!("[x402_agent] Payment successful! Status: {}", paid_status);

        // Format amount for display (USDC has 6 decimals)
        let amount_formatted = format_usdc(&payment_option.max_amount_required);

        // Try to parse response as JSON
        let result_content = if let Ok(json_val) = serde_json::from_str::<Value>(&paid_body) {
            serde_json::to_string_pretty(&json_val).unwrap_or(paid_body.clone())
        } else {
            paid_body
        };

        ToolResult::success(result_content).with_metadata(json!({
            "url": url,
            "status": paid_status.as_u16(),
            "payment_required": true,
            "payment": {
                "amount": payment_option.max_amount_required,
                "amount_formatted": amount_formatted,
                "asset": payment_option.asset,
                "pay_to": payment_option.pay_to,
                "network": payment_option.network,
                "wallet": wallet_address,
            }
        }))
    }
}

/// Sign payment using EIP-2612 (permit) or EIP-3009 (exact) based on scheme
async fn sign_agent_payment(
    signer: &X402Signer,
    option: &AgentPaymentOption,
    x402_version: u8,
) -> Result<PaymentPayload, String> {
    // Convert local extra to the x402 types extra
    let extra = option.extra.as_ref().map(|e| crate::x402::PaymentExtra {
        token: e.token.clone(),
        address: e.address.clone(),
        decimals: e.decimals,
        name: e.name.clone(),
        version: e.version.clone(),
        facilitator_signer: e.facilitator_signer.clone(),
    });

    // Create payment requirements in the format the signer expects
    let requirements = crate::x402::PaymentRequirements {
        scheme: option.scheme.clone(),
        network: option.network.clone(),
        max_amount_required: option.max_amount_required.clone(),
        pay_to_address: option.pay_to.clone(),
        asset: option.asset.clone(),
        max_timeout_seconds: option.max_timeout_seconds.unwrap_or(300),
        resource: option.resource.clone(),
        description: option.description.clone(),
        extra,
    };

    log::info!(
        "[x402_agent_invoke] Signing {} payment for {} on {}",
        option.scheme,
        option.asset,
        option.network
    );

    // Use the existing signer to create the payment
    let signed = signer.sign_payment(&requirements).await?;

    // Convert authorization based on scheme type
    let authorization = match signed.payload.authorization {
        crate::x402::EvmAuthorization::Eip2612(auth) => EvmAuthorization::Eip2612(Eip2612Authorization {
            owner: auth.owner,
            spender: auth.spender,
            value: auth.value,
            nonce: auth.nonce,
            deadline: auth.deadline,
        }),
        crate::x402::EvmAuthorization::Eip3009(auth) => EvmAuthorization::Eip3009(Eip3009Authorization {
            from: auth.from,
            to: auth.to,
            value: auth.value,
            valid_after: auth.valid_after,
            valid_before: auth.valid_before,
            nonce: auth.nonce,
        }),
    };

    // Convert to x402 spec format (scheme + network at top level, not in "accepted")
    Ok(PaymentPayload {
        x402_version,
        scheme: option.scheme.clone(),
        network: option.network.clone(),
        payload: ExactEvmPayload {
            signature: signed.payload.signature,
            authorization,
        },
    })
}

/// Format USDC amount (6 decimals) to human readable
fn format_usdc(raw: &str) -> String {
    if let Ok(value) = raw.parse::<u128>() {
        let whole = value / 1_000_000;
        let frac = value % 1_000_000;
        if frac == 0 {
            format!("{} USDC", whole)
        } else {
            let frac_str = format!("{:06}", frac).trim_end_matches('0').to_string();
            format!("{}.{} USDC", whole, frac_str)
        }
    } else {
        format!("{} units", raw)
    }
}
