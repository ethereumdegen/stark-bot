//! Generic x402 POST tool for making paid requests to any x402-enabled endpoint
//!
//! Unlike x402_agent_invoke (which uses /entrypoints/{name}/invoke pattern),
//! this tool works with any URL that supports the x402 payment protocol.
//!
//! For x402book.com endpoints, automatically signs requests with ERC-8128 wallet auth.

use crate::erc8128::Erc8128Signer;
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
use url::Url;

/// Generic x402 POST tool
pub struct X402PostTool {
    definition: ToolDefinition,
}

impl X402PostTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Full URL to POST to (e.g., https://api.x402book.com/boards/tech/threads)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "body".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "JSON body to send with the request".to_string(),
                default: Some(json!({})),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "headers".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description:
                    "Additional headers (e.g., {\"Authorization\": \"Bearer sk_abc123\"})"
                        .to_string(),
                default: Some(json!({})),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network for payment: 'base' (mainnet) or 'base-sepolia' (testnet)"
                    .to_string(),
                default: Some(json!("base")),
                items: None,
                enum_values: Some(vec!["base".to_string(), "base-sepolia".to_string()]),
            },
        );

        X402PostTool {
            definition: ToolDefinition {
                name: "x402_post".to_string(),
                description: "POST to any x402-enabled endpoint with automatic USDC payment. \
                    Handles 402 Payment Required flow automatically. Use for APIs like x402book, \
                    paid content platforms, etc."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["url".to_string()],
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

    /// Check if URL is an x402book endpoint
    fn is_x402book_url(url: &str) -> bool {
        url.contains("x402book.com") || url.contains("x402book.io")
    }

    /// Sign a request with ERC-8128 for x402book URLs.
    /// Returns (signature-input, signature, optional content-digest) headers.
    async fn sign_erc8128(
        context: &ToolContext,
        method: &str,
        url: &str,
        body: &[u8],
    ) -> Result<crate::erc8128::Erc8128SignedHeaders, String> {
        let wp = context
            .wallet_provider
            .as_ref()
            .ok_or("No wallet provider available for ERC-8128 signing")?;

        let parsed = Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
        let authority = parsed
            .host_str()
            .map(|h| {
                if let Some(port) = parsed.port() {
                    format!("{}:{}", h, port)
                } else {
                    h.to_string()
                }
            })
            .unwrap_or_default();
        let path = parsed.path().to_string();
        let query = parsed.query().map(|q| q.to_string());

        let signer = Erc8128Signer::new(wp.clone(), 8453);
        let body_opt = if body.is_empty() { None } else { Some(body) };
        signer
            .sign_request(method, &authority, &path, query.as_deref(), body_opt)
            .await
    }

    /// Apply ERC-8128 signed headers to a request builder.
    fn apply_erc8128_headers(
        mut request: reqwest::RequestBuilder,
        signed: &crate::erc8128::Erc8128SignedHeaders,
    ) -> reqwest::RequestBuilder {
        request = request
            .header("signature-input", &signed.signature_input)
            .header("signature", &signed.signature);
        if let Some(ref digest) = signed.content_digest {
            request = request.header("content-digest", digest);
        }
        request
    }
}

impl Default for X402PostTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct X402PostParams {
    url: String,
    #[serde(default)]
    body: Value,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default = "default_network")]
    network: String,
}

fn default_network() -> String {
    "base".to_string()
}

/// 402 response format (x402 spec)
#[derive(Debug, Deserialize)]
struct X402Response {
    #[allow(dead_code)]
    error: Option<serde_json::Value>,
    accepts: Vec<PaymentOption>,
    #[serde(rename = "x402Version", default = "default_x402_version")]
    x402_version: u8,
}

fn default_x402_version() -> u8 {
    1
}

