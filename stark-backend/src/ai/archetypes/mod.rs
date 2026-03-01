//! Model Archetypes for Agent Orchestration
//!
//! Archetypes define how different AI models handle tool calling:
//! - Some models (Kimi, OpenAI, Claude) support native tool calling via API
//! - Some models (Llama, generic endpoints) require text-based JSON tool calling
//!
//! This module provides a unified interface for handling both approaches.

pub mod claude;
pub mod kimi;
pub mod llama;
pub mod minimax;
pub mod openai;

use crate::tools::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Identifier for model archetypes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchetypeId {
    /// Text-based JSON tool calling (for generic Llama endpoints)
    Llama,
    /// Native OpenAI-compatible tool calling (Kimi, OpenAI, Azure, etc.)
    Kimi,
    /// Native OpenAI tool calling
    OpenAI,
    /// Native Claude tool calling
    Claude,
    /// MiniMax M2.5 - OpenAI-compatible with <think> block stripping
    MiniMax,
}

impl ArchetypeId {
    /// Parse archetype from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "llama" | "text" | "json" => Some(ArchetypeId::Llama),
            "kimi" | "moonshot" => Some(ArchetypeId::Kimi),
            "openai" | "native" | "standard" => Some(ArchetypeId::OpenAI),
            "claude" | "anthropic" => Some(ArchetypeId::Claude),
            "minimax" => Some(ArchetypeId::MiniMax),
            _ => None,
        }
    }

    /// Get the display name for this archetype
    pub fn as_str(&self) -> &'static str {
        match self {
            ArchetypeId::Llama => "llama",
            ArchetypeId::Kimi => "kimi",
            ArchetypeId::OpenAI => "openai",
            ArchetypeId::Claude => "claude",
            ArchetypeId::MiniMax => "minimax",
        }
    }
}

impl std::fmt::Display for ArchetypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Tool call extracted from text-based JSON response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextToolCall {
    pub tool_name: String,
    pub tool_params: Value,
}

/// JSON response format from the AI when using text-based tool calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub body: String,
    pub tool_call: Option<TextToolCall>,
}

/// Trait defining behavior for different model archetypes
pub trait ModelArchetype: Send + Sync {
    /// Get the archetype identifier
    fn id(&self) -> ArchetypeId;

    /// Whether this archetype uses native API tool calling
    fn uses_native_tool_calling(&self) -> bool;

    /// Get the default model name for this archetype
    /// Used when model is not explicitly specified (x402 endpoints use "default")
    fn default_model(&self) -> &'static str;

    /// Enhance system prompt with tool-calling instructions (for text-based archetypes)
    fn enhance_system_prompt(&self, base_prompt: &str, tools: &[ToolDefinition]) -> String;

    /// Parse AI response to extract tool calls (for text-based archetypes)
    /// Returns None if the response couldn't be parsed as a structured response
    fn parse_response(&self, content: &str) -> Option<AgentResponse>;

    /// Clean model-specific artifacts from response content (e.g. <think> blocks).
    /// Called on native tool-calling responses before the content is used.
    /// Default implementation returns content unchanged.
    fn clean_content(&self, content: &str) -> String {
        content.to_string()
    }

    /// Whether this model requires all system messages to be merged into one.
    /// Some APIs (e.g. MiniMax/Kimi) reject conversations with multiple system messages.
    /// Default: false.
    fn requires_single_system_message(&self) -> bool {
        false
    }

    /// Format the follow-up message after a tool execution
    fn format_tool_followup(&self, tool_name: &str, tool_result: &str, success: bool) -> String;
}

/// Registry holding all available archetypes
pub struct ArchetypeRegistry {
    archetypes: std::collections::HashMap<ArchetypeId, Box<dyn ModelArchetype>>,
}

impl ArchetypeRegistry {
    /// Create a new registry with all default archetypes
    pub fn new() -> Self {
        let mut registry = Self {
            archetypes: std::collections::HashMap::new(),
        };

        // Register default archetypes
        registry.register(Box::new(llama::LlamaArchetype::new()));
        registry.register(Box::new(kimi::KimiArchetype::new()));
        registry.register(Box::new(openai::OpenAIArchetype::new()));
        registry.register(Box::new(claude::ClaudeArchetype::new()));
        registry.register(Box::new(minimax::MiniMaxArchetype::new()));

        registry
    }

    /// Register an archetype
    pub fn register(&mut self, archetype: Box<dyn ModelArchetype>) {
        self.archetypes.insert(archetype.id(), archetype);
    }

    /// Get an archetype by ID
    pub fn get(&self, id: ArchetypeId) -> Option<&dyn ModelArchetype> {
        self.archetypes.get(&id).map(|a| a.as_ref())
    }

    /// Get the default archetype for safe fallback
    pub fn default_archetype(&self) -> &dyn ModelArchetype {
        // Llama is the safe default (text-based works everywhere)
        self.get(ArchetypeId::Llama)
            .expect("Llama archetype must always be registered")
    }
}

impl Default for ArchetypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

