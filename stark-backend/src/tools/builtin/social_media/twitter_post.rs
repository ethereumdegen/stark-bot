//! Twitter posting tool using OAuth 1.0a
//!
//! Posts tweets on behalf of a user using their OAuth 1.0a credentials.

use super::twitter_oauth::{
    check_subscription_tier, generate_oauth_header, TwitterCredentials, TWITTER_MAX_CHARS,
};
use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for posting tweets via Twitter API v2
pub struct TwitterPostTool {
    definition: ToolDefinition,
}

impl TwitterPostTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "text".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The text content of the tweet".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "reply_to".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional: The numeric tweet ID to reply to (e.g. \"1893027483920175104\"). Must be a number, NOT a username.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "quote_tweet_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional: The numeric tweet ID to quote (e.g. \"1893027483920175104\"). Must be a number, NOT a username.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        TwitterPostTool {
            definition: ToolDefinition {
                name: "twitter_post".to_string(),
                description: "Post a tweet to Twitter/X. Requires Twitter OAuth credentials to be configured in Settings > API Keys.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["text".to_string()],
                },
                group: ToolGroup::Messaging,
                hidden: false,
            },
        }
    }

    fn get_credential(&self, key_id: ApiKeyId, context: &ToolContext) -> Option<String> {
        context.get_api_key_by_id(key_id).filter(|k| !k.is_empty())
    }
}

impl Default for TwitterPostTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct TwitterPostParams {
    text: String,
    reply_to: Option<String>,
    quote_tweet_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitterApiResponse {
    data: Option<TwitterTweetData>,
    errors: Option<Vec<TwitterApiError>>,
}

#[derive(Debug, Deserialize)]
struct TwitterTweetData {
    id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct TwitterApiError {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[async_trait]
impl Tool for TwitterPostTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: TwitterPostParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate tweet text is not empty
        if params.text.is_empty() {
            return ToolResult::error("Tweet text cannot be empty");
        }

        // Get all 4 OAuth credentials
        let consumer_key = match self.get_credential(ApiKeyId::TwitterConsumerKey, context) {
            Some(k) => k,
            None => {
                return ToolResult::error(
                    "TWITTER_CONSUMER_KEY not configured. Add it in Settings > API Keys.",
                )
            }
        };

        let consumer_secret = match self.get_credential(ApiKeyId::TwitterConsumerSecret, context) {
            Some(k) => k,
            None => {
                return ToolResult::error(
                    "TWITTER_CONSUMER_SECRET not configured. Add it in Settings > API Keys.",
                )
            }
        };

        let access_token = match self.get_credential(ApiKeyId::TwitterAccessToken, context) {
            Some(k) => k,
            None => {
                return ToolResult::error(
                    "TWITTER_ACCESS_TOKEN not configured. Add it in Settings > API Keys.",
                )
            }
        };

        let access_token_secret =
            match self.get_credential(ApiKeyId::TwitterAccessTokenSecret, context) {
                Some(k) => k,
                None => {
                    return ToolResult::error(
                        "TWITTER_ACCESS_TOKEN_SECRET not configured. Add it in Settings > API Keys.",
                    )
                }
            };

        // Check subscription tier to enforce correct character limit
        let credentials = TwitterCredentials::new(
            consumer_key.clone(),
            consumer_secret.clone(),
            access_token.clone(),
            access_token_secret.clone(),
        );
        let client = context.http_client();
        let tier = check_subscription_tier(&client, &credentials).await;
        let max_chars = tier.max_tweet_chars();
        let char_count = params.text.chars().count();

        if char_count > max_chars {
            if max_chars == TWITTER_MAX_CHARS {
                return ToolResult::error(format!(
                    "Tweet is {} characters but this account is limited to {} (standard). \
                     X Premium is required for longer tweets.",
                    char_count, max_chars
                ));
            } else {
                return ToolResult::error(format!(
                    "Tweet exceeds maximum character limit ({} > {})",
                    char_count, max_chars
                ));
            }
        }

        // Validate tweet IDs are numeric before calling the API
        if let Some(reply_to) = &params.reply_to {
            if !reply_to.chars().all(|c| c.is_ascii_digit()) || reply_to.is_empty() {
                return ToolResult::error(format!(
                    "reply_to must be a numeric tweet ID (e.g. \"1893027483920175104\"), got \"{}\"",
                    reply_to
                ));
            }
        }
        if let Some(quote_id) = &params.quote_tweet_id {
            if !quote_id.chars().all(|c| c.is_ascii_digit()) || quote_id.is_empty() {
                return ToolResult::error(format!(
                    "quote_tweet_id must be a numeric tweet ID (e.g. \"1893027483920175104\"), got \"{}\"",
                    quote_id
                ));
            }
        }

        // Build request body
        let mut body = json!({
            "text": params.text
        });

        if let Some(reply_to) = &params.reply_to {
            body["reply"] = json!({
                "in_reply_to_tweet_id": reply_to
            });
        }

        if let Some(quote_id) = &params.quote_tweet_id {
            body["quote_tweet_id"] = json!(quote_id);
        }

        // Twitter API v2 endpoint
        let url = "https://api.twitter.com/2/tweets";

        // Generate OAuth header using shared module
        let auth_header = generate_oauth_header("POST", url, &credentials, None);

        // Make the request
        let response = match client
            .post(url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to send request: {}", e)),
        };

        let status = response.status();
        let response_text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            // Try to parse error response
            if let Ok(error_resp) = serde_json::from_str::<TwitterApiResponse>(&response_text) {
                if let Some(errors) = error_resp.errors {
                    let error_msg = errors
                        .iter()
                        .map(|e| e.message.clone())
                        .collect::<Vec<_>>()
                        .join("; ");
                    return ToolResult::error(format!("Twitter API error: {}", error_msg));
                }
            }
            return ToolResult::error(format!(
                "Twitter API error ({}): {}",
                status, response_text
            ));
        }

        // Parse success response
        match serde_json::from_str::<TwitterApiResponse>(&response_text) {
            Ok(resp) => {
                if let Some(data) = resp.data {
                    ToolResult::success(
                        json!({
                            "success": true,
                            "tweet_id": data.id,
                            "text": data.text,
                            "url": format!("https://twitter.com/i/web/status/{}", data.id)
                        })
                        .to_string(),
                    )
                } else {
                    ToolResult::error("Unexpected response format from Twitter API")
                }
            }
            Err(e) => ToolResult::error(format!("Failed to parse Twitter response: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::twitter_oauth::percent_encode;

    #[test]
    fn test_percent_encode() {
        assert_eq!(percent_encode("hello"), "hello");
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_tool_definition() {
        let tool = TwitterPostTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "twitter_post");
        assert!(def.input_schema.required.contains(&"text".to_string()));
    }
}