/// Extra token metadata from 402 response
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOptionExtra {
    /// Token symbol (e.g., "USDC")
    token: Option<String>,
    /// Token contract address
    address: Option<String>,
    /// Token decimals
    decimals: Option<u8>,
    /// Token name for EIP-712 (e.g., "USD Coin")
    name: Option<String>,
    /// Token version for EIP-712
    version: Option<String>,
    /// Facilitator signer address (spender for EIP-2612)
    facilitator_signer: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOption {
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
    /// Extra token metadata for signing
    #[serde(default)]
    extra: Option<PaymentOptionExtra>,
}

/// Payment payload for X-PAYMENT header
/// This is a simplified local type - the actual payload comes from X402Signer
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
impl Tool for X402PostTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let mut params: X402PostParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // LLMs sometimes send body as a JSON string instead of an object.
        // e.g. body: "{\"prompt\": \"...\"}" instead of body: {"prompt": "..."}
        // Parse the string into an object so the server receives valid JSON.
        if let Value::String(ref s) = params.body {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                log::info!("[x402_post] Coerced body from JSON string to object");
                params.body = parsed;
            }
        }
        // Also handle null/empty body — send empty object instead of `null`
        // so servers that check body.is_empty() get an actual empty body.
        if params.body.is_null() {
            params.body = json!({});
        }

        log::info!("[x402_post] POST to {} with body: {:?}", params.url, params.body);

        let is_x402book = Self::is_x402book_url(&params.url);
        let body_bytes = serde_json::to_vec(&params.body).unwrap_or_default();

        // Sign with ERC-8128 for x402book URLs
        let erc8128_signed = if is_x402book {
            match Self::sign_erc8128(context, "POST", &params.url, &body_bytes).await {
                Ok(signed) => {
                    log::info!("[x402_post] Signed request with ERC-8128 for x402book");
                    Some(signed)
                }
                Err(e) => {
                    log::warn!("[x402_post] ERC-8128 signing failed, proceeding without: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let client = context.http_client();

        // Build initial request with custom headers
        let mut request = client
            .post(&params.url)
            .timeout(Duration::from_secs(60))
            .header(header::CONTENT_TYPE, "application/json");

        // Apply ERC-8128 headers for x402book
        if let Some(ref signed) = erc8128_signed {
            request = Self::apply_erc8128_headers(request, signed);
        }

        for (key, value) in &params.headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Make initial request
        let initial_response = match request.json(&params.body).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Request failed: {}", e)),
        };

        let status = initial_response.status();

        // If not 402, return the response directly
        if status.as_u16() != 402 {
            let response_body = initial_response.text().await.unwrap_or_default();

            if status.is_success() {
                if let Ok(json_val) = serde_json::from_str::<Value>(&response_body) {
                    context.set_register("x402_result", json_val.clone(), "x402_post");
                    return ToolResult::success(
                        serde_json::to_string_pretty(&json_val).unwrap_or(response_body),
                    )
                    .with_metadata(json!({
                        "url": params.url,
                        "status": status.as_u16(),
                        "payment_required": false,
                    }));
                }
                return ToolResult::success(response_body).with_metadata(json!({
                    "url": params.url,
                    "status": status.as_u16(),
                    "payment_required": false,
                }));
            } else {
                return ToolResult::error(format!("HTTP {}: {}", status, response_body));
            }
        }

        log::info!("[x402_post] Received 402 Payment Required");

        // Parse 402 response
        let response_body = match initial_response.text().await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to read 402 response: {}", e)),
        };

        let payment_info: X402Response = match serde_json::from_str(&response_body) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to parse 402 response: {}. Body: {}",
                    e, response_body
                ))
            }
        };

        // Find matching payment option
        let payment_option = payment_info
            .accepts
            .iter()
            .find(|opt| opt.network == params.network)
            .or_else(|| payment_info.accepts.first());

        let payment_option = match payment_option {
            Some(opt) => opt.clone(),
            None => {
                return ToolResult::error("No compatible payment option found in 402 response")
            }
        };

        log::info!(
            "[x402_post] Payment: {} units to {} on {}",
            payment_option.max_amount_required,
            payment_option.pay_to,
            payment_option.network
        );

        // Get signer
        let signer = match self.get_signer(context) {
            Ok(s) => s,
            Err(e) => return ToolResult::error(e),
        };

        let wallet_address = signer.address();

        // Sign payment
        let payment_payload =
            match sign_payment(&signer, &payment_option, payment_info.x402_version).await {
                Ok(p) => p,
                Err(e) => return ToolResult::error(format!("Failed to sign payment: {}", e)),
            };

        // Encode as base64
        let payment_json = match serde_json::to_string(&payment_payload) {
            Ok(j) => j,
            Err(e) => return ToolResult::error(format!("Failed to serialize payment: {}", e)),
        };
        let payment_header =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &payment_json);

        log::info!("[x402_post] Retrying with X-PAYMENT header");

        // Re-sign with ERC-8128 for the retry (nonce/timestamps differ)
        let retry_signed = if is_x402book {
            match Self::sign_erc8128(context, "POST", &params.url, &body_bytes).await {
                Ok(signed) => Some(signed),
                Err(_) => erc8128_signed.clone(),
            }
        } else {
            None
        };

        // Retry with payment
        let mut paid_request = client
            .post(&params.url)
            .timeout(Duration::from_secs(60))
            .header(header::CONTENT_TYPE, "application/json")
            .header("X-PAYMENT", &payment_header);

        // Apply ERC-8128 headers for x402book
        if let Some(ref signed) = retry_signed {
            paid_request = Self::apply_erc8128_headers(paid_request, signed);
        }

        for (key, value) in &params.headers {
            paid_request = paid_request.header(key.as_str(), value.as_str());
        }

        let paid_response = match paid_request.json(&params.body).send().await {
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

        log::info!("[x402_post] Success! Status: {}", paid_status);

        let amount_formatted = format_usdc(&payment_option.max_amount_required);

        let result_content = if let Ok(json_val) = serde_json::from_str::<Value>(&paid_body) {
            context.set_register("x402_result", json_val.clone(), "x402_post");
            serde_json::to_string_pretty(&json_val).unwrap_or(paid_body.clone())
        } else {
            paid_body
        };

        ToolResult::success(result_content).with_metadata(json!({
            "url": params.url,
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
async fn sign_payment(
    signer: &X402Signer,
    option: &PaymentOption,
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
        "[x402_post] Signing {} payment for {} on {}",
        option.scheme,
        option.asset,
        option.network
    );

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

/// Format USDC amount (6 decimals)
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
