//! ERC-8128 request signing: RFC 9421 HTTP Message Signatures with ERC-191 signing
//!
//! Signs outgoing HTTP requests so ERC-8128-aware servers can verify
//! the caller's Ethereum identity via `ecrecover`.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;

use super::types::{content_digest_sha256, Erc8128SignedHeaders};
use crate::wallet::WalletProvider;

/// Signs outgoing HTTP requests per ERC-8128 (RFC 9421 + ERC-191).
pub struct Erc8128Signer {
    wallet_provider: Arc<dyn WalletProvider>,
    chain_id: u64,
}

impl Erc8128Signer {
    pub fn new(wallet_provider: Arc<dyn WalletProvider>, chain_id: u64) -> Self {
        Self {
            wallet_provider,
            chain_id,
        }
    }

    /// Wallet address (checksummed hex)
    pub fn address(&self) -> String {
        self.wallet_provider.get_address()
    }

    /// Sign an outgoing HTTP request and return the headers to attach.
    ///
    /// * `method`    — HTTP method, e.g. `"GET"`, `"POST"`
    /// * `authority` — Host (+ optional port), e.g. `"api.example.com"`
    /// * `path`      — Path component, e.g. `"/v1/data"`
    /// * `query`     — Query string WITHOUT leading `?`, or `None`
    /// * `body`      — Request body bytes (for POST/PUT), or `None`
    pub async fn sign_request(
        &self,
        method: &str,
        authority: &str,
        path: &str,
        query: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<Erc8128SignedHeaders, String> {
        // 1. Content-Digest (only when body is present)
        let content_digest = body.map(|b| content_digest_sha256(b));

        // 2. Covered components
        let mut components: Vec<String> = vec![
            "@method".to_string(),
            "@authority".to_string(),
            "@path".to_string(),
        ];
        if query.is_some() {
            components.push("@query".to_string());
        }
        if content_digest.is_some() {
            components.push("content-digest".to_string());
        }

        // 3. Signature parameters
        let nonce = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let created = now;
        let expires = now + 300; // 5 minutes
        let keyid = format!(
            "erc8128:{}:{}",
            self.chain_id,
            self.wallet_provider.get_address()
        );

        // 4. Build Signature-Input value
        //    Format: eth=(<components>);created=<t>;expires=<t>;keyid="<kid>";nonce="<n>";alg="erc191"
        let components_str = components
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(" ");

        let sig_input = format!(
            "eth=({});created={};expires={};keyid=\"{}\";nonce=\"{}\";alg=\"erc191\"",
            components_str, created, expires, keyid, nonce,
        );

        // 5. Build RFC 9421 signature base
        let mut base_lines: Vec<String> = Vec::new();
        for comp in &components {
            let value = match comp.as_str() {
                "@method" => method.to_uppercase(),
                "@authority" => authority.to_lowercase(),
                "@path" => path.to_string(),
                "@query" => format!("?{}", query.unwrap_or("")),
                "content-digest" => content_digest.clone().unwrap_or_default(),
                _ => String::new(),
            };
            base_lines.push(format!("\"{}\": {}", comp, value));
        }

        // Signature params line (the final component of the signature base)
        let sig_params = format!(
            "({});created={};expires={};keyid=\"{}\";nonce=\"{}\";alg=\"erc191\"",
            components_str, created, expires, keyid, nonce,
        );
        base_lines.push(format!("\"@signature-params\": {}", sig_params));

        let signature_base = base_lines.join("\n");

        log::debug!(
            "[ERC8128] Signature base ({} bytes):\n{}",
            signature_base.len(),
            signature_base
        );

        // 6. Sign with ERC-191 (wallet_provider.sign_message does the "\x19Ethereum Signed Message" prefix)
        let sig = self
            .wallet_provider
            .sign_message(signature_base.as_bytes())
            .await
            .map_err(|e| format!("ERC-191 signing failed: {}", e))?;

        // 7. Encode the 65-byte signature (r ++ s ++ v) as base64
        let mut sig_bytes = [0u8; 65];
        sig.r.to_big_endian(&mut sig_bytes[0..32]);
        sig.s.to_big_endian(&mut sig_bytes[32..64]);
        sig_bytes[64] = sig.v as u8;
        let sig_b64 = BASE64.encode(sig_bytes);

        let signature_header = format!("eth=:{}:", sig_b64);

        log::info!(
            "[ERC8128] Signed request {} {} as {} (chain {})",
            method,
            path,
            self.wallet_provider.get_address(),
            self.chain_id
        );

        Ok(Erc8128SignedHeaders {
            signature_input: sig_input,
            signature: signature_header,
            content_digest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ethers::types::{Signature, H256, transaction::eip2718::TypedTransaction};
    use ethers::signers::{LocalWallet, Signer};

    /// Minimal WalletProvider backed by a deterministic LocalWallet for testing
    struct TestWallet {
        wallet: LocalWallet,
    }

    #[async_trait]
    impl WalletProvider for TestWallet {
        async fn sign_message(&self, message: &[u8]) -> Result<Signature, String> {
            self.wallet
                .sign_message(message)
                .await
                .map_err(|e| e.to_string())
        }
        async fn sign_transaction(&self, _tx: &TypedTransaction) -> Result<Signature, String> {
            unimplemented!()
        }
        async fn sign_hash(&self, _hash: H256) -> Result<Signature, String> {
            unimplemented!()
        }
        async fn sign_typed_data(&self, _data: &serde_json::Value) -> Result<Signature, String> {
            unimplemented!()
        }
        fn get_address(&self) -> String {
            format!("{:#x}", self.wallet.address())
        }
        fn mode_name(&self) -> &'static str {
            "test"
        }
    }

    fn test_wallet() -> Arc<dyn WalletProvider> {
        // Deterministic key for reproducible tests
        let wallet: LocalWallet = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .parse()
            .unwrap();
        Arc::new(TestWallet { wallet })
    }

    #[tokio::test]
    async fn test_sign_get_request() {
        let signer = Erc8128Signer::new(test_wallet(), 1);
        let headers = signer
            .sign_request("GET", "api.example.com", "/v1/data", None, None)
            .await
            .unwrap();

        // Signature-Input must start with `eth=(`
        assert!(headers.signature_input.starts_with("eth=("));
        assert!(headers.signature_input.contains("@method"));
        assert!(headers.signature_input.contains("@authority"));
        assert!(headers.signature_input.contains("@path"));
        assert!(headers.signature_input.contains("alg=\"erc191\""));
        assert!(headers.signature_input.contains("keyid=\"erc8128:1:"));

        // Signature must be base64 wrapped in `eth=:...:` format
        assert!(headers.signature.starts_with("eth=:"));
        assert!(headers.signature.ends_with(":"));

        // No Content-Digest for GET without body
        assert!(headers.content_digest.is_none());
    }

    #[tokio::test]
    async fn test_sign_post_with_body() {
        let signer = Erc8128Signer::new(test_wallet(), 8453);
        let body = b"{\"amount\": 100}";
        let headers = signer
            .sign_request("POST", "pay.example.com", "/charge", None, Some(body))
            .await
            .unwrap();

        // Should have Content-Digest
        assert!(headers.content_digest.is_some());
        let digest = headers.content_digest.unwrap();
        assert!(digest.starts_with("sha-256=:"));

        // Signature-Input should cover content-digest
        assert!(headers.signature_input.contains("content-digest"));
    }

    #[tokio::test]
    async fn test_sign_with_query() {
        let signer = Erc8128Signer::new(test_wallet(), 1);
        let headers = signer
            .sign_request("GET", "api.example.com", "/search", Some("q=test&limit=10"), None)
            .await
            .unwrap();

        assert!(headers.signature_input.contains("@query"));
    }

    #[tokio::test]
    async fn test_signature_is_65_bytes_base64() {
        let signer = Erc8128Signer::new(test_wallet(), 1);
        let headers = signer
            .sign_request("GET", "example.com", "/", None, None)
            .await
            .unwrap();

        // Extract base64 between `eth=:` and trailing `:`
        let b64 = headers
            .signature
            .strip_prefix("eth=:")
            .unwrap()
            .strip_suffix(":")
            .unwrap();
        let decoded = BASE64.decode(b64).unwrap();
        assert_eq!(decoded.len(), 65, "ECDSA signature must be 65 bytes (r+s+v)");
    }
}
