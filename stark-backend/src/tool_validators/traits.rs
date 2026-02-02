//! Trait definition for tool validators

use async_trait::async_trait;
use super::types::{ValidationContext, ValidationResult, ValidatorPriority};

/// Trait that all tool validators must implement
///
/// Validators are modular checks that run before tool execution.
/// Each validator implements simple if/then/else logic to determine
/// whether a tool call should be allowed or blocked.
///
/// # Example
///
/// ```rust,ignore
/// pub struct MyValidator;
///
/// #[async_trait]
/// impl ToolValidator for MyValidator {
///     fn id(&self) -> &str { "my_validator" }
///     fn name(&self) -> &str { "My Custom Validator" }
///     fn applies_to(&self) -> Option<Vec<&str>> { Some(vec!["some_tool"]) }
///
///     async fn validate(&self, ctx: &ValidationContext) -> ValidationResult {
///         if some_condition {
///             ValidationResult::Block("Reason".into())
///         } else {
///             ValidationResult::Allow
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait ToolValidator: Send + Sync {
    /// Unique identifier for this validator
    fn id(&self) -> &str;

    /// Human-readable name for this validator
    fn name(&self) -> &str;

    /// Description of what this validator checks for
    fn description(&self) -> &str {
        ""
    }

    /// Which tools this validator applies to
    ///
    /// Return `None` to apply to all tools.
    /// Return `Some(vec!["tool1", "tool2"])` to only apply to specific tools.
    fn applies_to(&self) -> Option<Vec<&str>>;

    /// Priority for execution order (lower = earlier)
    fn priority(&self) -> ValidatorPriority {
        ValidatorPriority::Normal
    }

    /// Whether this validator is enabled
    fn enabled(&self) -> bool {
        true
    }

    /// Execute the validation check
    ///
    /// Returns `ValidationResult::Allow` to let the tool call proceed,
    /// or `ValidationResult::Block(reason)` to prevent execution.
    async fn validate(&self, ctx: &ValidationContext) -> ValidationResult;
}

/// A boxed validator for storage in collections
pub type BoxedValidator = std::sync::Arc<dyn ToolValidator>;
