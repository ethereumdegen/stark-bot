//! x402 Fetch tool for making paid HTTP requests via x402 protocol
//!
//! Uses presets to build URLs from register values, preventing hallucination.

use crate::tools::http_retry::HttpRetryManager;
use crate::tools::presets::{get_chain_id, get_fetch_preset, get_network_name, list_fetch_presets};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::x402::X402Client;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// x402 Fetch tool for paid HTTP requests (preset-only)
pub struct X402FetchTool {
    definition: ToolDefinition,
}

impl X402FetchTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "preset".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Preset name. Available: 'swap_quote'. Presets read from registers (sell_token, buy_token, sell_amount, wallet_address) and build URLs automatically. You do NOT need to pass any URL or parameters.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["swap_quote".to_string()]),
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network: 'base' or 'mainnet'".to_string(),
                default: Some(json!("base")),
                items: None,
                enum_values: Some(vec!["base".to_string(), "mainnet".to_string()]),
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name to store the result internally (e.g., 'swap_quote'). The data is stored in a register for use by decode_calldata — do NOT copy or extract data from the response yourself.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        X402FetchTool {
            definition: ToolDefinition {
                name: "x402_fetch".to_string(),
                description: "Fetch swap quotes from x402-enabled endpoints. Uses presets that automatically read from registers (sell_token, buy_token, sell_amount, wallet_address) to build the request. The response is stored internally in a register via cache_as — do NOT extract or copy hex/calldata from the response. After calling this, use decode_calldata to process the cached result.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["preset".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }

    /// Get or create the x402 client
    /// Uses wallet_provider from context if available (Flash mode), otherwise falls back to env var
    fn get_client(&self, context: &ToolContext) -> Result<X402Client, String> {
        // Try wallet_provider from context first (works in both Standard and Flash mode)
        if let Some(ref wallet_provider) = context.wallet_provider {
            return X402Client::new(wallet_provider.clone());
        }

        // Fall back to private key from environment (Standard mode only)
        let private_key = crate::config::burner_wallet_private_key()
            .ok_or("No wallet provider in context and BURNER_WALLET_BOT_PRIVATE_KEY not set")?;

        X402Client::from_private_key(&private_key)
    }

    /// Apply a simple jq-like filter to extract fields from JSON
    fn apply_jq_filter(&self, value: &Value, filter: &str) -> Result<Value, String> {
        let filter = filter.trim();

        // Handle object construction: {key: .field, key2: .field2}
        if filter.starts_with('{') && filter.ends_with('}') {
            let inner = &filter[1..filter.len() - 1];
            let mut result = serde_json::Map::new();

            for part in Self::split_object_fields(inner) {
                let part = part.trim();
                if let Some(colon_pos) = part.find(':') {
                    let key = part[..colon_pos].trim();
                    let field_path = part[colon_pos + 1..].trim();
                    let extracted = self.extract_field(value, field_path)?;
                    result.insert(key.to_string(), extracted);
                }
            }

            return Ok(Value::Object(result));
        }

        // Handle simple field access
        self.extract_field(value, filter)
    }

    /// Split object fields handling nested braces
    fn split_object_fields(s: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for c in s.chars() {
            match c {
                '{' | '[' => {
                    depth += 1;
                    current.push(c);
                }
                '}' | ']' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    fields.push(current.trim().to_string());
                    current = String::new();
                }
                _ => current.push(c),
            }
        }

        if !current.trim().is_empty() {
            fields.push(current.trim().to_string());
        }

        fields
    }

    /// Extract a field from JSON using dot notation
    fn extract_field(&self, value: &Value, path: &str) -> Result<Value, String> {
        let path = path.trim();

        if path == "." {
            return Ok(value.clone());
        }

        let path = path.strip_prefix('.').unwrap_or(path);
        let mut current = value;

        for part in path.split('.') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            match current {
                Value::Object(map) => {
                    current = map
                        .get(part)
                        .ok_or_else(|| format!("Field '{}' not found", part))?;
                }
                Value::Array(arr) => {
                    if let Ok(index) = part.parse::<usize>() {
                        current = arr
                            .get(index)
                            .ok_or_else(|| format!("Index {} out of bounds", index))?;
                    } else {
                        return Err(format!("Cannot access '{}' on array", part));
                    }
                }
                _ => return Err(format!("Cannot access '{}' on non-object", part)),
            }
        }

        Ok(current.clone())
    }
}

impl Default for X402FetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct X402FetchParams {
    preset: String,
    #[serde(default = "default_network")]
    network: String,
    cache_as: Option<String>,
}

fn default_network() -> String {
    "base".to_string()
}

