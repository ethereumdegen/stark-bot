//! Command router — routes user commands to the appropriate Starflask agent.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use starflask::Starflask;
use uuid::Uuid;

use crate::agent_registry::AgentRegistry;
use crate::crypto_executor::{CryptoExecutor, ExecutionResult};
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::starflask_bridge;

pub struct CommandRouter {
    registry: Arc<AgentRegistry>,
    starflask: Arc<Starflask>,
    crypto_executor: Option<Arc<CryptoExecutor>>,
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub message: String,
    #[serde(default)]
    pub capability: Option<String>,
    #[serde(default)]
    pub hook: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CommandOutput {
    CryptoExecution { results: Vec<ExecutionResult> },
    MediaGeneration { urls: Vec<String>, media_type: String },
    SocialPost { post_url: Option<String>, confirmation: String },
    TextResponse { text: String },
    Raw { data: Value },
}

impl CommandRouter {
    pub fn new(
        registry: Arc<AgentRegistry>,
        starflask: Arc<Starflask>,
        crypto_executor: Option<Arc<CryptoExecutor>>,
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
    ) -> Self {
        Self { registry, starflask, crypto_executor, db, broadcaster }
    }

    /// Route a command to the appropriate agent and return the result.
    pub async fn route(&self, command: Command) -> Result<CommandOutput, String> {
        let capability = command.capability.clone()
            .unwrap_or_else(|| self.detect_capability(&command.message));

        let agent_id = self.registry.get_agent_id(&capability)
            .or_else(|| {
                log::info!("[CommandRouter] No agent for '{}', falling back to any available agent", capability);
                self.registry.get_any_agent_id()
            })
            .ok_or_else(|| "No agents available. Sync your Starflask agents first.".to_string())?;

        // Log command
        let cmd_id = self.db.log_starflask_command(
            &capability,
            None,
            &command.message,
        ).unwrap_or(0);

        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.command_started",
            serde_json::json!({
                "command_id": cmd_id,
                "capability": &capability,
                "message": &command.message,
            }),
        ));

        // Dispatch
        let session = if let Some(hook) = &command.hook {
            let payload = command.payload.clone().unwrap_or(serde_json::json!({}));
            self.starflask.fire_hook_and_wait(&agent_id, hook, payload).await
                .map_err(|e| format!("Hook fire failed: {}", e))?
        } else {
            self.starflask.query(&agent_id, &command.message).await
                .map_err(|e| format!("Query failed: {}", e))?
        };

        // Update command log with session_id
        let _ = self.db.complete_starflask_command(
            cmd_id,
            "dispatched",
            &serde_json::json!({ "session_id": session.id.to_string() }),
        );

        // Parse result per capability
        let output = self.parse_output(&capability, &session.result, &agent_id).await;

        // Complete command log
        let status = if output.is_ok() { "completed" } else { "failed" };
        let result_data = match &output {
            Ok(o) => serde_json::to_value(o).unwrap_or(Value::Null),
            Err(e) => serde_json::json!({ "error": e }),
        };
        let _ = self.db.complete_starflask_command(cmd_id, status, &result_data);

        // Broadcast result
        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.command_completed",
            serde_json::json!({
                "command_id": cmd_id,
                "capability": &capability,
                "status": status,
                "result": &result_data,
            }),
        ));

        output
    }

    /// Keyword heuristic for capability detection.
    fn detect_capability(&self, message: &str) -> String {
        let lower = message.to_lowercase();

        let crypto_keywords = ["swap", "bridge", "send", "transfer", "wallet", "balance", "token", "eth", "usdc", "address"];
        let image_keywords = ["image", "picture", "photo", "draw", "illustration", "art", "generate an image", "create an image"];
        let video_keywords = ["video", "clip", "animate", "animation", "footage"];
        let social_keywords = ["tweet", "post", "twitter", "x.com"];

        if crypto_keywords.iter().any(|kw| lower.contains(kw)) {
            return "crypto".to_string();
        }
        if image_keywords.iter().any(|kw| lower.contains(kw)) {
            return "image_gen".to_string();
        }
        if video_keywords.iter().any(|kw| lower.contains(kw)) {
            return "video_gen".to_string();
        }
        if social_keywords.iter().any(|kw| lower.contains(kw)) {
            return "social_media".to_string();
        }

        "general".to_string()
    }

    /// Parse session result into typed output based on capability.
    async fn parse_output(
        &self,
        capability: &str,
        result: &Option<Value>,
        _agent_id: &Uuid,
    ) -> Result<CommandOutput, String> {
        match capability {
            "crypto" => {
                let instructions = starflask_bridge::parse_session_result(result);
                if instructions.is_empty() {
                    // No crypto instructions — treat as text response
                    let text = starflask_bridge::parse_text_result(result);
                    return Ok(CommandOutput::TextResponse { text });
                }

                let executor = self.crypto_executor.as_ref()
                    .ok_or("Crypto executor not available (no wallet configured)")?;

                let mut results = Vec::new();
                for instruction in instructions {
                    match executor.execute(instruction).await {
                        Ok(r) => results.push(r),
                        Err(e) => results.push(ExecutionResult {
                            success: false,
                            data: serde_json::json!({ "error": e }),
                        }),
                    }
                }
                Ok(CommandOutput::CryptoExecution { results })
            }

            "image_gen" => {
                let urls = starflask_bridge::parse_media_result(result);
                Ok(CommandOutput::MediaGeneration { urls, media_type: "image".to_string() })
            }

            "video_gen" => {
                let urls = starflask_bridge::parse_media_result(result);
                Ok(CommandOutput::MediaGeneration { urls, media_type: "video".to_string() })
            }

            "social_media" => {
                let (post_url, confirmation) = starflask_bridge::parse_social_result(result);
                Ok(CommandOutput::SocialPost { post_url, confirmation })
            }

            _ => {
                // General or unknown — return as text or raw
                let text = starflask_bridge::parse_text_result(result);
                if text.is_empty() {
                    Ok(CommandOutput::Raw { data: result.clone().unwrap_or(Value::Null) })
                } else {
                    Ok(CommandOutput::TextResponse { text })
                }
            }
        }
    }
}
