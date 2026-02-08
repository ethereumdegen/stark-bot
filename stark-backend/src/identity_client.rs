//! Identity API client with SIWE authentication and x402 payment support
//!
//! Handles authenticated access to the identity.defirelay.com API for
//! uploading agent identity registration files (un-encrypted).
//! Supports x402 payments when the identity server requires them.

use ethers::signers::{LocalWallet, Signer};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::wallet::WalletProvider;

/// Default identity API URL
pub const DEFAULT_IDENTITY_URL: &str = "https://identity.defirelay.com";

/// HTTP request timeout
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum payment amount in wei (1000 STARKBOT with 18 decimals)
/// Safety limit to prevent the identity server from overcharging
const MAX_PAYMENT_WEI: &str = "1000000000000000000000";

/// Cached session for identity API
#[derive(Debug, Clone)]
struct IdentitySession {
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// Thread-safe identity client with session caching
pub struct IdentityClient {
    session: Arc<RwLock<Option<IdentitySession>>>,
    http_client: reqwest::Client,
    base_url: Arc<RwLock<String>>,
}

// Request/Response types

#[derive(Serialize)]
struct AuthorizeRequest {
    address: String,
}

#[derive(Deserialize)]
struct AuthorizeResponse {
    success: bool,
    message: Option<String>,
    #[allow(dead_code)]
    nonce: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct VerifyRequest {
    address: String,
    signature: String,
}

#[derive(Deserialize)]
struct VerifyResponse {
    success: bool,
    token: Option<String>,
    expires_at: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct UploadIdentityRequest {
    identity_json: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadIdentityResponse {
    pub success: bool,
    pub url: Option<String>,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// x402 Payment Required response (returned on 402)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentRequiredResponse {
    #[allow(dead_code)]
    pub x402_version: u32,
    pub accepts: Vec<PaymentRequirements>,
}

/// Payment requirements from 402 response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    pub max_amount_required: String,
    pub resource: String,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    pub asset: String,
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}

impl IdentityClient {
    pub fn new() -> Self {
        Self::with_url(DEFAULT_IDENTITY_URL)
    }

    pub fn with_url(url: &str) -> Self {
        Self {
            session: Arc::new(RwLock::new(None)),
            http_client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("Failed to build HTTP client"),
            base_url: Arc::new(RwLock::new(url.trim_end_matches('/').to_string())),
        }
    }

    async fn get_base_url(&self) -> String {
        self.base_url.read().await.clone()
    }

    async fn get_token(&self) -> Option<String> {
        let session = self.session.read().await;
        if let Some(ref s) = *session {
            if s.expires_at > chrono::Utc::now() + chrono::Duration::seconds(60) {
                return Some(s.token.clone());
            }
        }
        None
    }

