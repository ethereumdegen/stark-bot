//! SIWA auth and ERC-8128 authenticated fetch.

use crate::erc8128::Erc8128Signer;
use crate::siwa::{build_siwa_message, SiwaMessageFields};
use crate::wallet::WalletProvider;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use url::Url;

/// Make an ERC-8128 signed HTTP request.
pub async fn erc8128_fetch(
    url: &str,
    method: &str,
    body: Option<&Value>,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<Value, String> {
    let parsed = Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
    let authority = parsed.host_str().map(|h| {
        if let Some(port) = parsed.port() { format!("{}:{}", h, port) } else { h.to_string() }
    }).unwrap_or_default();
    let path = parsed.path().to_string();
    let query = parsed.query().map(|q| q.to_string());

    let body_bytes = body.map(|b| serde_json::to_vec(b).unwrap_or_default());
    let body_ref = body_bytes.as_deref();

    let signer = Erc8128Signer::new(wallet_provider.clone(), 8453);
    let signed = signer.sign_request(method, &authority, &path, query.as_deref(), body_ref).await?;

    let client = crate::http::shared_client();
    let mut request = match method.to_uppercase().as_str() {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        _ => client.get(url),
    };

    request = request
        .header("signature-input", &signed.signature_input)
        .header("signature", &signed.signature);
    if let Some(ref digest) = signed.content_digest {
        request = request.header("content-digest", digest);
    }
    if let Some(b) = body {
        request = request.header("content-type", "application/json").json(b);
    }

    let response = request.send().await.map_err(|e| format!("Request failed: {}", e))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, text));
    }

    let val = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    Ok(json!({ "status": status.as_u16(), "data": val }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NonceResponse {
    nonce: String,
    issued_at: Option<String>,
    expiration_time: Option<String>,
}

/// Perform SIWA authentication handshake.
pub async fn siwa_auth(
    server_url: &str,
    domain: &str,
    uri: &str,
    agent_id: Option<&str>,
    wallet_provider: &Arc<dyn WalletProvider>,
    db: Option<&Arc<crate::db::Database>>,
) -> Result<Value, String> {
    let address = wallet_provider.get_address();
    let server_url = server_url.trim_end_matches('/');

    // Resolve agent identity
    let (resolved_agent_id, agent_registry) = resolve_agent_identity(agent_id, db);

    let client = crate::http::shared_client();

    // Request nonce
    let nonce_body = json!({
        "address": address,
        "agentId": resolved_agent_id,
        "agentRegistry": agent_registry,
    });

    let nonce_resp = client.post(format!("{}/siwa/nonce", server_url))
        .header("Content-Type", "application/json")
        .body(nonce_body.to_string())
        .send().await
        .map_err(|e| format!("Nonce request failed: {}", e))?;

    let nonce_status = nonce_resp.status();
    let nonce_text = nonce_resp.text().await.map_err(|e| format!("Read nonce: {}", e))?;
    if !nonce_status.is_success() {
        return Err(format!("Nonce failed ({}): {}", nonce_status, nonce_text));
    }

    let nonce_data: NonceResponse = serde_json::from_str(&nonce_text)
        .map_err(|e| format!("Parse nonce: {}", e))?;

    let now = chrono::Utc::now();
    let issued_at = nonce_data.issued_at
        .unwrap_or_else(|| now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    let expiration_time = nonce_data.expiration_time.unwrap_or_else(|| {
        (now + chrono::Duration::minutes(10))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    });

    let message = build_siwa_message(&SiwaMessageFields {
        domain: domain.to_string(), address: address.clone(), uri: uri.to_string(),
        agent_id: resolved_agent_id.clone(), agent_registry: agent_registry.clone(),
        chain_id: 8453, nonce: nonce_data.nonce, issued_at, expiration_time,
        statement: None,
    });

    let signature = wallet_provider.sign_message(message.as_bytes()).await
        .map_err(|e| format!("Sign failed: {}", e))?;
    let sig_hex = format!("0x{}", hex::encode(signature.to_vec()));

    let verify_body = json!({ "message": message, "signature": sig_hex });
    let verify_resp = client.post(format!("{}/siwa/verify", server_url))
        .header("Content-Type", "application/json")
        .body(verify_body.to_string())
        .send().await
        .map_err(|e| format!("Verify failed: {}", e))?;

    let verify_status = verify_resp.status();
    let verify_text = verify_resp.text().await.unwrap_or_default();
    if !verify_status.is_success() {
        return Err(format!("Verify failed ({}): {}", verify_status, verify_text));
    }

    let receipt = serde_json::from_str::<Value>(&verify_text).unwrap_or(json!(verify_text));

    let mode = if resolved_agent_id.is_some() { "SIWA" } else { "SIWE" };
    Ok(json!({
        "mode": mode,
        "address": address,
        "agent_id": resolved_agent_id,
        "receipt": receipt,
    }))
}

fn resolve_agent_identity(
    agent_id: Option<&str>,
    db: Option<&Arc<crate::db::Database>>,
) -> (Option<String>, Option<String>) {
    if let Some(id) = agent_id {
        return (Some(id.to_string()), None);
    }
    if let Some(db) = db {
        let conn = db.conn();
        if let Ok(row) = conn.query_row(
            "SELECT agent_id, agent_registry FROM agent_identity ORDER BY id DESC LIMIT 1",
            [], |row| Ok((row.get::<_, i64>(0).ok(), row.get::<_, String>(1).ok())),
        ) {
            return (row.0.map(|id| id.to_string()), row.1);
        }
    }
    (None, None)
}
