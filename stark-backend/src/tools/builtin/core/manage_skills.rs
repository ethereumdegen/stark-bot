//! Skill management tool for listing, installing, and managing skills
//!
//! This tool allows the agent to manage skills dynamically:
//! - List available skills with filtering
//! - Install skills from markdown content or URLs
//! - Enable/disable skills
//! - Delete skills
//! - Search skills by name or tag

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for managing skills in the skill registry
pub struct ManageSkillsTool {
    definition: ToolDefinition,
}

impl ManageSkillsTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The action to perform: 'list', 'get', 'install', 'update', 'enable', 'disable', 'delete', or 'search'. Use 'update' to modify an existing skill (bypasses version checks).".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list".to_string(),
                    "get".to_string(),
                    "install".to_string(),
                    "update".to_string(),
                    "enable".to_string(),
                    "disable".to_string(),
                    "delete".to_string(),
                    "search".to_string(),
                ]),
            },
        );

        properties.insert(
            "name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Skill name (required for get, enable, disable, delete)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "URL to fetch skill markdown from (for install action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "markdown".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Raw markdown content to install as a skill (for install action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query for finding skills by name, description, or tag (for search action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "filter_enabled".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, only list enabled skills (for list action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ManageSkillsTool {
            definition: ToolDefinition {
                name: "manage_skills".to_string(),
                description: "Manage skills: list, install, enable/disable, delete, or search. Use this to dynamically add new capabilities.".to_string(),
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

impl Default for ManageSkillsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ManageSkillsParams {
    action: String,
    name: Option<String>,
    url: Option<String>,
    markdown: Option<String>,
    query: Option<String>,
    filter_enabled: Option<bool>,
}

#[async_trait]
impl Tool for ManageSkillsTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ManageSkillsParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let registry = match &context.skill_registry {
            Some(r) => r,
            None => return ToolResult::error("Skill registry not available"),
        };

        match params.action.as_str() {
            "list" => {
                let skills = if params.filter_enabled.unwrap_or(false) {
                    registry.list_enabled()
                } else {
                    registry.list()
                };

                let skill_list: Vec<Value> = skills
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.metadata.name,
                            "description": s.metadata.description,
                            "version": s.metadata.version,
                            "enabled": s.enabled,
                            "tags": s.metadata.tags,
                        })
                    })
                    .collect();

                ToolResult::success(serde_json::to_string_pretty(&skill_list).unwrap_or_default())
                    .with_metadata(json!({
                        "count": skill_list.len(),
                        "filter_enabled": params.filter_enabled.unwrap_or(false)
                    }))
            }

            "get" => {
                let name = match params.name {
                    Some(n) => n,
                    None => return ToolResult::error("'name' parameter is required for 'get' action"),
                };

                match registry.get(&name) {
                    Some(skill) => {
                        let detail = json!({
                            "name": skill.metadata.name,
                            "description": skill.metadata.description,
                            "version": skill.metadata.version,
                            "author": skill.metadata.author,
                            "homepage": skill.metadata.homepage,
                            "enabled": skill.enabled,
                            "tags": skill.metadata.tags,
                            "requires_tools": skill.metadata.requires_tools,
                            "requires_binaries": skill.metadata.requires_binaries,
                            "prompt_template": skill.prompt_template,
                        });
                        ToolResult::success(serde_json::to_string_pretty(&detail).unwrap_or_default())
                    }
                    None => ToolResult::error(format!("Skill '{}' not found", name)),
                }
            }

            "install" => {
                // Install from URL or markdown content
                let markdown_content = if let Some(url) = params.url {
                    // Fetch markdown from URL
                    match fetch_markdown_from_url(&url, &context.http_client()).await {
                        Ok(content) => content,
                        Err(e) => return ToolResult::error(format!("Failed to fetch skill from URL: {}", e)),
                    }
                } else if let Some(md) = params.markdown {
                    md
                } else {
                    return ToolResult::error("Either 'url' or 'markdown' parameter is required for 'install' action");
                };

                match registry.create_skill_from_markdown(&markdown_content) {
                    Ok(skill) => {
                        let result = json!({
                            "success": true,
                            "message": format!("Skill '{}' installed successfully", skill.name),
                            "skill": {
                                "name": skill.name,
                                "description": skill.description,
                                "version": skill.version,
                                "enabled": skill.enabled,
                            }
                        });
                        ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
                    }
                    Err(e) => ToolResult::error(format!("Failed to install skill: {}", e)),
                }
            }

            "update" => {
                // Update an existing skill (bypasses version checks)
                let markdown_content = if let Some(md) = params.markdown {
                    md
                } else {
                    return ToolResult::error("'markdown' parameter is required for 'update' action");
                };

                match registry.create_skill_from_markdown_force(&markdown_content) {
                    Ok(skill) => {
                        let result = json!({
                            "success": true,
                            "message": format!("Skill '{}' updated successfully", skill.name),
                            "skill": {
                                "name": skill.name,
                                "description": skill.description,
                                "version": skill.version,
                                "enabled": skill.enabled,
                            }
                        });
                        ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
                    }
                    Err(e) => ToolResult::error(format!("Failed to update skill: {}", e)),
                }
            }

            "enable" => {
                let name = match params.name {
                    Some(n) => n,
                    None => return ToolResult::error("'name' parameter is required for 'enable' action"),
                };

                if !registry.has_skill(&name) {
                    return ToolResult::error(format!("Skill '{}' not found", name));
                }

                registry.set_enabled(&name, true);
                ToolResult::success(format!("Skill '{}' enabled", name))
            }

            "disable" => {
                let name = match params.name {
                    Some(n) => n,
                    None => return ToolResult::error("'name' parameter is required for 'disable' action"),
                };

                if !registry.has_skill(&name) {
                    return ToolResult::error(format!("Skill '{}' not found", name));
                }

                registry.set_enabled(&name, false);
                ToolResult::success(format!("Skill '{}' disabled", name))
            }

            "delete" => {
                let name = match params.name {
                    Some(n) => n,
                    None => return ToolResult::error("'name' parameter is required for 'delete' action"),
                };

                match registry.delete_skill(&name) {
                    Ok(true) => ToolResult::success(format!("Skill '{}' deleted", name)),
                    Ok(false) => ToolResult::error(format!("Skill '{}' not found", name)),
                    Err(e) => ToolResult::error(format!("Failed to delete skill: {}", e)),
                }
            }

            "search" => {
                let query = match params.query {
                    Some(q) => q,
                    None => return ToolResult::error("'query' parameter is required for 'search' action"),
                };

                let results = registry.search(&query);
                let skill_list: Vec<Value> = results
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.metadata.name,
                            "description": s.metadata.description,
                            "tags": s.metadata.tags,
                            "enabled": s.enabled,
                        })
                    })
                    .collect();

                ToolResult::success(serde_json::to_string_pretty(&skill_list).unwrap_or_default())
                    .with_metadata(json!({
                        "query": query,
                        "count": skill_list.len()
                    }))
            }

            _ => ToolResult::error(format!("Unknown action: '{}'. Valid actions: list, get, install, update, enable, disable, delete, search", params.action)),
        }
    }
}

/// Fetch markdown content from a URL
async fn fetch_markdown_from_url(url: &str, client: &reqwest::Client) -> Result<String, String> {
    // Validate URL
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }

    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .header("User-Agent", "StarkBot/1.0 (Skill Installer)")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = ManageSkillsTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "manage_skills");
        assert_eq!(def.group, ToolGroup::System);
    }
}
