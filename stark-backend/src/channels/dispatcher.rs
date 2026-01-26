use crate::ai::{AiClient, Message, MessageRole, ThinkingLevel, ToolCall, ToolHistoryEntry, ToolResponse};
use crate::channels::types::{DispatchResult, NormalizedMessage};
use crate::context::{self, estimate_tokens, ContextManager};
use crate::db::Database;
use crate::execution::ExecutionTracker;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{MemoryType, SessionScope};
use crate::models::session_message::MessageRole as DbMessageRole;
use crate::tools::{ToolConfig, ToolContext, ToolExecution, ToolRegistry};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Maximum number of tool execution iterations
const MAX_TOOL_ITERATIONS: usize = 10;

/// JSON response format from the AI when using text-based tool calling
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentResponse {
    body: String,
    tool_call: Option<TextToolCall>,
}

/// Tool call extracted from text-based JSON response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TextToolCall {
    tool_name: String,
    tool_params: Value,
}

/// Dispatcher routes messages to the AI and returns responses
pub struct MessageDispatcher {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    tool_registry: Arc<ToolRegistry>,
    execution_tracker: Arc<ExecutionTracker>,
    context_manager: ContextManager,
    // Regex patterns for memory markers
    daily_log_pattern: Regex,
    remember_pattern: Regex,
    remember_important_pattern: Regex,
    // Regex patterns for thinking directives
    thinking_directive_pattern: Regex,
}

impl MessageDispatcher {
    pub fn new(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
    ) -> Self {
        let context_manager = ContextManager::new(db.clone());
        Self {
            db,
            broadcaster,
            tool_registry,
            execution_tracker,
            context_manager,
            daily_log_pattern: Regex::new(r"\[DAILY_LOG:\s*(.+?)\]").unwrap(),
            remember_pattern: Regex::new(r"\[REMEMBER:\s*(.+?)\]").unwrap(),
            remember_important_pattern: Regex::new(r"\[REMEMBER_IMPORTANT:\s*(.+?)\]").unwrap(),
            thinking_directive_pattern: Regex::new(r"(?i)^/(?:t|think|thinking)(?::(\w+))?$").unwrap(),
        }
    }

    /// Create a dispatcher without tool support (for backwards compatibility)
    pub fn new_without_tools(db: Arc<Database>, broadcaster: Arc<EventBroadcaster>) -> Self {
        // Create a minimal execution tracker for legacy use
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
        let context_manager = ContextManager::new(db.clone());
        Self {
            db: db.clone(),
            broadcaster,
            tool_registry: Arc::new(ToolRegistry::new()),
            execution_tracker,
            context_manager,
            daily_log_pattern: Regex::new(r"\[DAILY_LOG:\s*(.+?)\]").unwrap(),
            remember_pattern: Regex::new(r"\[REMEMBER:\s*(.+?)\]").unwrap(),
            remember_important_pattern: Regex::new(r"\[REMEMBER_IMPORTANT:\s*(.+?)\]").unwrap(),
            thinking_directive_pattern: Regex::new(r"(?i)^/(?:t|think|thinking)(?::(\w+))?$").unwrap(),
        }
    }

    /// Dispatch a normalized message to the AI and return the response
    pub async fn dispatch(&self, message: NormalizedMessage) -> DispatchResult {
        // Emit message received event
        self.broadcaster.broadcast(GatewayEvent::channel_message(
            message.channel_id,
            &message.channel_type,
            &message.user_name,
            &message.text,
        ));

        // Check for reset commands
        let text_lower = message.text.trim().to_lowercase();
        if text_lower == "/new" || text_lower == "/reset" {
            return self.handle_reset_command(&message).await;
        }

        // Check for thinking directives (session-level setting)
        if let Some(thinking_response) = self.handle_thinking_directive(&message).await {
            return thinking_response;
        }

        // Parse inline thinking directive and extract clean message
        let (thinking_level, clean_text) = self.parse_inline_thinking(&message.text);

        // Start execution tracking
        let execution_id = self.execution_tracker.start_execution(message.channel_id, "execute");

        // Get or create identity for the user
        let identity = match self.db.get_or_create_identity(
            &message.channel_type,
            &message.user_id,
            Some(&message.user_name),
        ) {
            Ok(id) => id,
            Err(e) => {
                log::error!("Failed to get/create identity: {}", e);
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(format!("Identity error: {}", e));
            }
        };

        // Determine session scope (group if chat_id != user_id, otherwise dm)
        let scope = if message.chat_id != message.user_id {
            SessionScope::Group
        } else {
            SessionScope::Dm
        };

        // Get or create chat session
        let session = match self.db.get_or_create_chat_session(
            &message.channel_type,
            message.channel_id,
            &message.chat_id,
            scope,
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to get/create session: {}", e);
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(format!("Session error: {}", e));
            }
        };

        // Use clean text (with inline thinking directive removed) for storage
        let message_text = clean_text.as_deref().unwrap_or(&message.text);

