//! x402 payment operations — POST with automatic 402 handling.

use crate::erc8128::Erc8128Signer;
use crate::wallet::WalletProvider;
use crate::x402::{X402Signer, PaymentRequirements, PaymentExtra};
use reqwest::header;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

#[derive(Debug, Deserialize)]
struct X402Response {
    accepts: Vec<PaymentOption>,
    #[serde(rename = "x402Version", default = "default_x402_version")]
    x402_version: u8,
}

fn default_x402_version() -> u8 { 1 }

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOptionExtra {
    token: Option<String>,
    address: Option<String>,
    decimals: Option<u8>,
    name: Option<String>,
    version: Option<String>,
    facilitator_signer: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOption {
    scheme: String,
    network: String,
    #[serde(alias = "maxAmountRequired")]
    max_amount_required: String,
    #[serde(alias = "payTo")]
    pay_to: String,
    asset: String,
    #[serde(default)]
    max_timeout_seconds: Option<u64>,
    resource: Option<String>,
    description: Option<String>,
    #[serde(default)]
    extra: Option<PaymentOptionExtra>,
}

pub async fn x402_post(
    url: &str,
    body: &Value,
    headers: &HashMap<String, String>,
    network: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<Value, String> {
    let mut body = body.clone();
    if let Value::String(ref s) = body {
        if let Ok(parsed) = serde_json::from_str::<Value>(s) { body = parsed; }
    }
    if body.is_null() { body = json!({}); }

    let is_x402book = url.contains("x402book.com") || url.contains("x402book.io");
    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

    // Sign with ERC-8128 for x402book
    let erc8128_signed = if is_x402book {
        sign_erc8128(wallet_provider, "POST", url, &body_bytes).await.ok()
    } else {
        None
    };

    let client = crate::http::shared_client();

    let mut request = client.post(url)
        .timeout(Duration::from_secs(60))
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(ref signed) = erc8128_signed {
        request = apply_erc8128_headers(request, signed);
    }
    for (key, value) in headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let initial_response = request.json(&body).send().await
        .map_err(|e| format!("Request failed: {}", e))?;
    let status = initial_response.status();

    if status.as_u16() != 402 {
        let response_body = initial_response.text().await.unwrap_or_default();
        if status.is_success() {
            let val = serde_json::from_str::<Value>(&response_body).unwrap_or(Value::String(response_body));
            return Ok(json!({ "status": status.as_u16(), "payment_required": false, "data": val }));
        }
        return Err(format!("HTTP {}: {}", status, response_body));
    }

    // 402 Payment Required
    let response_body = initial_response.text().await.map_err(|e| format!("Failed to read 402: {}", e))?;
    let payment_info: X402Response = serde_json::from_str(&response_body)
        .map_err(|e| format!("Failed to parse 402: {}", e))?;

    let payment_option = payment_info.accepts.iter()
        .find(|opt| opt.network == network)
        .or_else(|| payment_info.accepts.first())
        .ok_or("No compatible payment option")?
        .clone();

    // Check payment limit
    crate::x402::payment_limits::check_payment_limit(&payment_option.asset, &payment_option.max_amount_required)?;

    // Sign payment
    let signer = X402Signer::new(wallet_provider.clone());
    let wallet_address = signer.address();

    let extra = payment_option.extra.as_ref().map(|e| PaymentExtra {
        token: e.token.clone(), address: e.address.clone(), decimals: e.decimals,
        name: e.name.clone(), version: e.version.clone(), facilitator_signer: e.facilitator_signer.clone(),
    });

    let requirements = PaymentRequirements {
        scheme: payment_option.scheme.clone(), network: payment_option.network.clone(),
        max_amount_required: payment_option.max_amount_required.clone(),
        pay_to_address: payment_option.pay_to.clone(), asset: payment_option.asset.clone(),
        max_timeout_seconds: payment_option.max_timeout_seconds.unwrap_or(300),
        resource: payment_option.resource.clone(), description: payment_option.description.clone(),
        extra,
    };

    let signed_payment = signer.sign_payment(&requirements).await?;
    let payment_json = serde_json::to_string(&signed_payment).map_err(|e| format!("Serialize: {}", e))?;
    let payment_header = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &payment_json);

    // Re-sign ERC-8128 for retry
    let retry_signed = if is_x402book {
        sign_erc8128(wallet_provider, "POST", url, &body_bytes).await.ok()
    } else { None };

    let mut paid_request = client.post(url)
        .timeout(Duration::from_secs(60))
        .header(header::CONTENT_TYPE, "application/json")
        .header("X-PAYMENT", &payment_header);
    if let Some(ref signed) = retry_signed {
        paid_request = apply_erc8128_headers(paid_request, signed);
    }
    for (key, value) in headers {
        paid_request = paid_request.header(key.as_str(), value.as_str());
    }

    let paid_response = paid_request.json(&body).send().await
        .map_err(|e| format!("Paid request failed: {}", e))?;
    let paid_status = paid_response.status();
    let paid_body = paid_response.text().await.unwrap_or_default();

    if !paid_status.is_success() {
        return Err(format!("Payment request failed HTTP {}: {}", paid_status, paid_body));
    }

    let result = serde_json::from_str::<Value>(&paid_body).unwrap_or(Value::String(paid_body));
    Ok(json!({
        "status": paid_status.as_u16(),
        "payment_required": true,
        "data": result,
        "payment": {
            "amount": payment_option.max_amount_required,
            "asset": payment_option.asset,
            "network": payment_option.network,
            "wallet": wallet_address,
        },
    }))
}

async fn sign_erc8128(
    wp: &Arc<dyn WalletProvider>,
    method: &str,
    url: &str,
    body: &[u8],
) -> Result<crate::erc8128::Erc8128SignedHeaders, String> {
    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
    let authority = parsed.host_str().map(|h| {
        if let Some(port) = parsed.port() { format!("{}:{}", h, port) } else { h.to_string() }
    }).unwrap_or_default();
    let path = parsed.path().to_string();
    let query = parsed.query().map(|q| q.to_string());
    let signer = Erc8128Signer::new(wp.clone(), 8453);
    let body_opt = if body.is_empty() { None } else { Some(body) };
    signer.sign_request(method, &authority, &path, query.as_deref(), body_opt).await
}

fn apply_erc8128_headers(
    mut request: reqwest::RequestBuilder,
    signed: &crate::erc8128::Erc8128SignedHeaders,
) -> reqwest::RequestBuilder {
    request = request
        .header("signature-input", &signed.signature_input)
        .header("signature", &signed.signature);
    if let Some(ref digest) = signed.content_digest {
        request = request.header("content-digest", digest);
    }
    request
}
