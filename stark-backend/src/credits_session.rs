//! Credits session client — caches a Bearer token obtained from a single ERC-8128 handshake.
//!
//! Instead of signing every request with ERC-8128 (which in Flash mode requires a
//! Privy remote signing round-trip), we establish a session once (~1 ERC-8128 sign)
//! and reuse the returned Bearer token for the remainder of the TTL (~1 hour).

use crate::erc8128::Erc8128Signer;
use crate::wallet::WalletProvider;
use std::sync::Arc;
use tokio::sync::RwLock;

/// How many seconds before expiry to proactively refresh.
const REFRESH_MARGIN_SECS: i64 = 60;

#[derive(Debug, Clone)]
struct SessionState {
    token: String,
    expires_at: i64,
    wallet: String,
}

/// Client that manages a single credits session with the inference-super-router.
pub struct CreditsSessionClient {
    wallet_provider: Arc<dyn WalletProvider>,
    session_url: String,
    state: RwLock<Option<SessionState>>,
}

impl CreditsSessionClient {
    /// Create a new session client.
    ///
    /// `base_url` is the inference router base (e.g. `https://inference.defirelay.com`).
    pub fn new(wallet_provider: Arc<dyn WalletProvider>, base_url: &str) -> Self {
        let session_url = format!("{}/credits/session", base_url.trim_end_matches('/'));
        Self {
            wallet_provider,
            session_url,
            state: RwLock::new(None),
        }
    }

    /// Get a valid Bearer token, establishing or refreshing the session as needed.
    pub async fn get_token(&self) -> Result<String, String> {
        // Fast path: read lock
        {
            let state = self.state.read().await;
            if let Some(ref s) = *state {
                let now = chrono::Utc::now().timestamp();
                if now < s.expires_at - REFRESH_MARGIN_SECS {
                    return Ok(s.token.clone());
                }
            }
        }

        // Slow path: write lock, double-check, then establish session
        let mut state = self.state.write().await;
        // Double-check: another task may have refreshed while we waited
        if let Some(ref s) = *state {
            let now = chrono::Utc::now().timestamp();
            if now < s.expires_at - REFRESH_MARGIN_SECS {
                return Ok(s.token.clone());
            }
        }

        let new_state = self.establish_session().await?;
        let token = new_state.token.clone();
        *state = Some(new_state);
        Ok(token)
    }

    /// Invalidate the cached session (e.g. on 401 from server).
    pub async fn invalidate(&self) {
        let mut state = self.state.write().await;
        *state = None;
        log::info!("[CreditsSession] Session invalidated");
    }

    /// Wallet address from the provider.
    pub fn wallet_address(&self) -> String {
        self.wallet_provider.get_address()
    }

    /// Perform the ERC-8128 handshake to get a new session token.
    async fn establish_session(&self) -> Result<SessionState, String> {
        log::info!("[CreditsSession] Establishing new session at {}", self.session_url);

        let signer = Erc8128Signer::new(self.wallet_provider.clone(), 8453);

        // Parse URL parts for ERC-8128 signing
        let without_scheme = self.session_url
            .strip_prefix("https://")
            .or_else(|| self.session_url.strip_prefix("http://"))
            .unwrap_or(&self.session_url);
        let (authority, path) = match without_scheme.find('/') {
            Some(idx) => (&without_scheme[..idx], &without_scheme[idx..]),
            None => (without_scheme, "/credits/session"),
        };

        // Empty body POST
        let body = b"{}";
        let signed = signer
            .sign_request("POST", authority, path, None, Some(body))
            .await?;

        let client = crate::http::shared_client();
        let mut req = client
            .post(&self.session_url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("signature-input", &signed.signature_input)
            .header("signature", &signed.signature);

        if let Some(ref digest) = signed.content_digest {
            req = req.header("content-digest", digest);
        }

        let resp = req
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| format!("Session request failed: {}", e))?;

        let status = resp.status();
        let body_text = resp.text().await
            .map_err(|e| format!("Failed to read session response: {}", e))?;

        if !status.is_success() {
            return Err(format!("Session creation failed ({}): {}", status, body_text));
        }

        let json: serde_json::Value = serde_json::from_str(&body_text)
            .map_err(|e| format!("Invalid session response JSON: {}", e))?;

        let token = json.get("session_token")
            .and_then(|v| v.as_str())
            .ok_or("Missing session_token in response")?
            .to_string();

        let expires_at = json.get("expires_at")
            .and_then(|v| v.as_i64())
            .ok_or("Missing expires_at in response")?;

        let wallet = json.get("wallet")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        log::info!(
            "[CreditsSession] Session established for wallet {} (expires_at: {})",
            wallet, expires_at
        );

        Ok(SessionState {
            token,
            expires_at,
            wallet,
        })
    }
}
