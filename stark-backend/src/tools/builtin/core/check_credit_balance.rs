use crate::tools::registry::Tool;
use crate::tools::types::{
    ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult, ToolSafetyLevel,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for checking the bot's AI credit balance
pub struct CheckCreditBalanceTool {
    definition: ToolDefinition,
}

impl CheckCreditBalanceTool {
    pub fn new() -> Self {
        CheckCreditBalanceTool {
            definition: ToolDefinition {
                name: "check_credit_balance".to_string(),
                description: "Check the current AI credit balance. Returns how many credits are available for AI inference calls.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: HashMap::new(),
                    required: vec![],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for CheckCreditBalanceTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CheckCreditBalanceTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }

    async fn execute(&self, _params: Value, context: &ToolContext) -> ToolResult {
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp.clone(),
            None => return ToolResult::error("No wallet configured — cannot check credit balance"),
        };

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        // Determine the inference endpoint base URL from active settings
        let base_url = match db.get_active_agent_settings() {
            Ok(Some(settings)) if settings.endpoint.contains("defirelay.com") => {
                if let Some(idx) = settings.endpoint.find("/api/") {
                    settings.endpoint[..idx].to_string()
                } else if let Some(idx) = settings.endpoint.find("/chat") {
                    settings.endpoint[..idx].to_string()
                } else {
                    settings.endpoint.trim_end_matches('/').to_string()
                }
            }
            _ => "https://inference.defirelay.com".to_string(),
        };

        // Sign the request with ERC-8128
        let signer = crate::erc8128::Erc8128Signer::new(wallet_provider, 8453);
        let authority = base_url
            .strip_prefix("https://")
            .or_else(|| base_url.strip_prefix("http://"))
            .unwrap_or(&base_url)
            .split('/')
            .next()
            .unwrap_or("inference.defirelay.com");

        let signed = match signer
            .sign_request("GET", authority, "/credits/balance", None, None)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to sign credits balance request: {}", e);
                return ToolResult::error("Failed to sign credit balance request");
            }
        };

        // Make the request
        let url = format!("{}/credits/balance", base_url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut req_builder = client.get(&url);
        req_builder = req_builder.header("signature-input", &signed.signature_input);
        req_builder = req_builder.header("signature", &signed.signature);
        if let Some(ref digest) = signed.content_digest {
            req_builder = req_builder.header("content-digest", digest);
        }

        match req_builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                match resp.text().await {
                    Ok(body) => {
                        if status.is_success() {
                            match serde_json::from_str::<Value>(&body) {
                                Ok(json) => {
                                    let credits = json.get("credits")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0);

                                    ToolResult::success(format!(
                                        "Credit balance: ${:.4}",
                                        credits
                                    ))
                                    .with_metadata(json)
                                }
                                Err(_) => ToolResult::error("Invalid response from credits service"),
                            }
                        } else {
                            ToolResult::error(format!("Credits service returned {}", status))
                        }
                    }
                    Err(e) => ToolResult::error(format!("Failed to read response: {}", e)),
                }
            }
            Err(e) => ToolResult::error(format!("Failed to connect to credits service: {}", e)),
        }
    }
}
