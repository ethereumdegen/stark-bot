use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for the agent to modify its own soul document
pub struct ModifySoulTool {
    definition: ToolDefinition,
}

impl ModifySoulTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action to perform: 'read' to view current soul, 'append' to add content at the end, 'replace_section' to replace a specific section".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["read".to_string(), "append".to_string(), "replace_section".to_string()]),
            },
        );
        properties.insert(
            "section".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Section header to replace (for replace_section action, e.g., 'Core Truths' or '## Core Truths')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "content".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Content to append or use as replacement for the section".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ModifySoulTool {
            definition: ToolDefinition {
                name: "modify_soul".to_string(),
                description: "Modify your soul document (SOUL.md) to update your personality, add new truths, or refine your identity. Use this to evolve your understanding of yourself.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for ModifySoulTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ModifySoulParams {
    action: String,
    section: Option<String>,
    content: Option<String>,
}

/// Get the path to SOUL.md in the workspace
/// The agent can only modify the workspace copy, not the original
fn soul_path() -> std::path::PathBuf {
    crate::config::soul_document_path()
}

#[async_trait]
impl Tool for ModifySoulTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ModifySoulParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let path = soul_path();

        match params.action.as_str() {
            "read" => {
                // Read and return current soul content
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => ToolResult::success(content).with_metadata(json!({
                        "action": "read",
                        "path": path.display().to_string()
                    })),
                    Err(e) => ToolResult::error(format!("Failed to read SOUL.md: {}", e)),
                }
            }
            "append" => {
                // Append content to the end
                let content = match params.content {
                    Some(c) => c,
                    None => return ToolResult::error("Content is required for append action"),
                };

                let current = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(e) => return ToolResult::error(format!("Failed to read SOUL.md: {}", e)),
                };

                // Check disk quota for the new content
                if let Err(e) = context.check_disk_quota(content.len()) {
                    return ToolResult::error(e);
                }

                // Append with proper spacing
                let new_content = if current.ends_with('\n') {
                    format!("{}\n{}", current, content)
                } else {
                    format!("{}\n\n{}", current, content)
                };

                match tokio::fs::write(&path, &new_content).await {
                    Ok(_) => {
                        context.record_disk_write(content.len());
                        log::info!("Soul document updated (append)");
                        ToolResult::success("Successfully appended content to soul document").with_metadata(json!({
                            "action": "append",
                            "added_length": content.len()
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to write SOUL.md: {}", e)),
                }
            }
            "replace_section" => {
                // Replace a specific section
                let section = match params.section {
                    Some(s) => s,
                    None => return ToolResult::error("Section is required for replace_section action"),
                };
                let content = match params.content {
                    Some(c) => c,
                    None => return ToolResult::error("Content is required for replace_section action"),
                };

                let current = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(e) => return ToolResult::error(format!("Failed to read SOUL.md: {}", e)),
                };

                // Normalize section header (support both "Core Truths" and "## Core Truths")
                let section_header = if section.starts_with('#') {
                    section.clone()
                } else {
                    format!("## {}", section)
                };

                // Find the section and replace it
                let lines: Vec<&str> = current.lines().collect();
                let mut new_lines: Vec<String> = Vec::new();
                let mut in_target_section = false;
                let mut found_section = false;
                let mut section_replaced = false;

                for line in lines {
                    if line.starts_with("## ") || line.starts_with("# ") {
                        if in_target_section {
                            // We were in the target section, now we've hit the next section
                            // The replacement content was already added
                            in_target_section = false;
                        }

                        // Check if this is our target section
                        let normalized = if line.starts_with("## ") {
                            line.to_string()
                        } else {
                            format!("## {}", line.trim_start_matches("# "))
                        };

                        if normalized.trim() == section_header.trim() ||
                           line.trim() == section_header.trim() ||
                           line.trim().trim_start_matches('#').trim() == section.trim() {
                            found_section = true;
                            in_target_section = true;
                            // Add the section header and new content
                            new_lines.push(line.to_string());
                            new_lines.push(String::new()); // Empty line after header
                            for content_line in content.lines() {
                                new_lines.push(content_line.to_string());
                            }
                            new_lines.push(String::new()); // Empty line after content
                            section_replaced = true;
                            continue;
                        }
                    }

                    if !in_target_section {
                        new_lines.push(line.to_string());
                    }
                }

                if !found_section {
                    return ToolResult::error(format!("Section '{}' not found in soul document", section));
                }

                let new_content = new_lines.join("\n");

                // Check disk quota for net size increase
                let size_increase = new_content.len().saturating_sub(current.len());
                if size_increase > 0 {
                    if let Err(e) = context.check_disk_quota(size_increase) {
                        return ToolResult::error(e);
                    }
                }

                match tokio::fs::write(&path, &new_content).await {
                    Ok(_) => {
                        if size_increase > 0 {
                            context.record_disk_write(size_increase);
                        }
                        log::info!("Soul document updated (replace_section: {})", section);
                        ToolResult::success(format!("Successfully replaced section '{}' in soul document", section)).with_metadata(json!({
                            "action": "replace_section",
                            "section": section,
                            "replaced": section_replaced
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to write SOUL.md: {}", e)),
                }
            }
            _ => ToolResult::error(format!("Unknown action: {}. Use 'read', 'append', or 'replace_section'", params.action)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_read_soul() {
        let tool = ModifySoulTool::new();
        // This test would need a mock or actual SOUL.md file
        // For now, just verify the tool can be created
        assert_eq!(tool.definition().name, "modify_soul");
    }
}
