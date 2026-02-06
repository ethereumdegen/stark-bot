//! Twitter OAuth 1.0a utilities for API authentication
//!
//! Provides shared OAuth functionality for Twitter API v2 access,
//! used by both the TwitterPostTool and the Twitter mention listener.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

/// Twitter OAuth 1.0a credentials
#[derive(Debug, Clone)]
pub struct TwitterCredentials {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
}

impl TwitterCredentials {
    pub fn new(
        consumer_key: String,
        consumer_secret: String,
        access_token: String,
        access_token_secret: String,
    ) -> Self {
        Self {
            consumer_key,
            consumer_secret,
            access_token,
            access_token_secret,
        }
    }
}

/// Percent-encode a string per OAuth spec (RFC 3986)
pub fn percent_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Generate OAuth 1.0a Authorization header for a request
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, etc.)
/// * `url` - Full URL (without query parameters)
/// * `credentials` - Twitter OAuth credentials
/// * `extra_params` - Additional parameters to include in signature (e.g., query params)
///
/// # Returns
/// The complete Authorization header value (e.g., "OAuth oauth_consumer_key=...")
pub fn generate_oauth_header(
    method: &str,
    url: &str,
    credentials: &TwitterCredentials,
    extra_params: Option<&[(&str, &str)]>,
) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    let nonce: String = (0..32)
        .map(|_| format!("{:x}", rand::random::<u8>()))
        .collect();

    // OAuth parameters
    let mut oauth_params: Vec<(&str, String)> = vec![
        ("oauth_consumer_key", credentials.consumer_key.clone()),
        ("oauth_nonce", nonce.clone()),
        ("oauth_signature_method", "HMAC-SHA1".to_string()),
        ("oauth_timestamp", timestamp.clone()),
        ("oauth_token", credentials.access_token.clone()),
        ("oauth_version", "1.0".to_string()),
    ];

    // Add extra params for signature if provided
    if let Some(params) = extra_params {
        for (k, v) in params {
            oauth_params.push((k, v.to_string()));
        }
    }

    // Sort parameters
    oauth_params.sort_by(|a, b| a.0.cmp(b.0));

    // Create parameter string
    let param_string: String = oauth_params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // Create signature base string
    let base_string = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(url),
        percent_encode(&param_string)
    );

    // Create signing key
    let signing_key = format!(
        "{}&{}",
        percent_encode(&credentials.consumer_secret),
        percent_encode(&credentials.access_token_secret)
    );

    // Generate HMAC-SHA1 signature
    let mut mac =
        HmacSha1::new_from_slice(signing_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(base_string.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());

    // Build Authorization header
    let auth_params = [
        ("oauth_consumer_key", credentials.consumer_key.as_str()),
        ("oauth_nonce", &nonce),
        ("oauth_signature", &signature),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_timestamp", &timestamp),
        ("oauth_token", &credentials.access_token),
        ("oauth_version", "1.0"),
    ];

    let auth_string: String = auth_params
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, percent_encode(v)))
        .collect::<Vec<_>>()
        .join(", ");

    format!("OAuth {}", auth_string)
}

/// Maximum characters per tweet (standard / free accounts)
pub const TWITTER_MAX_CHARS: usize = 280;

/// Maximum characters per tweet (X Premium / Premium+ accounts)
pub const TWITTER_PREMIUM_MAX_CHARS: usize = 25_000;

/// X subscription tier as reported by the API
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XSubscriptionTier {
    /// No subscription (free account) — 280 char limit
    None,
    /// Basic subscription — 280 char limit (no long tweets)
    Basic,
    /// Premium subscription — 25,000 char limit
    Premium,
    /// Premium+ subscription — 25,000 char limit
    PremiumPlus,
}

impl XSubscriptionTier {
    pub fn max_tweet_chars(&self) -> usize {
        match self {
            Self::Premium | Self::PremiumPlus => TWITTER_PREMIUM_MAX_CHARS,
            _ => TWITTER_MAX_CHARS,
        }
    }

    pub fn allows_long_tweets(&self) -> bool {
        matches!(self, Self::Premium | Self::PremiumPlus)
    }

    fn from_api_str(s: &str) -> Self {
        match s {
            "Premium" => Self::Premium,
            "PremiumPlus" => Self::PremiumPlus,
            "Basic" => Self::Basic,
            _ => Self::None,
        }
    }
}

#[derive(serde::Deserialize)]
struct UsersMeResponse {
    data: Option<UsersMeData>,
}

#[derive(serde::Deserialize)]
struct UsersMeData {
    subscription_type: Option<String>,
}

/// Check the authenticated user's X subscription tier via GET /2/users/me.
/// Returns the tier on success, or falls back to `None` (basic/free) on any error.
pub async fn check_subscription_tier(
    client: &reqwest::Client,
    credentials: &TwitterCredentials,
) -> XSubscriptionTier {
    let base_url = "https://api.twitter.com/2/users/me";
    let query_params = [("user.fields", "subscription_type")];
    let auth_header = generate_oauth_header("GET", base_url, credentials, Some(&query_params));

    let result = client
        .get(format!("{}?user.fields=subscription_type", base_url))
        .header("Authorization", auth_header)
        .send()
        .await;

    let response = match result {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Twitter: Failed to check subscription tier: {}", e);
            return XSubscriptionTier::None;
        }
    };

    if !response.status().is_success() {
        log::warn!(
            "Twitter: Subscription check returned status {}",
            response.status()
        );
        return XSubscriptionTier::None;
    }

    let body = response.text().await.unwrap_or_default();
    match serde_json::from_str::<UsersMeResponse>(&body) {
        Ok(resp) => {
            let tier_str = resp
                .data
                .and_then(|d| d.subscription_type)
                .unwrap_or_default();
            let tier = XSubscriptionTier::from_api_str(&tier_str);
            log::info!("Twitter: Account subscription tier: {:?}", tier);
            tier
        }
        Err(e) => {
            log::warn!("Twitter: Failed to parse subscription response: {}", e);
            XSubscriptionTier::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_encode() {
        assert_eq!(percent_encode("hello"), "hello");
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_credentials_creation() {
        let creds = TwitterCredentials::new(
            "key".to_string(),
            "secret".to_string(),
            "token".to_string(),
            "token_secret".to_string(),
        );
        assert_eq!(creds.consumer_key, "key");
        assert_eq!(creds.consumer_secret, "secret");
    }
}
