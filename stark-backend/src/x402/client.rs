//! Credits-aware HTTP client for inference-super-router
//!
//! For defirelay endpoints, payment is ALWAYS via credits (session-based Bearer
//! tokens or ERC-8128 signed headers). The client NEVER falls through to x402
//! on-chain payment — if credits are exhausted, the request fails with an error.
//!
//! For custom endpoints, no payment protocol is used (direct API key auth).

use reqwest::{header, Client, Response};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::signer::X402Signer;
use super::types::{PaymentRequired, X402PaymentInfo};
use crate::credits_session::CreditsSessionClient;
use crate::erc8128::Erc8128Signer;
use crate::wallet::WalletProvider;

/// Payment mode controlling how the client handles payment
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaymentMode {
    /// Credits only — session-based Bearer tokens + ERC-8128 signed headers.
    /// NEVER falls through to x402 on-chain payment.
    Credits,
    /// Custom endpoint — no payment protocol (direct API key auth)
    CustomEndpoint,
}

/// Result of a request that may have required payment
pub struct X402Response {
    pub response: Response,
    pub payment: Option<X402PaymentInfo>,
}

/// HTTP client that automatically handles x402 payment flow
/// and session-based credits.
pub struct X402Client {
    client: Client,
    signer: Arc<X402Signer>,
    wallet_provider: Arc<dyn WalletProvider>,
    erc8128_signer: Erc8128Signer,
    /// Hosts known to support ERC-8128 credits (discovered via `x-erc8128-credits` header).
    erc8128_credits_hosts: Arc<Mutex<HashSet<String>>>,
    /// Payment mode controlling credit vs x402 negotiation
    payment_mode: PaymentMode,
    /// Optional session client for Bearer-token credits (reduces Privy signing to ~1/hour)
    credits_session: Option<Arc<CreditsSessionClient>>,
}

impl X402Client {
    /// Create a new x402 client with a WalletProvider (preferred)
    pub fn new(wallet_provider: Arc<dyn WalletProvider>) -> Result<Self, String> {
        let signer = X402Signer::new(wallet_provider.clone());
        let erc8128_signer = Erc8128Signer::new(wallet_provider.clone(), 8453); // Base mainnet

        log::info!("[X402] Initialized with wallet address: {}", signer.address());

        Ok(Self {
            client: crate::http::shared_client().clone(),
            signer: Arc::new(signer),
            wallet_provider,
            erc8128_signer,
            erc8128_credits_hosts: Arc::new(Mutex::new(HashSet::new())),
            payment_mode: PaymentMode::Credits,
            credits_session: None,
        })
    }

    /// Set the payment mode (builder pattern)
    pub fn with_payment_mode(mut self, mode: PaymentMode) -> Self {
        self.payment_mode = mode;
        self
    }

    /// Set the credits session client (builder pattern)
    pub fn with_credits_session(mut self, session: Arc<CreditsSessionClient>) -> Self {
        self.credits_session = Some(session);
        self
    }

    /// Create a new x402 client with a private key (backward compatible)
    pub fn from_private_key(private_key: &str) -> Result<Self, String> {
        // For backward compat: create an EnvWalletProvider-equivalent
        // This path doesn't support ERC-8128 credits (no WalletProvider)
        let signer = X402Signer::from_private_key(private_key)?;

        log::info!("[X402] Initialized with wallet address: {} (private key mode, ERC-8128 credits disabled)", signer.address());

        // Create a minimal wallet provider from private key for ERC-8128
        let wp = crate::wallet::EnvWalletProvider::from_private_key(private_key)
            .map_err(|e| format!("Failed to create wallet provider: {}", e))?;
        let wp: Arc<dyn WalletProvider> = Arc::new(wp);
        let erc8128_signer = Erc8128Signer::new(wp.clone(), 8453);

        Ok(Self {
            client: crate::http::shared_client().clone(),
            signer: Arc::new(signer),
            wallet_provider: wp,
            erc8128_signer,
            erc8128_credits_hosts: Arc::new(Mutex::new(HashSet::new())),
            payment_mode: PaymentMode::Credits,
            credits_session: None,
        })
    }

    /// Get the wallet address
    pub fn wallet_address(&self) -> String {
        self.signer.address()
    }

    /// Whether a credits session client is attached.
    pub fn has_credits_session(&self) -> bool {
        self.credits_session.is_some()
    }

