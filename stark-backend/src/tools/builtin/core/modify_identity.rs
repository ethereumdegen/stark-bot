use crate::eip8004::types::{RegistrationFile, ServiceEntry};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for the agent to manage its EIP-8004 identity registration file
pub struct ModifyIdentityTool {
    definition: ToolDefinition,
}

impl ModifyIdentityTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action: 'read' view identity, 'create' new identity, 'update_field' update a field, 'add_service' add service entry, 'remove_service' remove service entry, 'upload' publish to identity.defirelay.com".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "read".to_string(),
                    "create".to_string(),
                    "update_field".to_string(),
                    "add_service".to_string(),
                    "remove_service".to_string(),
                    "upload".to_string(),
                ]),
            },
        );
        properties.insert(
            "name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Agent name (for create action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "description".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Agent description (for create action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "image".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Image URL (for create/update_field)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "field".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Field to update: name, description, image, active (for update_field)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "name".to_string(),
                    "description".to_string(),
                    "image".to_string(),
                    "active".to_string(),
                ]),
            },
        );
        properties.insert(
            "value".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "New value for the field (for update_field)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "service_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Service name, e.g. 'mcp', 'a2a', 'chat', 'x402', 'swap' (for add_service/remove_service)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "service_endpoint".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Service endpoint URL (for add_service)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "service_version".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Service version (for add_service, default '1.0')".to_string(),
                default: Some(serde_json::Value::String("1.0".to_string())),
                items: None,
                enum_values: None,
            },
        );

        ModifyIdentityTool {
            definition: ToolDefinition {
                name: "modify_identity".to_string(),
                description: "Manage your EIP-8004 agent identity registration file (IDENTITY.json). Create, read, update fields, add/remove services, or upload to identity.defirelay.com.".to_string(),
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

impl Default for ModifyIdentityTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ModifyIdentityParams {
    action: String,
    name: Option<String>,
    description: Option<String>,
    image: Option<String>,
    field: Option<String>,
    value: Option<String>,
    service_name: Option<String>,
    service_endpoint: Option<String>,
    service_version: Option<String>,
}

fn identity_path() -> std::path::PathBuf {
    crate::config::identity_document_path()
}

/// Read and parse the existing IDENTITY.json
async fn read_identity() -> Result<RegistrationFile, String> {
    let path = identity_path();
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read IDENTITY.json: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse IDENTITY.json: {}", e))
}

/// Write RegistrationFile back to IDENTITY.json
async fn write_identity(reg: &RegistrationFile) -> Result<(), String> {
    let path = identity_path();
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let json = serde_json::to_string_pretty(reg)
        .map_err(|e| format!("Failed to serialize identity: {}", e))?;
    tokio::fs::write(&path, &json)
        .await
        .map_err(|e| format!("Failed to write IDENTITY.json: {}", e))
}

#[async_trait]
impl Tool for ModifyIdentityTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ModifyIdentityParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        match params.action.as_str() {
            "read" => {
                let path = identity_path();
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => ToolResult::success(content).with_metadata(json!({
                        "action": "read",
                        "path": path.display().to_string()
                    })),
                    Err(_) => ToolResult::success("No identity file exists yet. Use action='create' to create one."),
                }
            }

            "create" => {
                // Refuse to overwrite an existing identity file
                let path = identity_path();
                if path.exists() {
                    return ToolResult::error(
                        "IDENTITY.json already exists. Use 'update_field' to modify it, or delete the file manually before creating a new one."
                    );
                }

                let name = match params.name {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for create action"),
                };
                let description = match params.description {
                    Some(d) => d,
                    None => return ToolResult::error("'description' is required for create action"),
                };

                let mut reg = RegistrationFile::new(&name, &description);

                if let Some(img) = params.image {
                    reg.image = Some(img);
                }

                match write_identity(&reg).await {
                    Ok(_) => {
                        let json = serde_json::to_string_pretty(&reg).unwrap_or_default();
                        log::info!("Created IDENTITY.json for agent: {}", name);
                        ToolResult::success(format!("Identity file created successfully:\n{}", json))
                            .with_metadata(json!({
                                "action": "create",
                                "name": name
                            }))
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "update_field" => {
                let field = match params.field {
                    Some(f) => f,
                    None => return ToolResult::error("'field' is required for update_field action"),
                };
                let value = match params.value {
                    Some(v) => v,
                    None => return ToolResult::error("'value' is required for update_field action"),
                };

                let mut reg = match read_identity().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(e),
                };

                match field.as_str() {
                    "name" => reg.name = value.clone(),
                    "description" => reg.description = value.clone(),
                    "image" => reg.image = Some(value.clone()),
                    "active" => {
                        reg.active = value.to_lowercase() == "true";
                    }
                    _ => return ToolResult::error(format!("Unknown field '{}'. Valid: name, description, image, active", field)),
                }

                match write_identity(&reg).await {
                    Ok(_) => {
                        log::info!("Updated IDENTITY.json field '{}' to '{}'", field, value);
                        ToolResult::success(format!("Updated '{}' to '{}'", field, value))
                            .with_metadata(json!({
                                "action": "update_field",
                                "field": field,
                                "value": value
                            }))
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "add_service" => {
                let service_name = match params.service_name {
                    Some(n) => n,
                    None => return ToolResult::error("'service_name' is required for add_service action"),
                };
                let endpoint = match params.service_endpoint {
                    Some(e) => e,
                    None => return ToolResult::error("'service_endpoint' is required for add_service action"),
                };
                let version = params.service_version.unwrap_or_else(|| "1.0".to_string());

                let mut reg = match read_identity().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(e),
                };

                reg.services.push(ServiceEntry {
                    name: service_name.clone(),
                    endpoint: endpoint.clone(),
                    version: version.clone(),
                });

                match write_identity(&reg).await {
                    Ok(_) => {
                        log::info!("Added service '{}' to IDENTITY.json", service_name);
                        ToolResult::success(format!("Added service '{}' at {}", service_name, endpoint))
                            .with_metadata(json!({
                                "action": "add_service",
                                "service_name": service_name,
                                "endpoint": endpoint,
                                "version": version
                            }))
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "remove_service" => {
                let service_name = match params.service_name {
                    Some(n) => n,
                    None => return ToolResult::error("'service_name' is required for remove_service action"),
                };

                let mut reg = match read_identity().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(e),
                };

                let before = reg.services.len();
                reg.services.retain(|s| s.name != service_name);
                let removed = before - reg.services.len();

                if removed == 0 {
                    return ToolResult::error(format!("Service '{}' not found in identity", service_name));
                }

                match write_identity(&reg).await {
                    Ok(_) => {
                        log::info!("Removed service '{}' from IDENTITY.json", service_name);
                        ToolResult::success(format!("Removed service '{}'", service_name))
                            .with_metadata(json!({
                                "action": "remove_service",
                                "service_name": service_name,
                                "removed_count": removed
                            }))
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "upload" => {
                let path = identity_path();
                let json_content = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(_) => return ToolResult::error("No IDENTITY.json found. Create one first with action='create'."),
                };

                // Validate it parses
                let _reg: RegistrationFile = match serde_json::from_str(&json_content) {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(format!("Invalid IDENTITY.json: {}", e)),
                };

                // Use the identity client to upload
                use crate::identity_client::IDENTITY_CLIENT;

                // Use wallet provider (Privy/Flash) for SIWE authentication
                let wallet_provider = match &context.wallet_provider {
                    Some(wp) => wp,
                    None => return ToolResult::error(
                        "No wallet connected. Connect your wallet first to upload your identity."
                    ),
                };
                let upload_result = IDENTITY_CLIENT
                    .upload_identity_with_provider(wallet_provider, &json_content)
                    .await;

                match upload_result {
                    Ok(resp) => {
                        if resp.success {
                            let url = resp.url.unwrap_or_else(|| "unknown".to_string());
                            log::info!("Uploaded IDENTITY.json to {}", url);

                            // Set the agent_uri register so the identity_register preset can use it
                            context.set_register("agent_uri", json!(&url), "modify_identity");

                            ToolResult::success(format!("Identity uploaded successfully!\nHosted at: {}\n\nThe agent_uri register has been set — you can now call identity_register.", url))
                                .with_metadata(json!({
                                    "action": "upload",
                                    "url": url,
                                    "success": true
                                }))
                        } else {
                            let error = resp.error.unwrap_or_else(|| "Unknown error".to_string());
                            ToolResult::error(format!("Upload failed: {}. STOP — do not proceed with on-chain registration until the upload succeeds.", error))
                        }
                    }
                    Err(e) => ToolResult::error(format!("Upload failed: {}. STOP — do not proceed with on-chain registration until the upload succeeds.", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Use: read, create, update_field, add_service, remove_service, upload",
                params.action
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_creation() {
        let tool = ModifyIdentityTool::new();
        assert_eq!(tool.definition().name, "modify_identity");
        assert_eq!(tool.definition().group, ToolGroup::System);
    }
}
