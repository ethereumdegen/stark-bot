//! RON schema types for validator definitions
//!
//! These types define the DSL for writing validators in RON format.

use serde::{Deserialize, Serialize};

/// Top-level validator definition
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorDef {
    /// Unique identifier for this validator
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what this validator checks
    #[serde(default)]
    pub description: Option<String>,
    /// List of tool names this validator applies to
    pub applies_to: Vec<String>,
    /// Execution priority (Critical runs first)
    #[serde(default)]
    pub priority: Priority,
    /// List of rules to evaluate in order
    pub rules: Vec<Rule>,
    /// Default action if no rules match
    #[serde(default)]
    pub default: Action,
}

/// Priority levels for validator execution order
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
pub enum Priority {
    /// Execute first (security checks)
    Critical,
    /// Execute early
    High,
    /// Normal execution order
    #[default]
    Normal,
    /// Execute later
    Low,
}

/// A single rule with a condition and action
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    /// Condition that must match for this rule to apply
    pub when: Condition,
    /// Action to take if the condition matches
    pub then: Action,
}

/// Conditions that can be evaluated against a tool call
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Condition {
    // Tool/URL matching
    /// Match exact tool name
    ToolName(String),
    /// URL argument contains substring (case-insensitive)
    UrlContains(String),
    /// URL argument matches regex pattern
    UrlMatches(String),

    // Argument checks
    /// Argument has exact value
    ArgEquals(String, String),
    /// Argument value contains substring
    ArgContains(String, String),
    /// Argument is present
    ArgExists(String),
    /// Argument is not present
    ArgMissing(String),

    // Credential checks
    /// API key/credential exists and is non-empty
    CredentialExists(String),
    /// API key/credential does not exist or is empty
    CredentialMissing(String),

    // Combinators
    /// All conditions must match (AND)
    All(Vec<Condition>),
    /// At least one condition must match (OR)
    Any(Vec<Condition>),
    /// Negation
    Not(Box<Condition>),
}

/// Actions that can be taken by a validator
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub enum Action {
    /// Allow the tool call to proceed
    #[default]
    Allow,
    /// Block the tool call with a reason
    Block(String),
    /// Block with a reason and suggestion for alternative action
    BlockWithSuggestion {
        reason: String,
        suggestion: String,
    },
}
