//! Command router — routes user commands via LLM-based orchestration.
//!
//! All user queries go to the `general` Starflask agent. The agent handles
//! delegation internally via the `delegate` tool in its agentic loop —
//! no parsing or interception needed here.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use starflask::Starflask;
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

    /// Route a command through the orchestrator agent.
    ///
    /// If `command.capability` is explicitly set, bypass the orchestrator and
    /// go directly to that agent. Otherwise, query the `general` agent which
    /// handles all delegation internally via its agentic loop.
    pub async fn route(&self, command: Command) -> Result<CommandOutput, String> {
        // Manual override — explicit capability set by user
        if let Some(ref cap) = command.capability {
            if !cap.is_empty() {
                return self.route_direct(cap, &command).await;
            }
        }

        // Query the general/orchestrator agent
        let general_id = self.registry.get_agent_id("general")
            .or_else(|| self.registry.get_any_agent_id())
            .ok_or_else(|| "No agents available. Sync your Starflask agents first.".to_string())?;

        let cmd_id = self.db.log_starflask_command("general", None, &command.message).unwrap_or(0);

        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.command_started",
            serde_json::json!({
                "command_id": cmd_id,
                "capability": "general",
                "message": &command.message,
            }),
        ));

        let session = if let Some(hook) = &command.hook {
            let payload = command.payload.clone().unwrap_or(serde_json::json!({}));
            self.starflask.fire_hook_and_wait(&general_id, hook, payload).await
                .map_err(|e| format!("Hook fire failed: {}", e))?
        } else {
            self.starflask.query(&general_id, &command.message).await
                .map_err(|e| format!("Query failed: {}", e))?
        };

        // The agent handles delegation internally — just parse and return the final result
        let output = self.parse_output("general", &session.result, session.result_summary.as_deref()).await;
        self.complete_command(cmd_id, "general", &output, false);
        output
    }

    /// Route directly to a specific agent, bypassing the orchestrator.
    async fn route_direct(&self, capability: &str, command: &Command) -> Result<CommandOutput, String> {
        let agent_id = self.registry.get_agent_id(capability)
            .or_else(|| {
                log::info!("[CommandRouter] No agent for '{}', falling back to any available agent", capability);
                self.registry.get_any_agent_id()
            })
            .ok_or_else(|| "No agents available. Sync your Starflask agents first.".to_string())?;

        let cmd_id = self.db.log_starflask_command(capability, None, &command.message).unwrap_or(0);

        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.command_started",
            serde_json::json!({
                "command_id": cmd_id,
                "capability": capability,
                "message": &command.message,
            }),
        ));

        let session = if let Some(hook) = &command.hook {
            let payload = command.payload.clone().unwrap_or(serde_json::json!({}));
            self.starflask.fire_hook_and_wait(&agent_id, hook, payload).await
                .map_err(|e| format!("Hook fire failed: {}", e))?
        } else {
            self.starflask.query(&agent_id, &command.message).await
                .map_err(|e| format!("Query failed: {}", e))?
        };

        let output = self.parse_output(capability, &session.result, session.result_summary.as_deref()).await;
        self.complete_command(cmd_id, capability, &output, false);
        output
    }

    /// Fire a hook on a capability's agent.
    ///
    /// Used by the hook endpoint so that hook-driven agents (discord_moderator, etc.)
    /// can handle requests. Delegation is handled internally by the agent's agentic loop.
    pub async fn fire_hook_with_delegation(
        &self,
        capability: &str,
        hook_event: &str,
        payload: Value,
    ) -> Result<CommandOutput, String> {
        let agent_id = self.registry.get_agent_id(capability)
            .ok_or_else(|| format!("No agent for capability '{}'", capability))?;

        let cmd_id = self.db.log_starflask_command(capability, Some(hook_event), &format!("hook:{}", hook_event)).unwrap_or(0);

        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.command_started",
            serde_json::json!({
                "command_id": cmd_id,
                "capability": capability,
                "hook_event": hook_event,
            }),
        ));

        let session = self.starflask.fire_hook_and_wait(&agent_id, hook_event, payload).await
            .map_err(|e| format!("Hook fire failed: {}", e))?;

        let output = self.parse_output(capability, &session.result, session.result_summary.as_deref()).await;
        self.complete_command(cmd_id, capability, &output, false);
        output
    }

    /// Log completion and broadcast the result event.
    fn complete_command(
        &self,
        cmd_id: i64,
        capability: &str,
        output: &Result<CommandOutput, String>,
        delegated: bool,
    ) {
        let status = if output.is_ok() { "completed" } else { "failed" };
        let result_data = match output {
            Ok(o) => serde_json::to_value(o).unwrap_or(Value::Null),
            Err(e) => serde_json::json!({ "error": e }),
        };
        let _ = self.db.complete_starflask_command(cmd_id, status, &result_data);

        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.command_completed",
            serde_json::json!({
                "command_id": cmd_id,
                "capability": capability,
                "status": status,
                "delegated": delegated,
                "result": &result_data,
            }),
        ));
    }

    /// Parse session result into typed output based on capability.
    async fn parse_output(
        &self,
        capability: &str,
        result: &Option<Value>,
        result_summary: Option<&str>,
    ) -> Result<CommandOutput, String> {
        match capability {
            "crypto" => {
                let instructions = starflask_bridge::parse_session_result(result);
                if instructions.is_empty() {
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
                let urls = starflask_bridge::parse_media_result(result, result_summary);
                Ok(CommandOutput::MediaGeneration { urls, media_type: "image".to_string() })
            }

            "video_gen" => {
                let urls = starflask_bridge::parse_media_result(result, result_summary);
                Ok(CommandOutput::MediaGeneration { urls, media_type: "video".to_string() })
            }

            _ => {
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
