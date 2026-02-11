//! Ask user tool for requesting clarification or additional information
//!
//! This tool allows the agent to ask the user questions when more information
//! is needed to complete a task. Use this for network selection, confirmation,
//! or any ambiguous requests.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Ask user tool for requesting clarification
pub struct AskUserTool {
    definition: ToolDefinition,
}

impl AskUserTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "question".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The question to ask the user. Be clear and specific.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "options".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Optional list of choices to present to the user (e.g., ['base', 'mainnet', 'arbitrum']). If provided, the user can select from these options.".to_string(),
                default: None,
                items: Some(Box::new(PropertySchema {
                    schema_type: "string".to_string(),
                    description: "An option choice".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "context".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional context explaining why you need this information.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "default".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional default value if the user doesn't specify.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        AskUserTool {
            definition: ToolDefinition {
                name: "ask_user".to_string(),
                description: "LAST RESORT: Ask the user for clarification. BEFORE using this tool, you MUST first try: (1) use_skill to find a relevant skill, (2) token_lookup to find token addresses, (3) other tools that might have the information. Only use ask_user when you genuinely cannot find the information any other way.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["question".to_string()],
                },
                group: ToolGroup::System,
            },
        }
    }
}

impl Default for AskUserTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct AskUserParams {
    question: String,
    options: Option<Vec<String>>,
    context: Option<String>,
    default: Option<String>,
}

#[async_trait]
impl Tool for AskUserTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let params: AskUserParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Format the question for display
        let mut output = String::new();

        // Add context if provided
        if let Some(ctx) = &params.context {
            output.push_str(&format!("📋 {}\n\n", ctx));
        }

        // Add the main question
        output.push_str(&format!("❓ **{}**\n", params.question));

        // Add options if provided
        if let Some(options) = &params.options {
            output.push_str("\nOptions:\n");
            for (i, opt) in options.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, opt));
            }
        }

        // Add default if provided
        if let Some(default) = &params.default {
            output.push_str(&format!("\n(Default: {})\n", default));
        }

        // Return as a special result that indicates user input is needed
        // The waiting instruction goes in metadata, not visible output
        ToolResult::success(output)
            .with_metadata(json!({
                "requires_user_response": true,
                "instruction": "WAIT for the user's response before taking any action. Do not answer the question yourself.",
                "question": params.question,
                "options": params.options,
                "default": params.default
            }))
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}
