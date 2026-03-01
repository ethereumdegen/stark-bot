use crate::tools::registry::Tool;
use crate::tools::types::{
    ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for reading the bot's current operating mode (rogue vs partner)
pub struct ReadOperatingModeTool {
    definition: ToolDefinition,
}

impl ReadOperatingModeTool {
    pub fn new() -> Self {
        ReadOperatingModeTool {
            definition: ToolDefinition {
                name: "read_operating_mode".to_string(),
                description: "Read the current operating mode (rogue or partner). Rogue mode allows autonomous actions; partner mode requires user confirmation for sensitive operations.".to_string(),
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

impl Default for ReadOperatingModeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReadOperatingModeTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, _params: Value, _context: &ToolContext) -> ToolResult {
        use crate::models::bot_config::OperatingMode;

        let config = crate::models::BotConfig::load();
        let rogue_mode_enabled = config.operating_mode.is_rogue();

        let mode = match config.operating_mode {
            OperatingMode::Rogue => "rogue",
            OperatingMode::Partner => "partner",
        };

        ToolResult::success(format!(
            "Operating mode: {}\n\nRogue mode {}: {}",
            mode,
            if rogue_mode_enabled { "ENABLED" } else { "DISABLED" },
            if rogue_mode_enabled {
                "The bot can take autonomous actions without user confirmation."
            } else {
                "The bot operates in partner mode, requiring user confirmation for sensitive operations."
            }
        ))
        .with_metadata(json!({
            "mode": mode,
            "rogue_mode_enabled": rogue_mode_enabled
        }))
    }
}
