use crate::ai::types::{AiResponse, ToolCall};
use crate::ai::Message;
use crate::tools::ToolDefinition;
use crate::x402::{X402Client, X402PaymentInfo, is_x402_endpoint};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct OpenAIClient {
    client: Client,
    endpoint: String,
    model: String,
    max_tokens: u32,
    x402_client: Option<Arc<X402Client>>,
}

#[derive(Debug, Serialize)]
struct OpenAICompletionRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
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

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        // Only add auth header if API key is provided and not empty
        if !api_key.is_empty() {
            let auth_value = header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| format!("Invalid API key format: {}", e))?;
            headers.insert(header::AUTHORIZATION, auth_value);
        }

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        // Create x402 client if private key is provided and endpoint uses x402
        let x402_client = if is_x402_endpoint(&endpoint_url) {
            if let Some(pk) = burner_private_key {
                if !pk.is_empty() {
                    match X402Client::new(pk) {
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

        // Determine model with smart defaults based on endpoint
        let model_name = match model {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => {
                // Use endpoint-specific defaults for known services
                if endpoint_url.contains("defirelay.com") {
                    // defirelay endpoints use "default" as model name
                    "default".to_string()
                } else {
                    "gpt-4o".to_string()
                }
            }
        };

        Ok(Self {
            client,
            endpoint: endpoint_url,
            model: model_name,
            max_tokens: max_tokens.unwrap_or(40000),
            x402_client,
        })
    }

    pub async fn generate_text(&self, messages: Vec<Message>) -> Result<String, String> {
        let response = self.generate_with_tools_internal(messages, vec![], vec![]).await?;
        Ok(response.content)
    }

    /// Generate text and return payment info if x402 payment was made
    pub async fn generate_text_with_payment_info(&self, messages: Vec<Message>) -> Result<(String, Option<X402PaymentInfo>), String> {
        let response = self.generate_with_tools_internal(messages, vec![], vec![]).await?;
        Ok((response.content, response.x402_payment))
    }

    pub async fn generate_with_tools(
        &self,
        messages: Vec<Message>,
        tool_history: Vec<OpenAIMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<AiResponse, String> {
        self.generate_with_tools_internal(messages, tool_history, tools).await
    }

    async fn generate_with_tools_internal(
        &self,
        messages: Vec<Message>,
        tool_history: Vec<OpenAIMessage>,
        tools: Vec<ToolDefinition>,
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
            tool_choice: if tools.is_empty() { None } else { Some("auto".to_string()) },
        };

        // Debug: Log full request details
        log::info!(
            "[OPENAI] Sending request to {} with model {} and {} tools (x402: {})",
            self.endpoint,
            self.model,
            openai_tools.as_ref().map(|t| t.len()).unwrap_or(0),
            self.x402_client.is_some()
        );
        log::debug!(
            "[OPENAI] Full request:\n{}",
            serde_json::to_string_pretty(&request).unwrap_or_default()
        );

        // Use x402 client if available, otherwise use regular client
        let (response, x402_payment) = if let Some(ref x402) = self.x402_client {
            let x402_response = x402.post_with_payment(&self.endpoint, &request)
                .await
                .map_err(|e| format!("x402 request failed: {}", e))?;
            (x402_response.response, x402_response.payment)
        } else {
            let resp = self.client
                .post(&self.endpoint)
                .json(&request)
                .send()
                .await
                .map_err(|e| format!("OpenAI API request failed: {}", e))?;
            (resp, None)
        };

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();

            if let Ok(error_response) = serde_json::from_str::<OpenAIErrorResponse>(&error_text) {
                return Err(format!("OpenAI API error: {}", error_response.error.message));
            }

            return Err(format!(
                "OpenAI API returned error status: {}, body: {}",
                status, error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read OpenAI response: {}", e))?;

        // Debug: Log raw response
        log::debug!("[OPENAI] Raw response:\n{}", response_text);

        let response_data: OpenAICompletionResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse OpenAI response: {} - body: {}", e, response_text))?;

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
            content: Some("".to_string()), // Kimi requires content field even if empty
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
}
