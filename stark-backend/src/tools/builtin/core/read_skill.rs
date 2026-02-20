//! Read skill tool — returns the full raw markdown of a locally installed skill
//!
//! This tool is HIDDEN by default and only becomes available when a skill
//! that requires it (e.g., starkhub) is activated. It reconstructs the complete
//! SKILL.md markdown (frontmatter + body) from the database, ready for submission
//! or inspection.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

pub struct ReadSkillTool {
    definition: ToolDefinition,
}

impl ReadSkillTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Name of the skill to read".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ReadSkillTool {
            definition: ToolDefinition {
                name: "read_skill".to_string(),
                description: "Read the full raw markdown of a locally installed skill, including YAML frontmatter. Returns the complete SKILL.md content ready for submission or inspection.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["name".to_string()],
                },
                group: ToolGroup::System,
                hidden: true, // Only visible when a skill (e.g. starkhub) requires it
            },
        }
    }
}

impl Default for ReadSkillTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ReadSkillParams {
    name: String,
}

#[async_trait]
impl Tool for ReadSkillTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ReadSkillParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let registry = match &context.skill_registry {
            Some(r) => r,
            None => return ToolResult::error("Skill registry not available"),
        };

        let skill = match registry.get(&params.name) {
            Some(s) => s,
            None => return ToolResult::error(format!("Skill '{}' not found", params.name)),
        };

        // Reconstruct full SKILL.md with YAML frontmatter
        let mut frontmatter = String::new();
        frontmatter.push_str("---\n");
        frontmatter.push_str(&format!("name: {}\n", skill.metadata.name));
        frontmatter.push_str(&format!(
            "description: \"{}\"\n",
            skill.metadata.description.replace('"', "\\\"")
        ));
        frontmatter.push_str(&format!("version: {}\n", skill.metadata.version));

        if let Some(ref author) = skill.metadata.author {
            frontmatter.push_str(&format!("author: {}\n", author));
        }
        if let Some(ref homepage) = skill.metadata.homepage {
            frontmatter.push_str(&format!("homepage: {}\n", homepage));
        }
        if let Some(ref metadata) = skill.metadata.metadata {
            frontmatter.push_str(&format!("metadata: {}\n", metadata));
        }

        if !skill.metadata.requires_tools.is_empty() {
            frontmatter.push_str(&format!(
                "requires_tools: [{}]\n",
                skill.metadata.requires_tools.join(", ")
            ));
        }
        if !skill.metadata.requires_binaries.is_empty() {
            frontmatter.push_str(&format!(
                "requires_binaries: [{}]\n",
                skill.metadata.requires_binaries.join(", ")
            ));
        }
        if !skill.metadata.tags.is_empty() {
            frontmatter.push_str(&format!(
                "tags: [{}]\n",
                skill.metadata.tags.join(", ")
            ));
        }

        if !skill.metadata.arguments.is_empty() {
            frontmatter.push_str("arguments:\n");
            for (arg_name, arg_def) in &skill.metadata.arguments {
                frontmatter.push_str(&format!("  {}:\n", arg_name));
                frontmatter.push_str(&format!(
                    "    description: \"{}\"\n",
                    arg_def.description.replace('"', "\\\"")
                ));
                if arg_def.required {
                    frontmatter.push_str("    required: true\n");
                } else {
                    frontmatter.push_str("    required: false\n");
                }
                if let Some(ref default) = arg_def.default {
                    frontmatter.push_str(&format!(
                        "    default: \"{}\"\n",
                        default.replace('"', "\\\"")
                    ));
                }
            }
        }

        if let Some(ref scripts) = skill.metadata.scripts {
            if !scripts.is_empty() {
                frontmatter.push_str(&format!(
                    "scripts: [{}]\n",
                    scripts.join(", ")
                ));
            }
        }
        if let Some(ref abis) = skill.metadata.abis {
            if !abis.is_empty() {
                frontmatter.push_str(&format!(
                    "abis: [{}]\n",
                    abis.join(", ")
                ));
            }
        }
        if let Some(ref presets_file) = skill.metadata.presets_file {
            frontmatter.push_str(&format!("presets_file: {}\n", presets_file));
        }

        if let Some(ref subagent_type) = skill.metadata.subagent_type {
            frontmatter.push_str(&format!("sets_agent_subtype: {}\n", subagent_type));
        }

        frontmatter.push_str("---\n");

        let full_markdown = format!("{}\n{}", frontmatter, skill.prompt_template);

        ToolResult::success(full_markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = ReadSkillTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "read_skill");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.hidden, "read_skill should be hidden by default");
    }
}
