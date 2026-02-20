//! Typed HTTP client for the OpenCode server API.

use serde::{Deserialize, Serialize};

pub struct OpenCodeClient {
    base_url: String,
    client: reqwest::Client,
}

// ── OpenCode API types ──────────────────────────────

#[derive(Debug, Serialize)]
pub struct MessagePart {
    pub r#type: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct SendMessageBody {
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Deserialize)]
pub struct OcSession {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OcMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub parts: Vec<OcMessagePart>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OcMessagePart {
    #[serde(default, rename = "type")]
    pub part_type: String,
    #[serde(default)]
    pub text: Option<String>,
}

// ── Client impl ─────────────────────────────────────

impl OpenCodeClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Check if OpenCode server is healthy
    pub async fn health(&self) -> Result<bool, String> {
        let resp = self
            .client
            .get(format!("{}/global/health", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Health check failed: {}", e))?;

        Ok(resp.status().is_success())
    }

    /// Create a new session
    pub async fn create_session(&self) -> Result<OcSession, String> {
        let resp = self
            .client
            .post(format!("{}/session", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Create session failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Create session HTTP {}: {}", 0, body));
        }

        resp.json::<OcSession>()
            .await
            .map_err(|e| format!("Parse session response: {}", e))
    }

    /// Send a prompt to a session and return the assistant's response text
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<String, String> {
        let body = SendMessageBody {
            parts: vec![MessagePart {
                r#type: "text".to_string(),
                text: text.to_string(),
            }],
        };

        let resp = self
            .client
            .post(format!("{}/session/{}/message", self.base_url, session_id))
            .json(&body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| format!("Prompt failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Prompt HTTP {}: {}", status, body));
        }

        // Extract assistant text from response messages
        let messages: Vec<OcMessage> = resp
            .json()
            .await
            .map_err(|e| format!("Parse prompt response: {}", e))?;

        let response_text = messages
            .iter()
            .filter(|m| m.role == "assistant")
            .flat_map(|m| &m.parts)
            .filter(|p| p.part_type == "text")
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");

        if response_text.is_empty() {
            // Fallback: return raw JSON so caller still gets something
            Ok(serde_json::to_string_pretty(&messages).unwrap_or_default())
        } else {
            Ok(response_text)
        }
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Result<Vec<OcSession>, String> {
        let resp = self
            .client
            .get(format!("{}/session", self.base_url))
            .send()
            .await
            .map_err(|e| format!("List sessions failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("List sessions error: {}", body));
        }

        resp.json::<Vec<OcSession>>()
            .await
            .map_err(|e| format!("Parse sessions: {}", e))
    }

    /// Delete a session
    #[allow(dead_code)]
    pub async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        let resp = self
            .client
            .delete(format!("{}/session/{}", self.base_url, session_id))
            .send()
            .await
            .map_err(|e| format!("Delete session failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Delete session error: {}", body));
        }

        Ok(())
    }
}