        // Estimate tokens for the user message
        let user_tokens = estimate_tokens(message_text);

        // Store user message in session with token count
        if let Err(e) = self.db.add_session_message(
            session.id,
            DbMessageRole::User,
            message_text,
            Some(&message.user_id),
            Some(&message.user_name),
            message.message_id.as_deref(),
            Some(user_tokens),
        ) {
            log::error!("Failed to store user message: {}", e);
        } else {
            // Update context tokens
            self.context_manager.update_context_tokens(session.id, user_tokens);
        }

        // Get active agent settings from database
        let settings = match self.db.get_active_agent_settings() {
            Ok(Some(settings)) => settings,
            Ok(None) => {
                let error = "No AI provider configured. Please configure agent settings.".to_string();
                log::error!("{}", error);
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error);
            }
            Err(e) => {
                let error = format!("Database error: {}", e);
                log::error!("{}", error);
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error);
            }
        };

        log::info!(
            "Using {} provider with model {} for message dispatch (api_key_len={})",
            settings.provider,
            settings.model,
            settings.api_key.len()
        );

        // Create AI client from settings
        let client = match AiClient::from_settings(&settings) {
            Ok(c) => c,
            Err(e) => {
                let error = format!("Failed to create AI client: {}", e);
                log::error!("{}", error);
                self.execution_tracker.complete_execution(message.channel_id);
                return DispatchResult::error(error);
            }
        };

        // Add thinking event before AI generation
        self.execution_tracker.add_thinking(message.channel_id, "Processing request...");

        // Get tool configuration for this channel (needed for system prompt)
        let tool_config = self.db.get_effective_tool_config(Some(message.channel_id))
            .unwrap_or_default();

        // Debug: Log tool configuration
        log::info!(
            "[DISPATCH] Tool config - profile: {:?}, allowed_groups: {:?}",
            tool_config.profile,
            tool_config.allowed_groups
        );

        // Build context from memories, tools, skills, and session history
        let system_prompt = self.build_system_prompt(&message, &identity.identity_id, &tool_config);

        // Debug: Log full system prompt
        log::debug!("[DISPATCH] System prompt:\n{}", system_prompt);

        // Get recent session messages for conversation context
        let history = self.db.get_recent_session_messages(session.id, 20).unwrap_or_default();

        // Build messages for the AI
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: system_prompt.clone(),
        }];

        // Add compaction summary if available (provides context from earlier in conversation)
        if let Some(compaction_summary) = self.context_manager.get_compaction_summary(session.id) {
            messages.push(Message {
                role: MessageRole::System,
                content: format!("## Previous Conversation Summary\n{}", compaction_summary),
            });
        }

        // Add conversation history (skip the last one since it's the current message)
        for msg in history.iter().take(history.len().saturating_sub(1)) {
            let role = match msg.role {
                DbMessageRole::User => MessageRole::User,
                DbMessageRole::Assistant => MessageRole::Assistant,
                DbMessageRole::System => MessageRole::System,
            };
            messages.push(Message {
                role,
                content: msg.content.clone(),
            });
        }

        // Add current user message (use clean text without thinking directive)
        messages.push(Message {
            role: MessageRole::User,
            content: message_text.to_string(),
        });

        // Debug: Log user message
        log::info!("[DISPATCH] User message: {}", message_text);

        // Apply thinking level if set (for Claude models)
        if let Some(level) = thinking_level {
            if client.supports_thinking() {
                log::info!("[DISPATCH] Applying thinking level: {}", level);
                client.set_thinking_level(level);
            }
        }

        // Check if the client supports tools and tools are configured
        let use_tools = client.supports_tools() && !self.tool_registry.is_empty();

        // Debug: Log tool availability
        log::info!(
            "[DISPATCH] Tool support - client_supports: {}, registry_count: {}, use_tools: {}",
            client.supports_tools(),
            self.tool_registry.len(),
            use_tools
        );

        // Build tool context with API keys from database
        let workspace_dir = std::env::var("STARK_WORKSPACE_DIR")
            .unwrap_or_else(|_| "./workspace".to_string());

        let mut tool_context = ToolContext::new()
            .with_channel(message.channel_id, message.channel_type.clone())
            .with_user(message.user_id.clone())
            .with_workspace(workspace_dir.clone());

        // Ensure workspace directory exists
        let _ = std::fs::create_dir_all(&workspace_dir);

        // Load API keys from database for tools that need them
        if let Ok(keys) = self.db.list_api_keys() {
            for key in keys {
                tool_context = tool_context.with_api_key(&key.service_name, key.api_key);
            }
        }

        // Load bot config from agent settings for git commits etc.
        if let Ok(Some(settings)) = self.db.get_active_agent_settings() {
            tool_context = tool_context.with_bot_config(settings.bot_name, settings.bot_email);
        }

        // Generate response with optional tool execution loop
        let final_response = if use_tools {
            self.generate_with_tool_loop(
                &client,
                messages,
                &tool_config,
                &tool_context,
                &identity.identity_id,
                session.id,
                &message,
            ).await
        } else {
            // Simple generation without tools
            client.generate_text(messages).await
        };

        match final_response {
            Ok(response) => {
                // Parse and create memories from the response
                self.process_memory_markers(
                    &response,
                    &identity.identity_id,
                    session.id,
                    &message.channel_type,
                    message.message_id.as_deref(),
                );

                // Clean response by removing memory markers before storing/returning
                let clean_response = self.clean_response(&response);

                // Estimate tokens for the response
                let response_tokens = estimate_tokens(&clean_response);

                // Store AI response in session with token count
                if let Err(e) = self.db.add_session_message(
                    session.id,
                    DbMessageRole::Assistant,
                    &clean_response,
                    None,
                    None,
                    None,
                    Some(response_tokens),
                ) {
                    log::error!("Failed to store AI response: {}", e);
                } else {
                    // Update context tokens
                    self.context_manager.update_context_tokens(session.id, response_tokens);

                    // Check if compaction is needed
                    if self.context_manager.needs_compaction(session.id) {
                        log::info!("[COMPACTION] Context limit reached for session {}, triggering compaction", session.id);
                        if let Err(e) = self.context_manager.compact_session(
                            session.id,
                            &client,
                            Some(&identity.identity_id),
                        ).await {
                            log::error!("[COMPACTION] Failed to compact session: {}", e);
                        }
                    }
                }

                // Emit response event
                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &clean_response,
                ));

                log::info!(
                    "Generated response for {} on channel {} using {}",
                    message.user_name,
                    message.channel_id,
                    settings.provider
                );

                // Complete execution tracking
                self.execution_tracker.complete_execution(message.channel_id);

                DispatchResult::success(clean_response)
            }
            Err(e) => {
                let error = format!("AI generation error ({}): {}", settings.provider, e);
                log::error!("{}", error);

                // Complete execution tracking on error
                self.execution_tracker.complete_execution(message.channel_id);

                DispatchResult::error(error)
            }
        }
    }

    /// Generate a response with tool execution loop (supports both native and text-based tool calling)
    async fn generate_with_tool_loop(
        &self,
        client: &AiClient,
        messages: Vec<Message>,
        tool_config: &ToolConfig,
        tool_context: &ToolContext,
        _identity_id: &str,
        _session_id: i64,
        original_message: &NormalizedMessage,
    ) -> Result<String, String> {
        let tools = self.tool_registry.get_tool_definitions(tool_config);

        // Debug: Log available tools
        log::info!(
            "[TOOL_LOOP] Available tools ({}): {:?}",
            tools.len(),
            tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        if tools.is_empty() {
            log::warn!("[TOOL_LOOP] No tools available, falling back to text-only generation");
            return client.generate_text(messages).await;
        }

        let mut conversation = messages.clone();
        let mut final_response = String::new();
        let mut iterations = 0;

        loop {
            iterations += 1;
            log::info!("[TOOL_LOOP] Iteration {} starting", iterations);

            if iterations > MAX_TOOL_ITERATIONS {
                log::warn!("Tool execution loop exceeded max iterations ({})", MAX_TOOL_ITERATIONS);
                break;
            }

            // Generate response (text-only since we're doing JSON-based tool calling)
            let ai_content = client.generate_text(conversation.clone()).await?;

            log::info!("[TOOL_LOOP] Raw AI response: {}", ai_content);

            // Try to parse as JSON AgentResponse
            let parsed = self.parse_agent_response(&ai_content);

            match parsed {
                Some(agent_response) => {
                    log::info!(
                        "[TOOL_LOOP] Parsed response - body_len: {}, has_tool_call: {}",
                        agent_response.body.len(),
                        agent_response.tool_call.is_some()
                    );

                    // Check if there's a tool call
                    if let Some(tool_call) = agent_response.tool_call {
                        log::info!(
                            "[TOOL_LOOP] Text-based tool call: {} with params: {}",
                            tool_call.tool_name,
                            tool_call.tool_params
                        );

                        // Handle special "use_skill" tool
                        let tool_result = if tool_call.tool_name == "use_skill" {
                            self.execute_skill_tool(&tool_call.tool_params).await
                        } else {
                            // Execute regular tool
                            self.tool_registry.execute(
                                &tool_call.tool_name,
                                tool_call.tool_params.clone(),
                                tool_context,
                                Some(tool_config),
                            ).await
                        };

                        log::info!("[TOOL_LOOP] Tool result success: {}", tool_result.success);
                        log::debug!("[TOOL_LOOP] Tool result content: {}", tool_result.content);

                        // Broadcast tool execution event
                        let _ = self.broadcaster.broadcast(GatewayEvent::tool_result(
                            original_message.channel_id,
                            &tool_call.tool_name,
                            tool_result.success,
                            0, // duration_ms - not tracked for text-based tool calls
                        ));

                        // Add the assistant's response and tool result to conversation
                        conversation.push(Message {
                            role: MessageRole::Assistant,
                            content: ai_content.clone(),
                        });

                        // Add tool result as user message for next iteration
                        let followup_prompt = if tool_result.success {
                            format!(
                                "Tool '{}' returned:\n{}\n\nNow provide your final response to the user based on this result. Remember to respond in JSON format.",
                                tool_call.tool_name,
                                tool_result.content
                            )
                        } else {
                            // Check if this is a git permission error (can be solved by forking)
                            let error_lower = tool_result.content.to_lowercase();
                            let is_git_permission = (error_lower.contains("permission") || error_lower.contains("403") || error_lower.contains("denied"))
                                && (error_lower.contains("git") || error_lower.contains("github") || error_lower.contains("push"));

                            if is_git_permission {
                                format!(
                                    "Tool '{}' FAILED with error:\n{}\n\nYou don't have push access to this repository. To contribute to repos you don't own, use the FORK workflow:\n1. Fork the repo: `gh repo fork OWNER/REPO --clone`\n2. Make changes in the forked repo\n3. Push to YOUR fork\n4. Create PR: `gh pr create --repo OWNER/REPO`\n\nTry the fork workflow. Remember to respond in JSON format.",
                                    tool_call.tool_name,
                                    tool_result.content
                                )
                            } else {
                                format!(
                                    "Tool '{}' FAILED with error:\n{}\n\nTry a different approach if possible. Common fixes:\n- If directory exists: cd into it instead of cloning\n- If command not found: try alternative command\n- If permission denied: check if you need to fork the repo first\n\nIf truly impossible, explain why. Remember to respond in JSON format.",
                                    tool_call.tool_name,
                                    tool_result.content
                                )
                            }
                        };
                        conversation.push(Message {
                            role: MessageRole::User,
                            content: followup_prompt,
                        });

                        // Continue loop to get final response
                        continue;
                    } else {
                        // No tool call, this is the final response
                        final_response = agent_response.body;
                        break;
                    }
                }
                None => {
                    // Couldn't parse as JSON, return raw content
                    log::warn!("[TOOL_LOOP] Could not parse response as JSON, returning raw content");
                    final_response = ai_content;
                    break;
                }
            }
        }

        if final_response.is_empty() {
            return Err("AI returned empty response".to_string());
        }

        Ok(final_response)
    }

    /// Execute the special "use_skill" tool
    async fn execute_skill_tool(&self, params: &Value) -> crate::tools::ToolResult {
        let skill_name = params.get("skill_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let input = params.get("input")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        log::info!("[SKILL] Executing skill '{}' with input: {}", skill_name, input);

        // Look up the skill
        let skills = match self.db.list_enabled_skills() {
            Ok(s) => s,
            Err(e) => {
                return crate::tools::ToolResult::error(format!("Failed to load skills: {}", e));
            }
        };

        let skill = skills.iter().find(|s| s.name == skill_name && s.enabled);

        match skill {
            Some(skill) => {
                // Return the skill's instructions/body along with context
                let mut result = format!("## Skill: {}\n\n", skill.name);
                result.push_str(&format!("Description: {}\n\n", skill.description));

                if !skill.body.is_empty() {
                    result.push_str("### Instructions:\n");
                    result.push_str(&skill.body);
                    result.push_str("\n\n");
                }

                result.push_str(&format!("### User Query:\n{}\n\n", input));
                result.push_str("Use the appropriate tools (like `exec` for commands) to fulfill this skill request based on the instructions above.");

                crate::tools::ToolResult::success(&result)
            }
            None => {
                crate::tools::ToolResult::error(format!(
                    "Skill '{}' not found or not enabled. Available skills: {}",
                    skill_name,
                    skills.iter()
                        .filter(|s| s.enabled)
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        }
    }

    /// Parse AI response as JSON AgentResponse, with fallback extraction
    fn parse_agent_response(&self, content: &str) -> Option<AgentResponse> {
        let content = content.trim();

        // Try direct JSON parse first
        if let Ok(response) = serde_json::from_str::<AgentResponse>(content) {
            return Some(response);
        }

        // Try to parse as typed JSON response
        // {"type": "message", "content": "..."} or {"type": "function", ...}
        if let Ok(json) = serde_json::from_str::<Value>(content) {
            if let Some(msg_type) = json.get("type").and_then(|v| v.as_str()) {
                // Handle message type - just extract content
                if msg_type == "message" {
                    if let Some(content_str) = json.get("content").and_then(|v| v.as_str()) {
                        log::info!("[PARSE] Extracted message content from type:message format");
                        return Some(AgentResponse {
                            body: content_str.to_string(),
                            tool_call: None,
                        });
                    }
                }
                // Handle function call type
                if msg_type == "function" {
                    if let (Some(name), Some(params)) = (
                        json.get("name").and_then(|v| v.as_str()),
                        json.get("parameters"),
                    ) {
                        log::info!("[PARSE] Converted native function call format: {}", name);
                        return Some(AgentResponse {
                            body: format!("Executing {}...", name),
                            tool_call: Some(TextToolCall {
                                tool_name: name.to_string(),
                                tool_params: params.clone(),
                            }),
                        });
                    }
                }
            }
        }

        // Try to extract JSON from markdown code blocks
        let json_patterns = [
            // ```json ... ```
            regex::Regex::new(r"```(?:json)?\s*\n?([\s\S]*?)\n?```").ok()?,
        ];

        for pattern in &json_patterns {
            if let Some(captures) = pattern.captures(content) {
                if let Some(json_match) = captures.get(1) {
                    let json_str = json_match.as_str().trim();
                    if let Ok(response) = serde_json::from_str::<AgentResponse>(json_str) {
                        return Some(response);
                    }
                    // Also try typed JSON format in code blocks
                    if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                        if let Some(msg_type) = json.get("type").and_then(|v| v.as_str()) {
                            // Handle message type
                            if msg_type == "message" {
                                if let Some(content_str) = json.get("content").and_then(|v| v.as_str()) {
                                    return Some(AgentResponse {
                                        body: content_str.to_string(),
                                        tool_call: None,
                                    });
                                }
                            }
                            // Handle function type
                            if msg_type == "function" {
                                if let (Some(name), Some(params)) = (
                                    json.get("name").and_then(|v| v.as_str()),
                                    json.get("parameters"),
                                ) {
                                    return Some(AgentResponse {
                                        body: format!("Executing {}...", name),
                                        tool_call: Some(TextToolCall {
                                            tool_name: name.to_string(),
                                            tool_params: params.clone(),
                                        }),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Try to find JSON object anywhere in the content
        if let Some(start) = content.find('{') {
            // Find matching closing brace
            let mut depth = 0;
            let mut end = start;
            for (i, c) in content[start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end > start {
                let json_str = &content[start..end];
                if let Ok(response) = serde_json::from_str::<AgentResponse>(json_str) {
                    return Some(response);
                }
                // Also try typed JSON format
                if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                    if let Some(msg_type) = json.get("type").and_then(|v| v.as_str()) {
                        // Handle message type
                        if msg_type == "message" {
                            if let Some(content_str) = json.get("content").and_then(|v| v.as_str()) {
                                return Some(AgentResponse {
                                    body: content_str.to_string(),
                                    tool_call: None,
                                });
                            }
                        }
                        // Handle function type
                        if msg_type == "function" {
                            if let (Some(name), Some(params)) = (
                                json.get("name").and_then(|v| v.as_str()),
                                json.get("parameters"),
                            ) {
                                return Some(AgentResponse {
                                    body: format!("Executing {}...", name),
                                    tool_call: Some(TextToolCall {
                                        tool_name: name.to_string(),
                                        tool_params: params.clone(),
                                    }),
                                });
                            }
                        }
                    }
                }
            }
        }

        // If all parsing fails, treat the whole content as body with no tool call
        log::debug!("[PARSE] Could not extract JSON, treating as plain text response");
        Some(AgentResponse {
            body: content.to_string(),
            tool_call: None,
        })
    }

    /// Execute a list of tool calls and return responses (for native tool calling)
    #[allow(dead_code)]
    async fn execute_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        tool_config: &ToolConfig,
        tool_context: &ToolContext,
        channel_id: i64,
    ) -> Vec<ToolResponse> {
        let mut responses = Vec::new();

        // Get the current execution ID for tracking
        let execution_id = self.execution_tracker.get_execution_id(channel_id);

        for call in tool_calls {
            let start = std::time::Instant::now();

            // Start tracking this tool execution
            let task_id = if let Some(ref exec_id) = execution_id {
                Some(self.execution_tracker.start_tool(channel_id, exec_id, &call.name))
            } else {
                None
            };

            // Emit tool execution event (legacy event for backwards compatibility)
            self.broadcaster.broadcast(GatewayEvent::tool_execution(
                channel_id,
                &call.name,
                &call.arguments,
            ));

            // Execute the tool
            let result = self
                .tool_registry
                .execute(&call.name, call.arguments.clone(), tool_context, Some(tool_config))
                .await;

            let duration_ms = start.elapsed().as_millis() as i64;

            // Complete the tool tracking
            if let Some(ref tid) = task_id {
                if result.success {
                    self.execution_tracker.complete_task(tid);
                } else {
                    self.execution_tracker.complete_task_with_error(tid, &result.content);
                }
            }

            // Emit tool result event (legacy event for backwards compatibility)
            self.broadcaster.broadcast(GatewayEvent::tool_result(
                channel_id,
                &call.name,
                result.success,
                duration_ms,
            ));

            // Log the execution
            if let Err(e) = self.db.log_tool_execution(&ToolExecution {
                id: None,
                channel_id,
                tool_name: call.name.clone(),
                parameters: call.arguments.clone(),
                success: result.success,
                result: Some(result.content.clone()),
                duration_ms: Some(duration_ms),
                executed_at: Utc::now().to_rfc3339(),
            }) {
                log::error!("Failed to log tool execution: {}", e);
            }

            log::info!(
                "Tool '{}' executed in {}ms, success: {}",
                call.name,
                duration_ms,
                result.success
            );

            // Create tool response
            responses.push(if result.success {
                ToolResponse::success(call.id.clone(), result.content)
            } else {
                ToolResponse::error(call.id.clone(), result.content)
            });
        }

        responses
    }

    /// Load SOUL.md content if it exists
    fn load_soul() -> Option<String> {
        // Try multiple locations for SOUL.md
        let paths = [
            "SOUL.md",
            "./SOUL.md",
            "/app/SOUL.md",
        ];

        for path in paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                log::debug!("[SOUL] Loaded from {}", path);
                return Some(content);
            }
        }

        log::debug!("[SOUL] No SOUL.md found, using default personality");
        None
    }

    /// Build the system prompt with context from memories, tools, and skills
    fn build_system_prompt(
        &self,
        message: &NormalizedMessage,
        identity_id: &str,
        tool_config: &ToolConfig,
    ) -> String {
        let mut prompt = String::new();

        // Load SOUL.md if available, otherwise use default intro
        if let Some(soul) = Self::load_soul() {
            prompt.push_str(&soul);
            prompt.push_str("\n\n");
        } else {
            prompt.push_str("You are StarkBot, an AI agent who can respond to users and operate tools.\n\n");
        }

        // Add JSON response format instruction
        prompt.push_str("## RESPONSE FORMAT (CRITICAL)\n\n");
        prompt.push_str("You MUST respond in this JSON format:\n");
        prompt.push_str("```\n");
        prompt.push_str("{\"body\": \"your message\", \"tool_call\": null}\n");
        prompt.push_str("```\n\n");
        prompt.push_str("To call a tool:\n");
        prompt.push_str("```\n");
        prompt.push_str("{\"body\": \"brief status\", \"tool_call\": {\"tool_name\": \"name\", \"tool_params\": {...}}}\n");
        prompt.push_str("```\n\n");

        // Build tools array in OpenAI schema format
        let tools = self.tool_registry.get_tool_definitions(tool_config);
        let skills = self.db.list_enabled_skills().unwrap_or_default();
        let active_skills: Vec<_> = skills.iter().filter(|s| s.enabled).collect();

        if !tools.is_empty() || !active_skills.is_empty() {
            prompt.push_str("## AVAILABLE TOOLS\n\n");
            prompt.push_str("```json\n");
            prompt.push_str("[\n");

            let mut tool_entries: Vec<String> = Vec::new();

            // Add regular tools
            for tool in &tools {
                let tool_json = serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema
                    }
                });
                tool_entries.push(serde_json::to_string_pretty(&tool_json).unwrap_or_default());
            }

            // Add skills as a special tool with nested skill options
            if !active_skills.is_empty() {
                let skill_names: Vec<&str> = active_skills.iter().map(|s| s.name.as_str()).collect();
                let skill_descriptions: Vec<String> = active_skills.iter()
                    .map(|s| format!("{}: {}", s.name, s.description))
                    .collect();

                let skills_tool = serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": "use_skill",
                        "description": format!("Execute a skill. Available skills: {}", skill_descriptions.join("; ")),
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "skill_name": {
                                    "type": "string",
                                    "enum": skill_names,
                                    "description": "The skill to execute"
                                },
                                "input": {
                                    "type": "string",
                                    "description": "Input or query for the skill"
                                }
                            },
                            "required": ["skill_name", "input"]
                        }
                    }
                });
                tool_entries.push(serde_json::to_string_pretty(&skills_tool).unwrap_or_default());
            }

            prompt.push_str(&tool_entries.join(",\n"));
            prompt.push_str("\n]\n```\n\n");

            // Add usage examples
            prompt.push_str("## EXAMPLES\n\n");
            prompt.push_str("Weather query:\n");
            prompt.push_str("```\n{\"body\": \"Checking...\", \"tool_call\": {\"tool_name\": \"exec\", \"tool_params\": {\"command\": \"curl -s 'wttr.in/Ohio?format=3'\"}}}\n```\n\n");

            prompt.push_str("Web search:\n");
            prompt.push_str("```\n{\"body\": \"Searching...\", \"tool_call\": {\"tool_name\": \"web_search\", \"tool_params\": {\"query\": \"latest news\"}}}\n```\n\n");

            if !active_skills.is_empty() {
                prompt.push_str("Using a skill:\n");
                prompt.push_str(&format!(
                    "```\n{{\"body\": \"Using skill...\", \"tool_call\": {{\"tool_name\": \"use_skill\", \"tool_params\": {{\"skill_name\": \"{}\", \"input\": \"your query\"}}}}}}\n```\n\n",
                    active_skills.first().map(|s| s.name.as_str()).unwrap_or("weather")
                ));
            }

            prompt.push_str("**IMPORTANT**: For weather, news, or live data - USE TOOLS IMMEDIATELY. Do not say you cannot access real-time data.\n\n");
        }

        // Add skill details for context
        if !active_skills.is_empty() {
            prompt.push_str("## SKILL DETAILS\n\n");
            for skill in &active_skills {
                prompt.push_str(&format!("### {}\n", skill.name));
                prompt.push_str(&format!("{}\n\n", skill.description));
                if !skill.body.is_empty() {
                    // Include full skill body (truncate if extremely long)
                    let body = if skill.body.len() > 4000 {
                        format!("{}...\n[truncated]", &skill.body[..4000])
                    } else {
                        skill.body.clone()
                    };
                    prompt.push_str(&body);
                    prompt.push_str("\n");
                }
                prompt.push_str("\n");
            }
        }

        // Add daily logs context
        if let Ok(daily_logs) = self.db.get_todays_daily_logs(Some(identity_id)) {
            if !daily_logs.is_empty() {
                prompt.push_str("## Today's Notes\n");
                for log in daily_logs {
                    prompt.push_str(&format!("- {}\n", log.content));
                }
                prompt.push('\n');
            }
        }

        // Add relevant long-term memories
        if let Ok(memories) = self.db.get_long_term_memories(Some(identity_id), Some(5), 10) {
            if !memories.is_empty() {
                prompt.push_str("## User Context\n");
                for mem in memories {
                    prompt.push_str(&format!("- {}\n", mem.content));
                }
                prompt.push('\n');
            }
        }

        // Add recent session summaries (past conversations)
        if let Ok(summaries) = self.db.get_session_summaries(Some(identity_id), 3) {
            if !summaries.is_empty() {
                prompt.push_str("## Previous Sessions\n");
                for summary in summaries {
                    prompt.push_str(&format!("{}\n\n", summary.content));
                }
            }
        }

        // Add context
        prompt.push_str(&format!(
            "## Current Request\nUser: {} | Channel: {}\n",
            message.user_name, message.channel_type
        ));

        prompt
    }

    /// Process memory markers in the AI response
    fn process_memory_markers(
        &self,
        response: &str,
        identity_id: &str,
        session_id: i64,
        channel_type: &str,
        message_id: Option<&str>,
    ) {
        let today = Utc::now().date_naive();

        // Process daily logs
        for cap in self.daily_log_pattern.captures_iter(response) {
            if let Some(content) = cap.get(1) {
                let content_str = content.as_str().trim();
                if !content_str.is_empty() {
                    if let Err(e) = self.db.create_memory(
                        MemoryType::DailyLog,
                        content_str,
                        None,
                        None,
                        5,
                        Some(identity_id),
                        Some(session_id),
                        Some(channel_type),
                        message_id,
                        Some(today),
                        None,
                    ) {
                        log::error!("Failed to create daily log: {}", e);
                    } else {
                        log::info!("Created daily log: {}", content_str);
                    }
                }
            }
        }

        // Process regular remember markers (importance 7)
        for cap in self.remember_pattern.captures_iter(response) {
            if let Some(content) = cap.get(1) {
                let content_str = content.as_str().trim();
                if !content_str.is_empty() {
                    if let Err(e) = self.db.create_memory(
                        MemoryType::LongTerm,
                        content_str,
                        None,
                        None,
                        7,
                        Some(identity_id),
                        Some(session_id),
                        Some(channel_type),
                        message_id,
                        None,
                        None,
                    ) {
                        log::error!("Failed to create long-term memory: {}", e);
                    } else {
                        log::info!("Created long-term memory: {}", content_str);
                    }
                }
            }
        }

        // Process important remember markers (importance 9)
        for cap in self.remember_important_pattern.captures_iter(response) {
            if let Some(content) = cap.get(1) {
                let content_str = content.as_str().trim();
                if !content_str.is_empty() {
                    if let Err(e) = self.db.create_memory(
                        MemoryType::LongTerm,
                        content_str,
                        None,
                        None,
                        9,
                        Some(identity_id),
                        Some(session_id),
                        Some(channel_type),
                        message_id,
                        None,
                        None,
                    ) {
                        log::error!("Failed to create important memory: {}", e);
                    } else {
                        log::info!("Created important memory: {}", content_str);
                    }
                }
            }
        }
    }

    /// Remove memory markers from the response before returning to user
    fn clean_response(&self, response: &str) -> String {
        let mut clean = response.to_string();
        clean = self.daily_log_pattern.replace_all(&clean, "").to_string();
        clean = self.remember_pattern.replace_all(&clean, "").to_string();
        clean = self.remember_important_pattern.replace_all(&clean, "").to_string();
        // Clean up any double spaces or trailing whitespace
        clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        clean.trim().to_string()
    }

    /// Handle thinking directive messages (e.g., "/think:medium" sets session default)
    async fn handle_thinking_directive(&self, message: &NormalizedMessage) -> Option<DispatchResult> {
        let text = message.text.trim();

        // Check if this is a standalone thinking directive
        if let Some(captures) = self.thinking_directive_pattern.captures(text) {
            let level_str = captures.get(1).map(|m| m.as_str()).unwrap_or("low");

            if let Some(level) = ThinkingLevel::from_str(level_str) {
                // Store the thinking level preference for this session
                // For now, we just acknowledge it (session storage could be added later)
                let response = format!(
                    "Thinking level set to **{}**. {}",
                    level,
                    match level {
                        ThinkingLevel::Off => "Extended thinking is now disabled.",
                        ThinkingLevel::Minimal => "Using minimal thinking (~1K tokens).",
                        ThinkingLevel::Low => "Using low thinking (~4K tokens).",
                        ThinkingLevel::Medium => "Using medium thinking (~10K tokens).",
                        ThinkingLevel::High => "Using high thinking (~32K tokens).",
                        ThinkingLevel::XHigh => "Using maximum thinking (~64K tokens).",
                    }
                );

                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &response,
                ));

                log::info!(
                    "Thinking level set to {} for user {} on channel {}",
                    level,
                    message.user_name,
                    message.channel_id
                );

                return Some(DispatchResult::success(response));
            } else {
                // Invalid level specified
                let response = format!(
                    "Invalid thinking level '{}'. Valid options: off, minimal, low, medium, high, xhigh",
                    level_str
                );
                self.broadcaster.broadcast(GatewayEvent::agent_response(
                    message.channel_id,
                    &message.user_name,
                    &response,
                ));
                return Some(DispatchResult::success(response));
            }
        }

        None
    }

    /// Parse inline thinking directive from message (e.g., "/think:high What is...")
    /// Returns the thinking level and the clean message text
    fn parse_inline_thinking(&self, text: &str) -> (Option<ThinkingLevel>, Option<String>) {
        let text = text.trim();

        // Pattern: /think:level followed by the actual message
        let inline_pattern = Regex::new(r"(?i)^/(?:t|think|thinking):(\w+)\s+(.+)$").unwrap();

        if let Some(captures) = inline_pattern.captures(text) {
            let level_str = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let clean_text = captures.get(2).map(|m| m.as_str().to_string());

            if let Some(level) = ThinkingLevel::from_str(level_str) {
                return (Some(level), clean_text);
            }
        }

        // No inline thinking directive found
        (None, None)
    }

    /// Handle /new or /reset commands
    async fn handle_reset_command(&self, message: &NormalizedMessage) -> DispatchResult {
        // Determine session scope
        let scope = if message.chat_id != message.user_id {
            SessionScope::Group
        } else {
            SessionScope::Dm
        };

        // Get the current session
        match self.db.get_or_create_chat_session(
            &message.channel_type,
            message.channel_id,
            &message.chat_id,
            scope,
            None,
        ) {
            Ok(session) => {
                // Get identity for memory storage
                let identity_id = self.db.get_or_create_identity(
                    &message.channel_type,
                    &message.user_id,
                    Some(&message.user_name),
                ).ok().map(|id| id.identity_id);

                // Save session memory before reset (session memory hook)
                let message_count = self.db.count_session_messages(session.id).unwrap_or(0);
                if message_count >= 2 {
                    // Only save if there are meaningful messages
                    if let Ok(Some(settings)) = self.db.get_active_agent_settings() {
                        if let Ok(client) = AiClient::from_settings(&settings) {
                            match context::save_session_memory(
                                &self.db,
                                &client,
                                session.id,
                                identity_id.as_deref(),
                                15, // Save last 15 messages
                            ).await {
                                Ok(memory_id) => {
                                    log::info!("[SESSION_MEMORY] Saved session memory (id={}) before reset", memory_id);
                                }
                                Err(e) => {
                                    log::warn!("[SESSION_MEMORY] Failed to save session memory: {}", e);
                                }
                            }
                        }
                    }
                }

                // Reset the session
                match self.db.reset_chat_session(session.id) {
                    Ok(_) => {
                        let response = "Session reset. Let's start fresh!".to_string();
                        self.broadcaster.broadcast(GatewayEvent::agent_response(
                            message.channel_id,
                            &message.user_name,
                            &response,
                        ));
                        DispatchResult::success(response)
                    }
                    Err(e) => {
                        log::error!("Failed to reset session: {}", e);
                        DispatchResult::error(format!("Failed to reset session: {}", e))
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to get session for reset: {}", e);
                DispatchResult::error(format!("Session error: {}", e))
            }
        }
    }
}