    /// Check if a host is known to support ERC-8128 credits.
    fn is_erc8128_credits_host(&self, url: &str) -> bool {
        let host = extract_host(url);
        self.erc8128_credits_hosts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&host)
    }

    /// Remember that a host supports ERC-8128 credits.
    fn mark_erc8128_credits_host(&self, url: &str) {
        let host = extract_host(url);
        log::info!("[X402] Discovered ERC-8128 credits support for host: {}", host);
        self.erc8128_credits_hosts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(host);
    }

    /// Build a request with ERC-8128 signed headers attached.
    async fn build_erc8128_post_request(
        &self,
        url: &str,
        body_bytes: &[u8],
    ) -> Result<reqwest::RequestBuilder, String> {
        let (authority, path, query) = parse_url_parts(url);

        let signed = self
            .erc8128_signer
            .sign_request("POST", &authority, &path, query.as_deref(), Some(body_bytes))
            .await?;

        let mut req = self
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .header("signature-input", &signed.signature_input)
            .header("signature", &signed.signature);

        if let Some(ref digest) = signed.content_digest {
            req = req.header("content-digest", digest);
        }

        Ok(req)
    }

    /// Make a POST request with credits-based payment (session Bearer or ERC-8128).
    ///
    /// For `Credits` mode: tries session token first, then ERC-8128 signed headers.
    /// NEVER falls through to x402 on-chain payment — returns error if credits fail.
    ///
    /// For `CustomEndpoint` mode: sends a plain request with no payment protocol.
    pub async fn post_with_payment<T: Serialize>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<X402Response, String> {
        log::info!("[Credits] Making POST request to {} (mode={:?})", url, self.payment_mode);

        // ── CustomEndpoint mode: no payment protocol ──
        if self.payment_mode == PaymentMode::CustomEndpoint {
            let response = self
                .client
                .post(url)
                .header(header::CONTENT_TYPE, "application/json")
                .json(body)
                .send()
                .await
                .map_err(|e| format!("Request failed: {}", e))?;

            return Ok(X402Response {
                response,
                payment: None,
            });
        }

        // ── Credits mode ──
        // Serialize body upfront (needed for ERC-8128 Content-Digest)
        let body_bytes = serde_json::to_vec(body)
            .map_err(|e| format!("Failed to serialize request body: {}", e))?;

        // 1. Session-based credits path (preferred when available)
        if let Some(ref session) = self.credits_session {
            match self.try_credits_session(session, url, &body_bytes).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    log::warn!("[Credits] Session path failed: {}, trying ERC-8128 fallback", e);
                }
            }
        }

        // 2. Proactive ERC-8128 path (known credits host)
        if self.is_erc8128_credits_host(url) {
            log::info!("[Credits] Proactively sending ERC-8128 headers (cached credits host)");

            match self.build_erc8128_post_request(url, &body_bytes).await {
                Ok(req) => {
                    match req.body(body_bytes.clone()).send().await {
                        Ok(response) if response.status().as_u16() != 402 => {
                            log::info!(
                                "[Credits] ERC-8128 credits accepted (proactive), status: {}",
                                response.status()
                            );
                            return Ok(X402Response {
                                response,
                                payment: None,
                            });
                        }
                        Ok(_) => {
                            return Err("Insufficient credits (ERC-8128 proactive got 402). Top up your credits balance.".to_string());
                        }
                        Err(e) => {
                            log::warn!("[Credits] ERC-8128 proactive request failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[Credits] ERC-8128 signing failed: {}", e);
                }
            }
        }

        // 3. Initial request without payment headers (discovers ERC-8128 support)
        let initial_response = self
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body_bytes.clone())
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if initial_response.status().as_u16() != 402 {
            return Ok(X402Response {
                response: initial_response,
                payment: None,
            });
        }

        // 4. Got 402: try ERC-8128 credits discovery
        if has_erc8128_credits_header(&initial_response) {
            self.mark_erc8128_credits_host(url);

            log::info!("[Credits] Discovered ERC-8128 credits, trying signed retry");

            match self.build_erc8128_post_request(url, &body_bytes).await {
                Ok(req) => {
                    match req.body(body_bytes.clone()).send().await {
                        Ok(response) if response.status().as_u16() != 402 => {
                            log::info!(
                                "[Credits] ERC-8128 credits accepted (discovered), status: {}",
                                response.status()
                            );
                            return Ok(X402Response {
                                response,
                                payment: None,
                            });
                        }
                        Ok(_) => {
                            return Err("Insufficient credits (ERC-8128 discovered but got 402). Top up your credits balance.".to_string());
                        }
                        Err(e) => {
                            return Err(format!("Credits request failed: {}", e));
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("ERC-8128 signing failed: {}", e));
                }
            }
        }

        // No credits support detected on this endpoint — fail
        Err("Endpoint returned 402 but does not support credits (no x-erc8128-credits header). Cannot proceed without credits.".to_string())
    }

    /// Try the session-based credits path: send Bearer token, retry on 401.
    async fn try_credits_session(
        &self,
        session: &CreditsSessionClient,
        url: &str,
        body_bytes: &[u8],
    ) -> Result<X402Response, String> {
        let token = session.get_token().await?;

        log::info!("[X402] Sending request with Bearer session token");
        let response = self
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .body(body_bytes.to_vec())
            .send()
            .await
            .map_err(|e| format!("Session request failed: {}", e))?;

        let status = response.status().as_u16();

        if status == 401 {
            // Session expired or invalid — invalidate and retry once
            log::info!("[X402] Bearer token rejected (401), invalidating and retrying");
            session.invalidate().await;

            let new_token = session.get_token().await?;
            let retry_response = self
                .client
                .post(url)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", new_token))
                .body(body_bytes.to_vec())
                .send()
                .await
                .map_err(|e| format!("Session retry request failed: {}", e))?;

            let retry_status = retry_response.status().as_u16();
            if retry_status == 401 {
                return Err("Session re-establishment failed (401 on retry)".to_string());
            }
            if retry_status == 402 {
                return Err("Insufficient credits (402 after session auth)".to_string());
            }

            log::info!("[X402] Session retry succeeded, status: {}", retry_status);
            return Ok(X402Response {
                response: retry_response,
                payment: None,
            });
        }

        if status == 402 {
            return Err("Insufficient credits (402 with session auth)".to_string());
        }

        log::info!("[X402] Credits session accepted, status: {}", status);
        Ok(X402Response {
            response,
            payment: None,
        })
    }

    /// Make a GET request (no payment handling for GET requests)
    pub async fn get_with_payment(
        &self,
        url: &str,
    ) -> Result<X402Response, String> {
        log::info!("[Credits] Making GET request to {}", url);

        let response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        Ok(X402Response {
            response,
            payment: None,
        })
    }

}

