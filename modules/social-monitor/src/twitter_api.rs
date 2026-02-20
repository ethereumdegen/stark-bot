//! Twitter/X API v2 client with OAuth 1.0a authentication.
//!
//! Provides user lookup and timeline fetching for the social monitor service.

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
    pub fn from_env() -> Option<Self> {
        Some(Self {
            consumer_key: std::env::var("TWITTER_CONSUMER_KEY").ok()?,
            consumer_secret: std::env::var("TWITTER_CONSUMER_SECRET").ok()?,
            access_token: std::env::var("TWITTER_ACCESS_TOKEN").ok()?,
            access_token_secret: std::env::var("TWITTER_ACCESS_TOKEN_SECRET").ok()?,
        })
    }
}

/// Twitter user info from the API
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TwitterUser {
    pub id: String,
    pub name: String,
    pub username: String,
}

/// A tweet from the API
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tweet {
    pub id: String,
    pub text: String,
    pub created_at: Option<String>,
    pub conversation_id: Option<String>,
    pub in_reply_to_user_id: Option<String>,
    pub public_metrics: Option<TweetPublicMetrics>,
    pub referenced_tweets: Option<Vec<ReferencedTweet>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TweetPublicMetrics {
    pub like_count: Option<i64>,
    pub retweet_count: Option<i64>,
    pub reply_count: Option<i64>,
    pub quote_count: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReferencedTweet {
    #[serde(rename = "type")]
    pub ref_type: String,
    pub id: String,
}

impl Tweet {
    /// Determine the tweet type based on referenced_tweets
    pub fn tweet_type(&self) -> &str {
        if let Some(refs) = &self.referenced_tweets {
            for r in refs {
                match r.ref_type.as_str() {
                    "replied_to" => return "reply",
                    "quoted" => return "quote",
                    "retweeted" => return "retweet",
                    _ => {}
                }
            }
        }
        "original"
    }
}

/// Rate limit info parsed from response headers
#[derive(Debug)]
pub struct RateLimitInfo {
    pub remaining: Option<u32>,
    pub reset_at: Option<u64>,
}

/// Look up a Twitter user by username
pub async fn lookup_user_by_username(
    client: &reqwest::Client,
    credentials: &TwitterCredentials,
    username: &str,
) -> Result<TwitterUser, String> {
    let clean_username = username.trim_start_matches('@');
    let base_url = format!(
        "https://api.twitter.com/2/users/by/username/{}",
        clean_username
    );
    let auth = generate_oauth_header("GET", &base_url, credentials, None);

    let response = client
        .get(&base_url)
        .header("Authorization", auth)
        .send()
        .await
        .map_err(|e| format!("Twitter API request failed: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "Twitter API error ({}): {}",
            status,
            truncate_error(&body)
        ));
    }

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Invalid JSON: {}", e))?;

    if let Some(data) = json.get("data") {
        serde_json::from_value(data.clone()).map_err(|e| format!("Failed to parse user: {}", e))
    } else if let Some(errors) = json.get("errors") {
        Err(format!(
            "Twitter API error: {}",
            errors[0]["detail"]
                .as_str()
                .unwrap_or("Unknown error")
        ))
    } else {
        Err("User not found".to_string())
    }
}

/// Fetch recent tweets from a user's timeline
pub async fn get_user_tweets(
    client: &reqwest::Client,
    credentials: &TwitterCredentials,
    user_id: &str,
    since_id: Option<&str>,
    max_results: u32,
) -> Result<(Vec<Tweet>, RateLimitInfo), String> {
    let base_url = format!("https://api.twitter.com/2/users/{}/tweets", user_id);

    let mut query_params: Vec<(&str, String)> = vec![
        ("max_results", max_results.to_string()),
        (
            "tweet.fields",
            "created_at,conversation_id,in_reply_to_user_id,public_metrics,referenced_tweets"
                .to_string(),
        ),
        ("exclude", "retweets".to_string()),
    ];

    if let Some(sid) = since_id {
        query_params.push(("since_id", sid.to_string()));
    }

    let extra_params: Vec<(&str, &str)> = query_params
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();

    let auth =
        generate_oauth_header("GET", &base_url, credentials, Some(&extra_params));

    let query_string: String = query_params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencod(v)))
        .collect::<Vec<_>>()
        .join("&");

    let full_url = format!("{}?{}", base_url, query_string);

    let response = client
        .get(&full_url)
        .header("Authorization", auth)
        .send()
        .await
        .map_err(|e| format!("Twitter API request failed: {}", e))?;

    let rate_info = RateLimitInfo {
        remaining: response
            .headers()
            .get("x-rate-limit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok()),
        reset_at: response
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok()),
    };

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if status.as_u16() == 429 {
        return Err("Rate limited — backing off".to_string());
    }

    if !status.is_success() {
        return Err(format!(
            "Twitter API error ({}): {}",
            status,
            truncate_error(&body)
        ));
    }

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Invalid JSON: {}", e))?;

    let tweets = if let Some(data) = json.get("data") {
        serde_json::from_value(data.clone()).unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok((tweets, rate_info))
}

// =====================================================
// OAuth 1.0a Implementation
// =====================================================

fn percent_encode(s: &str) -> String {
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

fn urlencod(s: &str) -> String {
    percent_encode(s)
}

fn generate_oauth_header(
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

    let mut oauth_params: Vec<(&str, String)> = vec![
        ("oauth_consumer_key", credentials.consumer_key.clone()),
        ("oauth_nonce", nonce.clone()),
        ("oauth_signature_method", "HMAC-SHA1".to_string()),
        ("oauth_timestamp", timestamp.clone()),
        ("oauth_token", credentials.access_token.clone()),
        ("oauth_version", "1.0".to_string()),
    ];

    if let Some(params) = extra_params {
        for (k, v) in params {
            oauth_params.push((k, v.to_string()));
        }
    }

    oauth_params.sort_by(|a, b| a.0.cmp(b.0));

    let param_string: String = oauth_params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let base_string = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(url),
        percent_encode(&param_string)
    );

    let signing_key = format!(
        "{}&{}",
        percent_encode(&credentials.consumer_secret),
        percent_encode(&credentials.access_token_secret)
    );

    let mut mac =
        HmacSha1::new_from_slice(signing_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(base_string.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());

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

fn truncate_error(s: &str) -> &str {
    if s.len() > 200 {
        &s[..200]
    } else {
        s
    }
}
