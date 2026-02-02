//! Condition interpreter for RON validators
//!
//! This module evaluates conditions against a ValidationContext at runtime.

use std::str::FromStr;
use regex::Regex;
use crate::controllers::api_keys::ApiKeyId;
use crate::tool_validators::ValidationContext;
use super::schema::Condition;

impl Condition {
    /// Evaluate this condition against the given context
    pub fn evaluate(&self, ctx: &ValidationContext) -> bool {
        match self {
            // Tool name matching
            Condition::ToolName(name) => ctx.tool_name == *name,

            // URL matching (case-insensitive)
            Condition::UrlContains(s) => {
                ctx.tool_args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|url| url.to_lowercase().contains(&s.to_lowercase()))
                    .unwrap_or(false)
            }

            // URL regex matching
            Condition::UrlMatches(pattern) => {
                Regex::new(pattern)
                    .ok()
                    .and_then(|re| {
                        ctx.tool_args
                            .get("url")
                            .and_then(|v| v.as_str())
                            .map(|url| re.is_match(url))
                    })
                    .unwrap_or(false)
            }

            // Argument existence
            Condition::ArgExists(key) => ctx.tool_args.get(key).is_some(),
            Condition::ArgMissing(key) => ctx.tool_args.get(key).is_none(),

            // Argument value matching
            Condition::ArgEquals(key, value) => {
                ctx.tool_args
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(|v| v == value)
                    .unwrap_or(false)
            }

            Condition::ArgContains(key, substr) => {
                ctx.tool_args
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(|v| v.contains(substr))
                    .unwrap_or(false)
            }

            // Credential checks
            Condition::CredentialExists(key) => {
                ApiKeyId::from_str(key)
                    .ok()
                    .and_then(|k| ctx.tool_context.get_api_key_by_id(k))
                    .map(|v| !v.is_empty())
                    .unwrap_or(false)
            }

            Condition::CredentialMissing(key) => {
                !Condition::CredentialExists(key.clone()).evaluate(ctx)
            }

            // Combinators
            Condition::All(conditions) => conditions.iter().all(|c| c.evaluate(ctx)),
            Condition::Any(conditions) => conditions.iter().any(|c| c.evaluate(ctx)),
            Condition::Not(c) => !c.evaluate(ctx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::types::ToolContext;
    use serde_json::json;
    use std::sync::Arc;

    fn make_ctx(tool_name: &str, args: serde_json::Value) -> ValidationContext {
        ValidationContext::new(
            tool_name.to_string(),
            args,
            Arc::new(ToolContext::new()),
        )
    }

    #[test]
    fn test_tool_name_condition() {
        let ctx = make_ctx("test_tool", json!({}));

        assert!(Condition::ToolName("test_tool".to_string()).evaluate(&ctx));
        assert!(!Condition::ToolName("other_tool".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_url_contains_condition() {
        let ctx = make_ctx("http_post", json!({
            "url": "https://api.example.com/v1/users"
        }));

        assert!(Condition::UrlContains("example.com".to_string()).evaluate(&ctx));
        assert!(Condition::UrlContains("EXAMPLE.COM".to_string()).evaluate(&ctx)); // case-insensitive
        assert!(!Condition::UrlContains("other.com".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_url_matches_condition() {
        let ctx = make_ctx("http_post", json!({
            "url": "https://api.example.com/v1/users/123"
        }));

        assert!(Condition::UrlMatches(r"/users/\d+".to_string()).evaluate(&ctx));
        assert!(!Condition::UrlMatches(r"/posts/\d+".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_arg_exists_condition() {
        let ctx = make_ctx("test", json!({
            "key1": "value1"
        }));

        assert!(Condition::ArgExists("key1".to_string()).evaluate(&ctx));
        assert!(!Condition::ArgExists("key2".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_arg_missing_condition() {
        let ctx = make_ctx("test", json!({
            "key1": "value1"
        }));

        assert!(!Condition::ArgMissing("key1".to_string()).evaluate(&ctx));
        assert!(Condition::ArgMissing("key2".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_arg_equals_condition() {
        let ctx = make_ctx("test", json!({
            "key1": "value1"
        }));

        assert!(Condition::ArgEquals("key1".to_string(), "value1".to_string()).evaluate(&ctx));
        assert!(!Condition::ArgEquals("key1".to_string(), "other".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_arg_contains_condition() {
        let ctx = make_ctx("test", json!({
            "key1": "hello world"
        }));

        assert!(Condition::ArgContains("key1".to_string(), "world".to_string()).evaluate(&ctx));
        assert!(!Condition::ArgContains("key1".to_string(), "foo".to_string()).evaluate(&ctx));
    }

    #[test]
    fn test_all_combinator() {
        let ctx = make_ctx("test_tool", json!({
            "key1": "value1"
        }));

        let cond = Condition::All(vec![
            Condition::ToolName("test_tool".to_string()),
            Condition::ArgExists("key1".to_string()),
        ]);
        assert!(cond.evaluate(&ctx));

        let cond = Condition::All(vec![
            Condition::ToolName("test_tool".to_string()),
            Condition::ArgExists("key2".to_string()),
        ]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_any_combinator() {
        let ctx = make_ctx("test_tool", json!({}));

        let cond = Condition::Any(vec![
            Condition::ToolName("test_tool".to_string()),
            Condition::ToolName("other_tool".to_string()),
        ]);
        assert!(cond.evaluate(&ctx));

        let cond = Condition::Any(vec![
            Condition::ToolName("foo".to_string()),
            Condition::ToolName("bar".to_string()),
        ]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_not_combinator() {
        let ctx = make_ctx("test_tool", json!({}));

        assert!(!Condition::Not(Box::new(Condition::ToolName("test_tool".to_string()))).evaluate(&ctx));
        assert!(Condition::Not(Box::new(Condition::ToolName("other".to_string()))).evaluate(&ctx));
    }
}