impl X402Client {
    /// Make a regular POST request without x402 payment handling
    /// Used for custom RPC endpoints that don't require payment
    pub async fn post_regular<T: Serialize>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<X402Response, String> {
        log::info!("[X402] Making regular POST request to {} (no payment)", url);

        let response = self.client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        Ok(X402Response {
            response,
            payment: None,
        })
    }

    /// Make a regular GET request without x402 payment handling
    pub async fn get_regular(
        &self,
        url: &str,
    ) -> Result<X402Response, String> {
        log::info!("[X402] Making regular GET request to {} (no payment)", url);

        let response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        Ok(X402Response {
            response,
            payment: None,
        })
    }
}

/// Check if a URL is a defirelay endpoint that uses x402
pub fn is_x402_endpoint(url: &str) -> bool {
    url.contains("defirelay.com") || url.contains("defirelay.io")
}

/// Check USDC balance on Base for a wallet address.
/// Returns the balance in raw units (6 decimals for USDC).
/// Used to detect insufficient funds after an x402 payment failure.
pub async fn check_usdc_balance(wallet_address: &str) -> Result<ethers::types::U256, String> {
    let address: ethers::types::Address = wallet_address
        .parse()
        .map_err(|e| format!("Invalid wallet address: {}", e))?;

    let usdc_address: ethers::types::Address = super::types::USDC_ADDRESS
        .parse()
        .map_err(|e| format!("Invalid USDC address: {}", e))?;

    let call_data = super::erc20::encode_balance_of(address);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "to": format!("{:?}", usdc_address),
            "data": format!("0x{}", hex::encode(&call_data))
        }, "latest"],
        "id": 1
    });

    let resolved = crate::tools::rpc_config::resolve_rpc_readonly("base");
    let client = crate::http::shared_client();
    let response = client
        .post(&resolved.url)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    let result = body
        .get("result")
        .and_then(|r| r.as_str())
        .ok_or_else(|| {
            let error = body.get("error").map(|e| e.to_string()).unwrap_or_default();
            format!("RPC error: {}", error)
        })?;

    let bytes = hex::decode(result.trim_start_matches("0x"))
        .map_err(|e| format!("Failed to decode balance hex: {}", e))?;

    super::erc20::decode_balance(&bytes)
}