    /// Authenticate with the identity server using SIWE
    async fn authenticate(&self, private_key: &str) -> Result<String, String> {
        let pk_clean = private_key.trim_start_matches("0x");
        let wallet: LocalWallet = pk_clean
            .parse()
            .map_err(|e| format!("Invalid private key: {:?}", e))?;
        let address = format!("{:?}", wallet.address());

        let base_url = self.get_base_url().await;
        log::info!("[Identity] Authenticating wallet: {} (server: {})", address, base_url);

        // Step 1: Request challenge
        let auth_resp = self
            .http_client
            .post(format!("{}/api/authorize", base_url))
            .json(&AuthorizeRequest {
                address: address.clone(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to connect to identity server: {}", e))?;

        if !auth_resp.status().is_success() {
            return Err(format!(
                "Identity authorize failed with status: {}",
                auth_resp.status()
            ));
        }

        let auth_data: AuthorizeResponse = auth_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse authorize response: {}", e))?;

        if !auth_data.success {
            return Err(auth_data.error.unwrap_or_else(|| "Authorization failed".to_string()));
        }

        let message = auth_data
            .message
            .ok_or_else(|| "No challenge message in response".to_string())?;

        // Step 2: Sign the SIWE message
        let signature = wallet
            .sign_message(&message)
            .await
            .map_err(|e| format!("Failed to sign message: {:?}", e))?;
        let signature_hex = format!("0x{}", hex::encode(signature.to_vec()));

        // Step 3: Verify signature and get token
        let verify_resp = self
            .http_client
            .post(format!("{}/api/authorize/verify", base_url))
            .json(&VerifyRequest {
                address: address.clone(),
                signature: signature_hex,
            })
            .send()
            .await
            .map_err(|e| format!("Failed to verify signature: {}", e))?;

        if !verify_resp.status().is_success() {
            return Err(format!(
                "Identity verify failed with status: {}",
                verify_resp.status()
            ));
        }

        let verify_data: VerifyResponse = verify_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse verify response: {}", e))?;

        if !verify_data.success {
            return Err(verify_data.error.unwrap_or_else(|| "Verification failed".to_string()));
        }

        let token = verify_data
            .token
            .ok_or_else(|| "No token in response".to_string())?;

        let expires_at = if let Some(exp) = verify_data.expires_at {
            chrono::DateTime::parse_from_rfc3339(&exp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now() + chrono::Duration::hours(1))
        } else {
            chrono::Utc::now() + chrono::Duration::hours(1)
        };

        let mut session = self.session.write().await;
        *session = Some(IdentitySession {
            token: token.clone(),
            expires_at,
        });

        log::info!("[Identity] Authentication successful, token expires at {}", expires_at);
        Ok(token)
    }

    async fn ensure_authenticated(&self, private_key: &str) -> Result<String, String> {
        if let Some(token) = self.get_token().await {
            return Ok(token);
        }
        self.authenticate(private_key).await
    }

    /// Upload identity JSON to the identity server (un-encrypted)
    /// Handles x402 payment automatically if required
    pub async fn upload_identity(
        &self,
        private_key: &str,
        identity_json: &str,
    ) -> Result<UploadIdentityResponse, String> {
        let token = self.ensure_authenticated(private_key).await?;
        let base_url = self.get_base_url().await;

        let resp = self
            .http_client
            .post(format!("{}/api/store_identity", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&UploadIdentityRequest {
                identity_json: identity_json.to_string(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to connect to identity server: {}", e))?;

        // If unauthorized, re-authenticate once
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            log::warn!("[Identity] Token expired, re-authenticating...");
            let new_token = self.authenticate(private_key).await?;

            let retry_resp = self
                .http_client
                .post(format!("{}/api/store_identity", base_url))
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&UploadIdentityRequest {
                    identity_json: identity_json.to_string(),
                })
                .send()
                .await
                .map_err(|e| format!("Failed to connect to identity server: {}", e))?;

            if retry_resp.status().as_u16() == 402 {
                return self.handle_x402_upload(
                    private_key,
                    &new_token,
                    identity_json,
                    retry_resp,
                ).await;
            }

            return retry_resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e));
        }

        // Check for 402 Payment Required
        if resp.status().as_u16() == 402 {
            return self.handle_x402_upload(
                private_key,
                &token,
                identity_json,
                resp,
            ).await;
        }

        resp.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))
    }

    /// Handle x402 payment for upload_identity
    async fn handle_x402_upload(
        &self,
        private_key: &str,
        token: &str,
        identity_json: &str,
        response: reqwest::Response,
    ) -> Result<UploadIdentityResponse, String> {
        let base_url = self.get_base_url().await;
        log::info!("[Identity] Server requires x402 payment for upload");

        let payment_required: PaymentRequiredResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse 402 response: {}", e))?;

        let requirements = payment_required.accepts.first()
            .ok_or_else(|| "No payment options in 402 response".to_string())?;

        // Safety check on amount
        let required_amount = &requirements.max_amount_required;
        let required_num: u128 = required_amount.parse()
            .map_err(|_| format!("Invalid payment amount: {}", required_amount))?;
        let max_num: u128 = MAX_PAYMENT_WEI.parse()
            .map_err(|_| "Invalid max payment constant".to_string())?;

        if required_num > max_num {
            return Err(format!(
                "Payment amount {} exceeds safety limit of {} (1000 STARKBOT)",
                required_amount, MAX_PAYMENT_WEI
            ));
        }

        log::info!(
            "[Identity] Payment required: {} to {} on {}",
            requirements.max_amount_required,
            requirements.pay_to,
            requirements.network
        );

        let signer = crate::x402::X402Signer::from_private_key(private_key)
            .map_err(|e| format!("Failed to create x402 signer: {}", e))?;

        let extra = requirements.extra.as_ref().and_then(|e| {
            serde_json::from_value::<crate::x402::PaymentExtra>(e.clone()).ok()
        });

        let x402_requirements = crate::x402::PaymentRequirements {
            scheme: requirements.scheme.clone(),
            network: requirements.network.clone(),
            max_amount_required: requirements.max_amount_required.clone(),
            resource: Some(requirements.resource.clone()),
            description: Some("Upload agent identity".to_string()),
            pay_to_address: requirements.pay_to.clone(),
            max_timeout_seconds: requirements.max_timeout_seconds,
            asset: requirements.asset.clone(),
            extra,
        };

        let payment_payload = signer.sign_payment(&x402_requirements).await
            .map_err(|e| format!("Failed to sign x402 payment: {}", e))?;

        let payment_header = payment_payload.to_base64()
            .map_err(|e| format!("Failed to encode payment: {}", e))?;

        log::info!("[Identity] Retrying request with x402 payment...");

        let retry_resp = self
            .http_client
            .post(format!("{}/api/store_identity", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("X-PAYMENT", payment_header)
            .json(&UploadIdentityRequest {
                identity_json: identity_json.to_string(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to send paid request: {}", e))?;

        if retry_resp.status().is_success() {
            log::info!("[Identity] Payment accepted, identity uploaded successfully");
        } else {
            log::warn!("[Identity] Paid request returned status: {}", retry_resp.status());
        }

        retry_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response after payment: {}", e))
    }

    // =====================================================
    // WalletProvider-aware methods (for Flash/Privy mode)
    // =====================================================

    /// Authenticate via WalletProvider
    async fn authenticate_with_provider(&self, provider: &Arc<dyn WalletProvider>) -> Result<String, String> {
        let address = provider.get_address();
        let base_url = self.get_base_url().await;
        log::info!("[Identity] Authenticating wallet via provider: {} (server: {})", address, base_url);

        let auth_resp = self
            .http_client
            .post(format!("{}/api/authorize", base_url))
            .json(&AuthorizeRequest {
                address: address.clone(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to connect to identity server: {}", e))?;

        if !auth_resp.status().is_success() {
            return Err(format!(
                "Identity authorize failed with status: {}",
                auth_resp.status()
            ));
        }

        let auth_data: AuthorizeResponse = auth_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse authorize response: {}", e))?;

        if !auth_data.success {
            return Err(auth_data.error.unwrap_or_else(|| "Authorization failed".to_string()));
        }

        let message = auth_data
            .message
            .ok_or_else(|| "No challenge message in response".to_string())?;

        let signature = provider
            .sign_message(message.as_bytes())
            .await
            .map_err(|e| format!("Failed to sign message: {}", e))?;
        let signature_hex = format!("0x{}", hex::encode(signature.to_vec()));

        let verify_resp = self
            .http_client
            .post(format!("{}/api/authorize/verify", base_url))
            .json(&VerifyRequest {
                address: address.clone(),
                signature: signature_hex,
            })
            .send()
            .await
            .map_err(|e| format!("Failed to verify signature: {}", e))?;

        if !verify_resp.status().is_success() {
            return Err(format!(
                "Identity verify failed with status: {}",
                verify_resp.status()
            ));
        }

        let verify_data: VerifyResponse = verify_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse verify response: {}", e))?;

        if !verify_data.success {
            return Err(verify_data.error.unwrap_or_else(|| "Verification failed".to_string()));
        }

        let token = verify_data
            .token
            .ok_or_else(|| "No token in response".to_string())?;

        let expires_at = if let Some(exp) = verify_data.expires_at {
            chrono::DateTime::parse_from_rfc3339(&exp)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now() + chrono::Duration::hours(1))
        } else {
            chrono::Utc::now() + chrono::Duration::hours(1)
        };

        let mut session = self.session.write().await;
        *session = Some(IdentitySession {
            token: token.clone(),
            expires_at,
        });

        log::info!("[Identity] Authentication via provider successful, token expires at {}", expires_at);
        Ok(token)
    }

    async fn ensure_authenticated_with_provider(&self, provider: &Arc<dyn WalletProvider>) -> Result<String, String> {
        if let Some(token) = self.get_token().await {
            return Ok(token);
        }
        self.authenticate_with_provider(provider).await
    }

    /// Upload identity JSON using WalletProvider for auth and x402 signing
    pub async fn upload_identity_with_provider(
        &self,
        provider: &Arc<dyn WalletProvider>,
        identity_json: &str,
    ) -> Result<UploadIdentityResponse, String> {
        let token = self.ensure_authenticated_with_provider(provider).await?;
        let base_url = self.get_base_url().await;

        let resp = self
            .http_client
            .post(format!("{}/api/store_identity", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&UploadIdentityRequest {
                identity_json: identity_json.to_string(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to connect to identity server: {}", e))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            log::warn!("[Identity] Token expired, re-authenticating via provider...");
            let new_token = self.authenticate_with_provider(provider).await?;

            let retry_resp = self
                .http_client
                .post(format!("{}/api/store_identity", base_url))
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&UploadIdentityRequest {
                    identity_json: identity_json.to_string(),
                })
                .send()
                .await
                .map_err(|e| format!("Failed to connect to identity server: {}", e))?;

            if retry_resp.status().as_u16() == 402 {
                return self.handle_x402_upload_with_provider(
                    provider,
                    &new_token,
                    identity_json,
                    retry_resp,
                ).await;
            }

            return retry_resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e));
        }

        if resp.status().as_u16() == 402 {
            return self.handle_x402_upload_with_provider(
                provider,
                &token,
                identity_json,
                resp,
            ).await;
        }

        resp.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))
    }

    /// Handle x402 payment for upload using WalletProvider
    async fn handle_x402_upload_with_provider(
        &self,
        provider: &Arc<dyn WalletProvider>,
        token: &str,
        identity_json: &str,
        response: reqwest::Response,
    ) -> Result<UploadIdentityResponse, String> {
        let base_url = self.get_base_url().await;
        log::info!("[Identity] Server requires x402 payment for upload (via provider)");

        let payment_required: PaymentRequiredResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse 402 response: {}", e))?;

        let requirements = payment_required.accepts.first()
            .ok_or_else(|| "No payment options in 402 response".to_string())?;

        let required_amount = &requirements.max_amount_required;
        let required_num: u128 = required_amount.parse()
            .map_err(|_| format!("Invalid payment amount: {}", required_amount))?;
        let max_num: u128 = MAX_PAYMENT_WEI.parse()
            .map_err(|_| "Invalid max payment constant".to_string())?;

        if required_num > max_num {
            return Err(format!(
                "Payment amount {} exceeds safety limit of {} (1000 STARKBOT)",
                required_amount, MAX_PAYMENT_WEI
            ));
        }

        log::info!(
            "[Identity] Payment required: {} to {} on {}",
            requirements.max_amount_required,
            requirements.pay_to,
            requirements.network
        );

        let signer = crate::x402::X402Signer::new(provider.clone());

        let extra = requirements.extra.as_ref().and_then(|e| {
            serde_json::from_value::<crate::x402::PaymentExtra>(e.clone()).ok()
        });

        let x402_requirements = crate::x402::PaymentRequirements {
            scheme: requirements.scheme.clone(),
            network: requirements.network.clone(),
            max_amount_required: requirements.max_amount_required.clone(),
            resource: Some(requirements.resource.clone()),
            description: Some("Upload agent identity".to_string()),
            pay_to_address: requirements.pay_to.clone(),
            max_timeout_seconds: requirements.max_timeout_seconds,
            asset: requirements.asset.clone(),
            extra,
        };

        let payment_payload = signer.sign_payment(&x402_requirements).await
            .map_err(|e| format!("Failed to sign x402 payment: {}", e))?;

        let payment_header = payment_payload.to_base64()
            .map_err(|e| format!("Failed to encode payment: {}", e))?;

        log::info!("[Identity] Retrying request with x402 payment (via provider)...");

        let retry_resp = self
            .http_client
            .post(format!("{}/api/store_identity", base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("X-PAYMENT", payment_header)
            .json(&UploadIdentityRequest {
                identity_json: identity_json.to_string(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to send paid request: {}", e))?;

        if retry_resp.status().is_success() {
            log::info!("[Identity] Payment accepted, identity uploaded successfully");
        } else {
            log::warn!("[Identity] Paid request returned status: {}", retry_resp.status());
        }

        retry_resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response after payment: {}", e))
    }
}

impl Default for IdentityClient {
    fn default() -> Self {
        Self::new()
    }
}

// Global singleton for the identity client
lazy_static::lazy_static! {
    pub static ref IDENTITY_CLIENT: IdentityClient = IdentityClient::new();
}
