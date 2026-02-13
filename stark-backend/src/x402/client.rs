//! x402-aware HTTP client

use reqwest::{header, Client, Response};
use serde::Serialize;
use std::sync::Arc;

use super::signer::X402Signer;
use super::types::{PaymentRequired, X402PaymentInfo};
use crate::wallet::WalletProvider;

/// Result of a request that may have required payment
pub struct X402Response {
    pub response: Response,
    pub payment: Option<X402PaymentInfo>,
}

/// HTTP client that automatically handles x402 payment flow
pub struct X402Client {
    client: Client,
    signer: Arc<X402Signer>,
}

impl X402Client {
    /// Create a new x402 client with a WalletProvider (preferred)
    pub fn new(wallet_provider: Arc<dyn WalletProvider>) -> Result<Self, String> {
        let signer = X402Signer::new(wallet_provider.clone());

        log::info!("[X402] Initialized with wallet address: {}", signer.address());

        Ok(Self {
            client: crate::http::shared_client().clone(),
            signer: Arc::new(signer),
        })
    }

    /// Create a new x402 client with a private key (backward compatible)
    pub fn from_private_key(private_key: &str) -> Result<Self, String> {
        let signer = X402Signer::from_private_key(private_key)?;

        log::info!("[X402] Initialized with wallet address: {}", signer.address());

        Ok(Self {
            client: crate::http::shared_client().clone(),
            signer: Arc::new(signer),
        })
    }

    /// Get the wallet address
    pub fn wallet_address(&self) -> String {
        self.signer.address()
    }

    /// Make a POST request with automatic x402 payment handling
    /// Returns both the response and payment info if a payment was made
    pub async fn post_with_payment<T: Serialize>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<X402Response, String> {
        log::info!("[X402] Making POST request to {}", url);

        // First request without payment
        let initial_response = self.client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_402_response(initial_response, || {
            self.client
                .post(url)
                .header(header::CONTENT_TYPE, "application/json")
                .json(body)
        }).await
    }

    /// Make a GET request with automatic x402 payment handling
    /// Returns both the response and payment info if a payment was made
    pub async fn get_with_payment(
        &self,
        url: &str,
    ) -> Result<X402Response, String> {
        log::info!("[X402] Making GET request to {}", url);

        // First request without payment
        let initial_response = self.client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_402_response(initial_response, || {
            self.client.get(url)
        }).await
    }

    /// Handle 402 response and retry with payment if needed
    async fn handle_402_response<F>(
        &self,
        initial_response: Response,
        build_request: F,
    ) -> Result<X402Response, String>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        // Check if payment is required
        if initial_response.status().as_u16() != 402 {
            log::info!("[X402] No payment required, status: {}", initial_response.status());
            return Ok(X402Response {
                response: initial_response,
                payment: None,
            });
        }

        log::info!("[X402] Received 402 Payment Required");

        // Get payment requirements from header
        let payment_header = initial_response
            .headers()
            .get("payment-required")
            .or_else(|| initial_response.headers().get("PAYMENT-REQUIRED"))
            .ok_or_else(|| "402 response missing payment-required header".to_string())?
            .to_str()
            .map_err(|e| format!("Invalid payment-required header: {}", e))?;

        let payment_required = PaymentRequired::from_base64(payment_header)?;

        log::info!(
            "[X402] Payment requirements: {} {} to {}",
            payment_required.accepts.first().map(|a| a.max_amount_required.as_str()).unwrap_or("?"),
            payment_required.accepts.first().map(|a| a.asset.as_str()).unwrap_or("?"),
            payment_required.accepts.first().map(|a| a.pay_to_address.as_str()).unwrap_or("?")
        );

        // Get the first (and typically only) payment option
        let requirements = payment_required.accepts.first()
            .ok_or_else(|| "No payment options in 402 response".to_string())?;

        // Check payment limit before signing
        super::payment_limits::check_payment_limit(
            &requirements.asset,
            &requirements.max_amount_required,
        )?;

        // Create payment info before signing
        let payment_info = X402PaymentInfo::from_requirements(requirements);

        // Sign the payment using V2 format (required by Kimi/AI relay)
        let payment_payload = self.signer.sign_payment_v2(requirements).await?;
        let payment_header_value = payment_payload.to_base64()?;

        log::info!(
            "[X402] Signed payment for {} {} to {}, retrying request",
            payment_info.amount_formatted,
            payment_info.asset,
            payment_info.pay_to
        );

        // Retry with payment
        let paid_response = build_request()
            .header("X-PAYMENT", payment_header_value)
            .send()
            .await
            .map_err(|e| format!("Paid request failed: {}", e))?;

        log::info!("[X402] Payment sent, response status: {}", paid_response.status());

        // Try to extract transaction hash from response headers
        // x402 servers may return it in various headers
        let tx_hash = paid_response
            .headers()
            .get("x-payment-transaction")
            .or_else(|| paid_response.headers().get("X-Payment-Transaction"))
            .or_else(|| paid_response.headers().get("x-transaction-hash"))
            .or_else(|| paid_response.headers().get("X-Transaction-Hash"))
            .or_else(|| paid_response.headers().get("x-payment-tx"))
            .or_else(|| paid_response.headers().get("X-Payment-Tx"))
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Update payment info with tx_hash if available
        let payment_info = if let Some(hash) = tx_hash {
            log::info!("[X402] Received transaction hash: {}", hash);
            payment_info.with_tx_hash(hash)
        } else if paid_response.status().is_success() {
            // Payment succeeded but no tx_hash in headers - mark as confirmed anyway
            log::info!("[X402] Payment confirmed (no tx_hash in response headers)");
            payment_info.mark_confirmed()
        } else {
            // Payment may have failed
            log::warn!("[X402] Payment response status: {}, keeping as pending", paid_response.status());
            payment_info
        };

        Ok(X402Response {
            response: paid_response,
            payment: Some(payment_info),
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

    let client = crate::http::shared_client();
    let response = client
        .post("https://mainnet.base.org")
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