/// Parse a 402 response and sign an x402 payment, returning the X-PAYMENT header value.
///
/// Tries to parse payment requirements from:
/// 1. `payment-required` / `PAYMENT-REQUIRED` response header (base64-encoded)
/// 2. Response body as JSON (direct `PaymentRequired` structure)
///
/// Returns `(x_payment_header_value, payment_info)` on success.
pub async fn sign_402_payment(
    response_body: &str,
    response_headers: &reqwest::header::HeaderMap,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<(String, X402PaymentInfo), String> {
    // Try header first (base64-encoded)
    let payment_required = if let Some(header_val) = response_headers
        .get("payment-required")
        .or_else(|| response_headers.get("PAYMENT-REQUIRED"))
        .and_then(|v| v.to_str().ok())
    {
        PaymentRequired::from_base64(header_val)?
    } else {
        // Fall back to response body (JSON)
        serde_json::from_str::<PaymentRequired>(response_body)
            .map_err(|e| format!("Failed to parse 402 payment requirements from body: {}", e))?
    };

    let requirements = payment_required
        .accepts
        .first()
        .ok_or_else(|| "No payment options in 402 response".to_string())?;

    // Check payment limit
    super::payment_limits::check_payment_limit(
        &requirements.asset,
        &requirements.max_amount_required,
    )?;

    let payment_info = X402PaymentInfo::from_requirements(requirements);

    // Sign the payment
    let signer = X402Signer::new(wallet_provider.clone());
    let payment_payload = signer.sign_payment_v2(requirements).await?;
    let header_value = payment_payload.to_base64()?;

    log::info!(
        "[X402] Signed payment for {} {} to {}",
        payment_info.amount_formatted,
        payment_info.asset,
        payment_info.pay_to
    );

    Ok((header_value, payment_info))
}

/// Result of an x402-aware request that may have required payment.
pub struct X402RetryResult {
    /// The final response (after payment if needed)
    pub response: Response,
    /// Payment info if an x402 payment was made
    pub payment: Option<X402PaymentInfo>,
}

/// Handle a 402 response by signing an x402 payment and retrying the request.
///
/// `build_retry_request` is a closure that builds a fresh `RequestBuilder` for the retry.
/// The caller is responsible for attaching all original headers/body to the retry builder.
/// This function only adds the `X-PAYMENT` header.
///
/// Returns `Ok(X402RetryResult)` with the paid response on success,
/// or `Err(error_message)` if payment fails.
pub async fn retry_with_x402_payment<F>(
    initial_response: Response,
    wallet_provider: &Arc<dyn WalletProvider>,
    build_retry_request: F,
) -> Result<X402RetryResult, String>
where
    F: FnOnce() -> reqwest::RequestBuilder,
{
    let response_headers = initial_response.headers().clone();
    let body_402 = initial_response
        .text()
        .await
        .map_err(|e| format!("Failed to read 402 body: {}", e))?;

    let (x_payment_header, payment_info) =
        sign_402_payment(&body_402, &response_headers, wallet_provider).await?;

    log::info!(
        "[X402] Retrying request with payment ({} {} to {})",
        payment_info.amount_formatted,
        payment_info.asset,
        payment_info.pay_to
    );

    let retry_req = build_retry_request().header("X-PAYMENT", x_payment_header);

    let paid_response = retry_req
        .send()
        .await
        .map_err(|e| format!("Paid request failed: {}", e))?;

    // Extract tx hash from response headers
    let tx_hash = paid_response
        .headers()
        .get("x-transaction-hash")
        .or_else(|| paid_response.headers().get("X-Transaction-Hash"))
        .or_else(|| paid_response.headers().get("x-payment-transaction"))
        .or_else(|| paid_response.headers().get("X-Payment-Transaction"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let payment_info = if let Some(hash) = tx_hash {
        payment_info.with_tx_hash(hash)
    } else if paid_response.status().is_success() {
        payment_info.mark_confirmed()
    } else {
        payment_info
    };

    Ok(X402RetryResult {
        response: paid_response,
        payment: Some(payment_info),
    })
}

/// Check if a response has the `x-erc8128-credits` header indicating
/// the endpoint supports ERC-8128 credits as an alternative to x402.
fn has_erc8128_credits_header(response: &Response) -> bool {
    response
        .headers()
        .get("x-erc8128-credits")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true")
}

/// Extract host from a URL string (e.g. "https://api.example.com:8080/path" → "api.example.com:8080")
fn extract_host(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Parse a URL into (authority, path, query) components for ERC-8128 signing.
fn parse_url_parts(url: &str) -> (String, String, Option<String>) {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    let (authority, path_and_query) = match without_scheme.find('/') {
        Some(idx) => (
            without_scheme[..idx].to_string(),
            &without_scheme[idx..],
        ),
        None => (without_scheme.to_string(), "/"),
    };

    let (path, query) = match path_and_query.find('?') {
        Some(idx) => (
            path_and_query[..idx].to_string(),
            Some(path_and_query[idx + 1..].to_string()),
        ),
        None => (path_and_query.to_string(), None),
    };

    (authority, path, query)
}
