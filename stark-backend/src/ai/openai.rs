use crate::ai::streaming::{StreamEvent, StreamSender};
use crate::ai::types::{AiError, AiResponse, ToolCall};
use crate::ai::Message;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::tools::ToolDefinition;
use crate::wallet::WalletProvider;
use crate::x402::{X402Client, X402PaymentInfo, is_x402_endpoint};
use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct OpenAIClient {
    client: Client,
    auth_headers: header::HeaderMap,
    endpoint: String,
    model: Option<String>,
    max_tokens: u32,
    x402_client: Option<Arc<X402Client>>,
    /// Optional broadcaster for emitting retry events
    broadcaster: Option<Arc<EventBroadcaster>>,
    /// Channel ID for events (set when broadcasting)
    channel_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct OpenAICompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

/// Streaming chunk response from OpenAI API
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAIStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
    index: usize,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamToolCall {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<OpenAIStreamFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenAIMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAICompletionResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorResponse {
    error: OpenAIError,
}

#[derive(Debug, Deserialize)]
struct OpenAIError {
    message: String,
}

impl OpenAIClient {
    pub fn new(api_key: &str, endpoint: Option<&str>, model: Option<&str>) -> Result<Self, String> {
        Self::new_with_x402_and_tokens(api_key, endpoint, model, None, None)
    }

    pub fn new_with_x402(
        api_key: &str,
        endpoint: Option<&str>,
        model: Option<&str>,
        burner_private_key: Option<&str>,
    ) -> Result<Self, String> {
        Self::new_with_x402_and_tokens(api_key, endpoint, model, burner_private_key, None)
    }

    /// Create OpenAI client with WalletProvider for x402 payments
    /// This works with both Standard mode (LocalWallet) and Flash mode (Privy)
    pub fn new_with_wallet_provider(
        api_key: &str,
        endpoint: Option<&str>,
        model: Option<&str>,
        wallet_provider: Option<Arc<dyn WalletProvider>>,
        max_tokens: Option<u32>,
    ) -> Result<Self, String> {
        let endpoint_url = endpoint
            .unwrap_or("https://api.openai.com/v1/chat/completions")
            .to_string();

        let mut auth_headers = header::HeaderMap::new();
        auth_headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        // Only add auth header if API key is provided and not empty
        if !api_key.is_empty() {
            let auth_value = header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| format!("Invalid API key format: {}", e))?;
            auth_headers.insert(header::AUTHORIZATION, auth_value);
        }

        // Create x402 client if wallet provider is provided and endpoint uses x402
        let x402_client = if is_x402_endpoint(&endpoint_url) {
            if let Some(provider) = wallet_provider {
                match X402Client::new(provider) {
                    Ok(c) => {
                        log::info!("[AI] x402 enabled for endpoint {} with wallet {}", endpoint_url, c.wallet_address());
                        Some(Arc::new(c))
                    }
                    Err(e) => {
                        log::warn!("[AI] Failed to create x402 client: {}", e);
                        None
                    }
                }
            } else {
                log::warn!("[AI] x402 endpoint {} requires wallet_provider", endpoint_url);
                None
            }
        } else {
            None
        };

        // Determine model: use provided model, or infer from endpoint URL
        let effective_model = match model {
            Some(m) if !m.is_empty() => Some(m.to_string()),
            _ => {
                if endpoint_url.contains("openai.com") {
                    Some("gpt-4o".to_string())
                } else if endpoint_url.contains("kimi") || endpoint_url.contains("moonshot") {
                    Some("kimi-k2-turbo-preview".to_string())
                } else {
                    None
                }
            }
        };

        Ok(Self {
            client: crate::http::shared_client().clone(),
            auth_headers,
            endpoint: endpoint_url,
            model: effective_model,
            max_tokens: max_tokens.unwrap_or(40096),
            x402_client,
            broadcaster: None,
            channel_id: None,
        })
    }

    pub fn new_with_x402_and_tokens(
        api_key: &str,
        endpoint: Option<&str>,
        model: Option<&str>,
        burner_private_key: Option<&str>,
        max_tokens: Option<u32>,
    ) -> Result<Self, String> {
        let endpoint_url = endpoint
            .unwrap_or("https://api.openai.com/v1/chat/completions")
            .to_string();

        let mut auth_headers = header::HeaderMap::new();
        auth_headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        // Only add auth header if API key is provided and not empty
        if !api_key.is_empty() {
            let auth_value = header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| format!("Invalid API key format: {}", e))?;
            auth_headers.insert(header::AUTHORIZATION, auth_value);
        }

        // Create x402 client if private key is provided and endpoint uses x402
        let x402_client = if is_x402_endpoint(&endpoint_url) {
            if let Some(pk) = burner_private_key {
                if !pk.is_empty() {
                    match X402Client::from_private_key(pk) {
                        Ok(c) => {
                            log::info!("[AI] x402 enabled for endpoint {} with wallet {}", endpoint_url, c.wallet_address());
                            Some(Arc::new(c))
                        }
                        Err(e) => {
                            log::warn!("[AI] Failed to create x402 client: {}", e);
                            None
                        }
                    }
                } else {
                    log::warn!("[AI] x402 endpoint {} requires BURNER_WALLET_BOT_PRIVATE_KEY", endpoint_url);
                    None
                }
            } else {
                log::warn!("[AI] x402 endpoint {} requires BURNER_WALLET_BOT_PRIVATE_KEY", endpoint_url);
                None
            }
        } else {
            None
        };

        // Determine model: use provided model, or infer from endpoint URL
        let model_name = match model {
            Some(m) if !m.is_empty() => Some(m.to_string()),
            _ => {
                if endpoint_url.contains("openai.com") {
                    Some("gpt-4o".to_string())
                } else if endpoint_url.contains("kimi") || endpoint_url.contains("moonshot") {
                    Some("kimi-k2-turbo-preview".to_string())
                } else {
                    None
                }
            }
        };

        Ok(Self {
            client: crate::http::shared_client().clone(),
            auth_headers,
            endpoint: endpoint_url,
            model: model_name,
            max_tokens: max_tokens.unwrap_or(40000),
            x402_client,
            broadcaster: None,
            channel_id: None,
        })
    }

    /// Set the broadcaster for emitting retry events
    pub fn with_broadcaster(mut self, broadcaster: Arc<EventBroadcaster>, channel_id: i64) -> Self {
        self.broadcaster = Some(broadcaster);
        self.channel_id = Some(channel_id);
        self
    }

    /// Emit a retry event if broadcaster is configured
    fn emit_retry_event(&self, attempt: u32, max_attempts: u32, wait_seconds: u64, error: &str) {
        if let (Some(broadcaster), Some(channel_id)) = (&self.broadcaster, self.channel_id) {
            broadcaster.broadcast(GatewayEvent::ai_retrying(
                channel_id,
                attempt,
                max_attempts,
                wait_seconds,
                error,
                "openai",
            ));
        }
    }

    pub async fn generate_text(&self, messages: Vec<Message>) -> Result<String, String> {
        let response = self.generate_with_tools_internal(messages, vec![], vec![]).await
            .map_err(|e| e.to_string())?;
        Ok(response.content)
    }

    /// Generate text and return payment info if x402 payment was made
    pub async fn generate_text_with_payment_info(&self, messages: Vec<Message>) -> Result<(String, Option<X402PaymentInfo>), String> {
        let response = self.generate_with_tools_internal(messages, vec![], vec![]).await
            .map_err(|e| e.to_string())?;
        Ok((response.content, response.x402_payment))
    }

    pub async fn generate_with_tools(
        &self,
        messages: Vec<Message>,
        tool_history: Vec<OpenAIMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<AiResponse, AiError> {
        self.generate_with_tools_internal(messages, tool_history, tools).await
    }

    async fn generate_with_tools_internal(
        &self,
        messages: Vec<Message>,
        tool_history: Vec<OpenAIMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<AiResponse, AiError> {
        // Convert messages to OpenAI format
        let mut api_messages: Vec<OpenAIMessage> = messages
            .into_iter()
            .map(|m| OpenAIMessage {
                role: m.role.to_string(),
                content: Some(m.content),
                tool_calls: None,
                tool_call_id: None,
            })
            .collect();

        // Add tool history messages (previous tool calls and results)
        api_messages.extend(tool_history);

        // Convert tool definitions to OpenAI format
        let openai_tools: Option<Vec<OpenAITool>> = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| OpenAITool {
                        tool_type: "function".to_string(),
                        function: OpenAIFunction {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: json!({
                                "type": t.input_schema.schema_type,
                                "properties": t.input_schema.properties.iter().map(|(k, v)| {
                                    let mut prop = serde_json::Map::new();
                                    prop.insert("type".to_string(), json!(v.schema_type));
                                    prop.insert("description".to_string(), json!(v.description));
                                    if let Some(ref enum_vals) = v.enum_values {
                                        prop.insert("enum".to_string(), json!(enum_vals));
                                    }
                                    if let Some(ref default_val) = v.default {
                                        prop.insert("default".to_string(), default_val.clone());
                                    }
                                    if let Some(ref items) = v.items {
                                        prop.insert("items".to_string(), json!({
                                            "type": items.schema_type,
                                            "description": items.description
                                        }));
                                    }
                                    (k.clone(), Value::Object(prop))
                                }).collect::<serde_json::Map<String, Value>>(),
                                "required": t.input_schema.required
                            }),
                        },
                    })
                    .collect(),
            )
        };

        let request = OpenAICompletionRequest {
            model: self.model.clone(),
            messages: api_messages,
            max_tokens: self.max_tokens,
            tools: openai_tools.clone(),
            tool_choice: if tools.is_empty() { None } else { Some("required".to_string()) },
            stream: None,
        };

        // Debug: Log full request details
        log::info!(
            "[OPENAI] Sending request to {} with model {} and {} tools (x402: {})",
            self.endpoint,
            self.model.as_deref().unwrap_or("(relay default)"),
            openai_tools.as_ref().map(|t| t.len()).unwrap_or(0),
            self.x402_client.is_some()
        );
        log::debug!(
            "[OPENAI] Full request:\n{}",
            serde_json::to_string_pretty(&request).unwrap_or_default()
        );

        // Retry configuration for transient errors
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 2000; // 2 seconds base delay

        let mut last_error: Option<(String, Option<u16>)> = None;
        let mut x402_payment: Option<X402PaymentInfo> = None;
        let mut response_text: Option<String> = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                // Exponential backoff: 2s, 4s, 8s
                let delay_ms = BASE_DELAY_MS * (1 << (attempt - 1));
                let wait_secs = delay_ms / 1000;
                log::warn!(
                    "[OPENAI] Retry attempt {}/{} after {}ms delay",
                    attempt,
                    MAX_RETRIES,
                    delay_ms
                );
                // Emit retry event to frontend
                self.emit_retry_event(
                    attempt,
                    MAX_RETRIES,
                    wait_secs,
                    last_error.as_ref().map(|(m, _)| m.as_str()).unwrap_or("Unknown error"),
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            // Use x402 client if available, otherwise use regular client
            let request_result = if let Some(ref x402) = self.x402_client {
                match x402.post_with_payment(&self.endpoint, &request).await {
                    Ok(x402_response) => {
                        x402_payment = x402_response.payment;
                        Ok(x402_response.response)
                    }
                    Err(e) => Err(format!("x402 request failed: {}", e)),
                }
            } else {
                self.client
                    .post(&self.endpoint)
                    .headers(self.auth_headers.clone())
                    .json(&request)
                    .send()
                    .await
                    .map_err(|e| format!("OpenAI API request failed: {}", e))
            };

            let response = match request_result {
                Ok(r) => r,
                Err(e) => {
                    // Network errors are retryable
                    last_error = Some((e.clone(), None));
                    if attempt < MAX_RETRIES {
                        log::warn!("[OPENAI] Request failed (attempt {}): {}, will retry", attempt + 1, e);
                        continue;
                    }
                    return Err(AiError::new(e));
                }
            };

            let status = response.status();
            let status_code = status.as_u16();

            // Check for retryable status codes: 429 (rate limit), 502, 503, 504 (gateway errors)
            let is_retryable = matches!(status_code, 429 | 502 | 503 | 504);

            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_default();

                // Check if this is a transient 402 error (payment settlement network failure)
                // These contain patterns like "connection failed", "error sending request", etc.
                let is_transient_402 = status_code == 402 && (
                    error_text.contains("connection failed") ||
                    error_text.contains("Connection failed") ||
                    error_text.contains("error sending request") ||
                    error_text.contains("timed out") ||
                    error_text.contains("timeout") ||
                    error_text.contains("temporarily unavailable") ||
                    error_text.contains("network error")
                );

                if (is_retryable || is_transient_402) && attempt < MAX_RETRIES {
                    log::warn!(
                        "[OPENAI] Received retryable status {} (attempt {}), will retry: {}",
                        status,
                        attempt + 1,
                        if error_text.len() > 200 { &error_text[..200] } else { &error_text }
                    );
                    last_error = Some((format!("HTTP {}: {}", status, error_text), Some(status_code)));
                    continue;
                }

                let error_msg = if let Ok(error_response) = serde_json::from_str::<OpenAIErrorResponse>(&error_text) {
                    format!("OpenAI API error: {}", error_response.error.message)
                } else {
                    // Don't include HTML error pages or overly long error bodies
                    let trimmed = error_text.trim_start();
                    let is_html = trimmed.starts_with("<!DOCTYPE")
                        || trimmed.starts_with("<html")
                        || trimmed.starts_with("<HTML");

                    if is_html {
                        // Friendly messages for common gateway errors
                        match status_code {
                            502 => "OpenAI API returned 502 Bad Gateway (provider temporarily unavailable)".to_string(),
                            503 => "OpenAI API returned 503 Service Unavailable (provider temporarily unavailable)".to_string(),
                            504 => "OpenAI API returned 504 Gateway Timeout (provider did not respond in time)".to_string(),
                            _ => format!("OpenAI API returned error status: {} (HTML error page)", status),
                        }
                    } else {
                        // Truncate long text errors
                        let truncated = if error_text.len() > 200 {
                            format!("{}...", &error_text[..200])
                        } else {
                            error_text.clone()
                        };
                        format!("OpenAI API returned error status: {}, body: {}", status, truncated)
                    }
                };

                return Err(AiError::with_status(error_msg, status_code));
            }

            // Success - read response body
            response_text = Some(response
                .text()
                .await
                .map_err(|e| AiError::new(format!("Failed to read OpenAI response: {}", e)))?);
            break;
        }

        // If we exhausted retries without success
        let response_text = response_text.ok_or_else(|| {
            let (msg, code) = last_error.unwrap_or_else(|| ("Max retries exceeded".to_string(), None));
            match code {
                Some(c) => AiError::with_status(msg, c),
                None => AiError::new(msg),
            }
        })?;

        // Debug: Log raw response
        log::debug!("[OPENAI] Raw response:\n{}", response_text);

        let response_data: OpenAICompletionResponse = serde_json::from_str(&response_text)
            .map_err(|e| AiError::new(format!("Failed to parse OpenAI response: {} - body: {}", e, response_text)))?;

        let choice = response_data
            .choices
            .first()
            .ok_or_else(|| "OpenAI API returned no choices".to_string())?;

        // Debug: Log parsed response
        log::info!(
            "[OPENAI] Response - content_len: {}, tool_calls: {}, finish_reason: {:?}",
            choice.message.content.as_ref().map(|c| c.len()).unwrap_or(0),
            choice.message.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
            choice.finish_reason
        );

        let content = choice.message.content.clone().unwrap_or_default();
        let finish_reason = choice.finish_reason.clone();

        // Convert tool calls if present
        let tool_calls: Vec<ToolCall> = choice
            .message
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|tc| {
                        let args: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(json!({}));
                        Some(ToolCall {
                            id: tc.id.clone(),
                            name: tc.function.name.clone(),
                            arguments: args,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let is_tool_use = finish_reason.as_deref() == Some("tool_calls") || !tool_calls.is_empty();

        Ok(AiResponse {
            content,
            tool_calls,
            stop_reason: if is_tool_use {
                Some("tool_use".to_string())
            } else {
                Some("end_turn".to_string())
            },
            x402_payment,
        })
    }

    /// Build tool result messages for continuing after tool execution
    pub fn build_tool_result_messages(
        tool_calls: &[ToolCall],
        tool_responses: &[crate::ai::ToolResponse],
    ) -> Vec<OpenAIMessage> {
        let mut messages = Vec::new();

        // First, add the assistant message with tool calls
        let openai_tool_calls: Vec<OpenAIToolCall> = tool_calls
            .iter()
            .map(|tc| OpenAIToolCall {
                id: tc.id.clone(),
                call_type: "function".to_string(),
                function: OpenAIFunctionCall {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                },
            })
            .collect();

        messages.push(OpenAIMessage {
            role: "assistant".to_string(),
            content: Some("\n".to_string()), // Must be non-empty: Kimi rejects "", MiniMax/litellm rejects omitted field
            tool_calls: Some(openai_tool_calls),
            tool_call_id: None,
        });

        // Then add the tool results
        for response in tool_responses {
            messages.push(OpenAIMessage {
                role: "tool".to_string(),
                content: Some(response.content.clone()),
                tool_calls: None,
                tool_call_id: Some(response.tool_call_id.clone()),
            });
        }

        messages
    }

    /// Generate response with streaming support
    ///
    /// Sends stream events through the provided sender as they arrive.
    /// Returns the final accumulated response.
    pub async fn generate_with_tools_streaming(
        &self,
        messages: Vec<Message>,
        tool_history: Vec<OpenAIMessage>,
        tools: Vec<ToolDefinition>,
        stream_sender: StreamSender,
    ) -> Result<AiResponse, String> {
        // Convert messages to OpenAI format
        let mut api_messages: Vec<OpenAIMessage> = messages
            .into_iter()
            .map(|m| OpenAIMessage {
                role: m.role.to_string(),
                content: Some(m.content),
                tool_calls: None,
                tool_call_id: None,
            })
            .collect();

        // Add tool history messages
        api_messages.extend(tool_history);

        // Convert tool definitions to OpenAI format
        let openai_tools: Option<Vec<OpenAITool>> = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| OpenAITool {
                        tool_type: "function".to_string(),
                        function: OpenAIFunction {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: json!({
                                "type": t.input_schema.schema_type,
                                "properties": t.input_schema.properties.iter().map(|(k, v)| {
                                    let mut prop = serde_json::Map::new();
                                    prop.insert("type".to_string(), json!(v.schema_type));
                                    prop.insert("description".to_string(), json!(v.description));
                                    if let Some(ref enum_vals) = v.enum_values {
                                        prop.insert("enum".to_string(), json!(enum_vals));
                                    }
                                    if let Some(ref default_val) = v.default {
                                        prop.insert("default".to_string(), default_val.clone());
                                    }
                                    if let Some(ref items) = v.items {
                                        prop.insert("items".to_string(), json!({
                                            "type": items.schema_type,
                                            "description": items.description
                                        }));
                                    }
                                    (k.clone(), Value::Object(prop))
                                }).collect::<serde_json::Map<String, Value>>(),
                                "required": t.input_schema.required
                            }),
                        },
                    })
                    .collect(),
            )
        };

        let request = OpenAICompletionRequest {
            model: self.model.clone(),
            messages: api_messages,
            max_tokens: self.max_tokens,
            tools: openai_tools.clone(),
            tool_choice: if tools.is_empty() { None } else { Some("required".to_string()) },
            stream: Some(true),
        };

        log::info!(
            "[OPENAI] Streaming request to {} with model {} and {} tools",
            self.endpoint,
            self.model.as_deref().unwrap_or("(relay default)"),
            openai_tools.as_ref().map(|t| t.len()).unwrap_or(0),
        );

        // Retry configuration for transient errors
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 2000;

        let mut last_error: Option<String> = None;
        let mut response_opt: Option<reqwest::Response> = None;

        // Note: x402 streaming not yet supported, fall back to regular client
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let delay_ms = BASE_DELAY_MS * (1 << (attempt - 1));
                let wait_secs = delay_ms / 1000;
                log::warn!(
                    "[OPENAI] Streaming retry attempt {}/{} after {}ms delay",
                    attempt,
                    MAX_RETRIES,
                    delay_ms
                );
                // Emit retry event to frontend
                self.emit_retry_event(
                    attempt,
                    MAX_RETRIES,
                    wait_secs,
                    last_error.as_deref().unwrap_or("Unknown error"),
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            let request_result = self.client
                .post(&self.endpoint)
                .headers(self.auth_headers.clone())
                .json(&request)
                .send()
                .await;

            let response = match request_result {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(format!("OpenAI API streaming request failed: {}", e));
                    if attempt < MAX_RETRIES {
                        log::warn!("[OPENAI] Streaming request failed (attempt {}): {}, will retry", attempt + 1, e);
                        continue;
                    }
                    let _ = stream_sender.send(StreamEvent::Error {
                        message: format!("Request failed after {} retries: {}", MAX_RETRIES, e),
                        code: None,
                    }).await;
                    return Err(last_error.unwrap());
                }
            };

            let status = response.status();
            let status_code = status.as_u16();
            let is_retryable = matches!(status_code, 429 | 502 | 503 | 504);

            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_default();

                // Check if this is a transient 402 error (payment settlement network failure)
                let is_transient_402 = status_code == 402 && (
                    error_text.contains("connection failed") ||
                    error_text.contains("Connection failed") ||
                    error_text.contains("error sending request") ||
                    error_text.contains("timed out") ||
                    error_text.contains("timeout") ||
                    error_text.contains("temporarily unavailable") ||
                    error_text.contains("network error")
                );

                if (is_retryable || is_transient_402) && attempt < MAX_RETRIES {
                    log::warn!(
                        "[OPENAI] Streaming received retryable status {} (attempt {}), will retry",
                        status,
                        attempt + 1
                    );
                    last_error = Some(format!("HTTP {}: {}", status, error_text));
                    continue;
                }

                let _ = stream_sender.send(StreamEvent::Error {
                    message: format!("OpenAI API error: {}", error_text),
                    code: Some(status_code.to_string()),
                }).await;
                return Err(format!("OpenAI API returned error status: {}", status));
            }

            response_opt = Some(response);
            break;
        }

        let response = response_opt.ok_or_else(|| {
            last_error.unwrap_or_else(|| "Max retries exceeded".to_string())
        })?;

        // Process SSE stream
        let mut stream = response.bytes_stream();
        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut partial_tool_calls: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new(); // index -> (id, name, arguments)
        let mut finish_reason: Option<String> = None;
        let mut usage: Option<(u32, u32)> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| format!("Stream read error: {}", e))?;

            let chunk_str = String::from_utf8_lossy(&chunk);

            // Parse SSE format (data: {...}\n\n)
            for line in chunk_str.lines() {
                let line = line.trim();
                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }

                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(chunk_data) = serde_json::from_str::<OpenAIStreamChunk>(json_str) {
                        for choice in chunk_data.choices {
                            // Handle content delta
                            if let Some(delta_content) = choice.delta.content {
                                content.push_str(&delta_content);
                                let _ = stream_sender.send(StreamEvent::ContentDelta {
                                    content: delta_content,
                                    index: choice.index,
                                }).await;
                            }

                            // Handle tool call deltas
                            if let Some(tool_call_deltas) = choice.delta.tool_calls {
                                for tc_delta in tool_call_deltas {
                                    let idx = tc_delta.index;
                                    let entry = partial_tool_calls.entry(idx).or_insert_with(|| {
                                        (String::new(), String::new(), String::new())
                                    });

                                    // Update ID if present
                                    if let Some(id) = tc_delta.id {
                                        entry.0 = id.clone();
                                        if let Some(ref func) = tc_delta.function {
                                            if let Some(ref name) = func.name {
                                                entry.1 = name.clone();
                                                let _ = stream_sender.send(StreamEvent::ToolCallStart {
                                                    id: entry.0.clone(),
                                                    name: name.clone(),
                                                    index: idx,
                                                }).await;
                                            }
                                        }
                                    }

                                    // Update function details
                                    if let Some(ref func) = tc_delta.function {
                                        if let Some(ref name) = func.name {
                                            if entry.1.is_empty() {
                                                entry.1 = name.clone();
                                            }
                                        }
                                        if let Some(ref args) = func.arguments {
                                            entry.2.push_str(args);
                                            let _ = stream_sender.send(StreamEvent::ToolCallDelta {
                                                id: entry.0.clone(),
                                                arguments_delta: args.clone(),
                                                index: idx,
                                            }).await;
                                        }
                                    }
                                }
                            }

                            // Handle finish reason
                            if let Some(reason) = choice.finish_reason {
                                finish_reason = Some(reason);
                            }
                        }

                        // Capture usage if present
                        if let Some(u) = chunk_data.usage {
                            usage = Some((
                                u.prompt_tokens.unwrap_or(0),
                                u.completion_tokens.unwrap_or(0),
                            ));
                        }
                    }
                }
            }
        }

        // Convert partial tool calls to complete ones
        for (idx, (id, name, args)) in partial_tool_calls {
            if !id.is_empty() && !name.is_empty() {
                let arguments: Value = serde_json::from_str(&args).unwrap_or(json!({}));

                let _ = stream_sender.send(StreamEvent::ToolCallComplete {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                    index: idx,
                }).await;

                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }

        // Send done event
        let _ = stream_sender.send(StreamEvent::Done {
            stop_reason: finish_reason.clone(),
            usage: usage.map(|(input, output)| crate::ai::streaming::StreamUsage {
                input_tokens: input,
                output_tokens: output,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        }).await;

        let is_tool_use = finish_reason.as_deref() == Some("tool_calls") || !tool_calls.is_empty();

        Ok(AiResponse {
            content,
            tool_calls,
            stop_reason: if is_tool_use {
                Some("tool_use".to_string())
            } else {
                Some("end_turn".to_string())
            },
            x402_payment: None, // Streaming doesn't support x402 yet
        })
    }
}
