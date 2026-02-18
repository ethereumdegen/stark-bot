use crate::keystore_client::KEYSTORE_CLIENT;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for triggering a cloud backup or checking backup status
pub struct CloudBackupTool {
    definition: ToolDefinition,
}

impl CloudBackupTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action to perform: 'backup' to trigger a cloud backup, 'status' to check the last backup status".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["backup".to_string(), "status".to_string()]),
            },
        );

        CloudBackupTool {
            definition: ToolDefinition {
                name: "cloud_backup".to_string(),
                description: "Trigger a cloud backup of all bot data (API keys, settings, channels, skills, impulse map, etc.) or check the last backup status. Data is encrypted with ECIES before upload.".to_string(),
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

impl Default for CloudBackupTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct CloudBackupParams {
    action: String,
}

#[async_trait]
impl Tool for CloudBackupTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: CloudBackupParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        // Wallet provider is always set when a wallet is configured (Standard=EnvWalletProvider, Flash=FlashWalletProvider)
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => {
                return ToolResult::success(
                    "No wallet configured. Cloud backup is not available.",
                )
                .with_metadata(json!({ "configured": false }));
            }
        };
        let wallet_address = wallet_provider.get_address();

        match params.action.as_str() {
            "status" => {
                match db.get_bot_settings() {
                    Ok(_settings) => {
                        ToolResult::success(format!(
                            "Backup status:\n  Wallet: {}\n  Cloud backup is configured and available.\n  Use action 'backup' to trigger a new backup.",
                            wallet_address
                        ))
                        .with_metadata(json!({
                            "configured": true,
                            "wallet_address": wallet_address
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to check status: {}", e)),
                }
            }

            "backup" => {
                // Get ECIES encryption key from wallet provider
                let private_key = match wallet_provider.get_encryption_key().await {
                    Ok(k) => k,
                    Err(e) => {
                        return ToolResult::error(format!(
                            "Failed to get encryption key: {}", e
                        ));
                    }
                };

                // Collect all backup data
                let backup =
                    crate::backup::collect_backup_data(db, wallet_address.clone()).await;

                if backup.is_empty() {
                    return ToolResult::error("No data to backup.");
                }

                let item_count = backup.item_count();
                let key_count = backup.api_keys.len();
                let node_count = backup
                    .impulse_map_nodes
                    .iter()
                    .filter(|n| !n.is_trunk)
                    .count();
                let channel_count = backup.channels.len();
                let skill_count = backup.skills.len();

                // Serialize to JSON
                let backup_json = match serde_json::to_string(&backup) {
                    Ok(j) => j,
                    Err(e) => {
                        return ToolResult::error(format!("Failed to serialize backup: {}", e));
                    }
                };

                // Encrypt with ECIES using the raw private key (NOT wallet provider — this is encryption, not signing)
                let encrypted_data =
                    match crate::backup::encrypt_with_private_key(&private_key, &backup_json) {
                        Ok(data) => data,
                        Err(e) => {
                            return ToolResult::error(format!("Failed to encrypt backup: {}", e));
                        }
                    };

                // Upload to keystore — use wallet provider for SIWE auth (works in both modes)
                let store_result = KEYSTORE_CLIENT
                    .store_keys_with_provider(wallet_provider, &encrypted_data, item_count)
                    .await;

                match store_result {
                    Ok(resp) if resp.success => {
                        // Record backup in local state
                        if let Err(e) = db.record_keystore_backup(
                            &backup.wallet_address,
                            backup.version,
                            item_count,
                        ) {
                            log::warn!("Failed to record backup: {}", e);
                        }

                        ToolResult::success(format!(
                            "Cloud backup successful!\n  Items: {}\n  Keys: {}\n  Nodes: {}\n  Channels: {}\n  Skills: {}",
                            item_count, key_count, node_count, channel_count, skill_count
                        ))
                        .with_metadata(json!({
                            "success": true,
                            "item_count": item_count,
                            "key_count": key_count,
                            "node_count": node_count,
                            "channel_count": channel_count,
                            "skill_count": skill_count,
                            "wallet_address": wallet_address,
                        }))
                    }
                    Ok(resp) => {
                        let error = resp.error.unwrap_or_else(|| "Unknown error".to_string());
                        ToolResult::error(format!("Backup upload failed: {}", error))
                    }
                    Err(e) => ToolResult::error(format!("Failed to upload backup: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: backup, status",
                params.action
            )),
        }
    }
}