#[async_trait]
impl Tool for X402FetchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: X402FetchParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get preset configuration
        let preset = match get_fetch_preset(&params.preset) {
            Some(p) => p,
            None => {
                return ToolResult::error(format!(
                    "Unknown preset: '{}'. Available: {}",
                    params.preset,
                    list_fetch_presets().join(", ")
                ))
            }
        };

        // Store network info in registers for use by other tools
        let chain_id = get_chain_id(&params.network);
        let network_name = get_network_name(&params.network);
        context.set_register("network_name", json!(&network_name), "x402_fetch");
        context.set_register("chain_id", json!(&chain_id), "x402_fetch");
        log::info!(
            "[x402_fetch] Stored network info: name={}, chain_id={}",
            network_name, chain_id
        );

        // Build URL from registers
        let mut url_params: Vec<String> = Vec::new();

        // Add chain ID
        url_params.push(format!("chainId={}", chain_id));

        // Read register values and build URL params
        for (reg_key, param_name) in &preset.params {
            let value = match context.registers.get(reg_key) {
                Some(v) => match v.as_str() {
                    Some(s) => s.to_string(),
                    None => v.to_string().trim_matches('"').to_string(),
                },
                None => {
                    return ToolResult::error(format!(
                        "Preset '{}' requires register '{}' but it was not found. Available: {:?}",
                        params.preset,
                        reg_key,
                        context.registers.keys()
                    ));
                }
            };
            url_params.push(format!("{}={}", param_name, value));
        }

        let url = format!("{}?{}", preset.base_url, url_params.join("&"));
        log::info!("[x402_fetch] Preset '{}' built URL: {}", params.preset, url);

        // Validate URL is an x402 endpoint
        if !crate::x402::is_x402_endpoint(&url) {
            return ToolResult::error(
                "URL must be an x402-enabled endpoint. Check preset configuration.",
            );
        }

        // Get the x402 client
        let client = match self.get_client(context) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        // Extract host for retry tracking
        let retry_key = format!("x402:{}", params.preset);
        let retry_manager = HttpRetryManager::global();

        // Retry configuration for swap_quote preset
        let max_retries = if params.preset == "swap_quote" { 3 } else { 1 };
        let retry_delay_secs = 5;

        let mut last_error: Option<String> = None;
        let mut response_opt = None;

        for attempt in 1..=max_retries {
            if attempt > 1 {
                log::info!("[x402_fetch] Retry attempt {}/{} for preset '{}' after {}s delay",
                    attempt, max_retries, params.preset, retry_delay_secs);
                tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay_secs)).await;
            }

            match client.get_with_payment(&url).await {
                Ok(r) => {
                    // Check if response is successful before accepting
                    let status = r.response.status();
                    if status.is_success() {
                        response_opt = Some(r);
                        last_error = None;
                        break;
                    }

                    // Non-success response - check if we should retry
                    let body = r.response.text().await.unwrap_or_default();
                    let error_msg = format!("HTTP error {}: {}", status, body);

                    // For swap_quote, retry on 402/5xx/429 errors
                    let should_retry = params.preset == "swap_quote" &&
                        (status.as_u16() == 402 ||
                         HttpRetryManager::is_retryable_status(status.as_u16()));

                    if should_retry && attempt < max_retries {
                        log::warn!("[x402_fetch] Retryable error on attempt {}: {}", attempt, error_msg);
                        last_error = Some(error_msg);
                        continue;
                    }

                    // Not retryable or last attempt - return error with appropriate type
                    if status.as_u16() == 402 && (body.contains("Settlement") || body.contains("Facilitator")) {
                        let delay = retry_manager.record_error(&retry_key);
                        return ToolResult::retryable_error(
                            format!("HTTP error {} Payment Required: {}\n\n⚠️ This is a temporary settlement/payment relay error. Retried {} times.", status, body, attempt),
                            delay
                        );
                    }

                    if HttpRetryManager::is_retryable_status(status.as_u16()) {
                        let delay = retry_manager.record_error(&retry_key);
                        return ToolResult::retryable_error(format!("HTTP error {}: {} (retried {} times)", status, body, attempt), delay);
                    }

                    return ToolResult::error(format!("HTTP error {}: {}", status, body));
                }
                Err(e) => {
                    let error_msg = format!("Request failed: {}", e);

                    // For swap_quote, retry on network errors
                    let should_retry = params.preset == "swap_quote" &&
                        HttpRetryManager::is_retryable_error(&error_msg);

                    if should_retry && attempt < max_retries {
                        log::warn!("[x402_fetch] Retryable network error on attempt {}: {}", attempt, error_msg);
                        last_error = Some(error_msg);
                        continue;
                    }

                    // Not retryable or last attempt
                    if HttpRetryManager::is_retryable_error(&error_msg) {
                        let delay = retry_manager.record_error(&retry_key);
                        return ToolResult::retryable_error(format!("{} (retried {} times)", error_msg, attempt), delay);
                    }
                    return ToolResult::error(error_msg);
                }
            }
        }

        // If we exhausted retries without success
        let response = match response_opt {
            Some(r) => r,
            None => {
                let error_msg = last_error.unwrap_or_else(|| "Unknown error after retries".to_string());
                return ToolResult::error(format!("Failed after {} retries: {}", max_retries, error_msg));
            }
        };

        // Success - reset backoff
        retry_manager.record_success(&retry_key);

        // Parse response body
        let body = match response.response.text().await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to read response: {}", e)),
        };

        // Parse as JSON and apply filter
        let json_value: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => {
                return ToolResult::error(format!("Response is not valid JSON: {}", body));
            }
        };

        let filtered = match self.apply_jq_filter(&json_value, &preset.jq_filter) {
            Ok(f) => f,
            Err(e) => return ToolResult::error(format!("Filter error: {}", e)),
        };

        let result_content =
            serde_json::to_string_pretty(&filtered).unwrap_or_else(|_| body.clone());

        // Cache result in register if cache_as is specified
        if let Some(ref register_name) = params.cache_as {
            context.set_register(register_name, filtered.clone(), "x402_fetch");
            log::info!(
                "[x402_fetch] Cached result in register '{}' (keys: {:?})",
                register_name,
                filtered.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
        }

        // Build metadata (status is always 200 here since we only break loop on success)
        let mut metadata = json!({
            "preset": params.preset,
            "network": params.network,
            "status": 200,
            "wallet": client.wallet_address(),
        });

        if let Some(payment) = response.payment {
            metadata["payment"] = json!({
                "amount": payment.amount_formatted,
                "asset": payment.asset,
                "pay_to": payment.pay_to,
            });
        }

        if let Some(ref register_name) = params.cache_as {
            metadata["cached_in_register"] = json!(register_name);
        }

        ToolResult::success(result_content).with_metadata(metadata)
    }
}
