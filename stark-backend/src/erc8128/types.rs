/// Headers produced by ERC-8128 request signing (RFC 9421 + ERC-191)
#[derive(Debug, Clone)]
pub struct Erc8128SignedHeaders {
    /// RFC 9421 Signature-Input header value, e.g. `eth=(@method @authority @path ...);created=...;expires=...;keyid="erc8128:1:0xABC...";nonce="...";alg="erc191"`
    pub signature_input: String,
    /// RFC 9421 Signature header value, e.g. `eth=:<base64 of 65-byte ECDSA sig>:`
    pub signature: String,
    /// Content-Digest header (only present when body exists), e.g. `sha-256=:<base64>:`
    pub content_digest: Option<String>,
}

/// Compute Content-Digest for an HTTP body using SHA-256.
/// Returns the header value in RFC 9530 format: `sha-256=:<base64>:`
pub fn content_digest_sha256(body: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha2::{Sha256, Digest};

    let hash = Sha256::digest(body);
    let encoded = STANDARD.encode(hash);
    format!("sha-256=:{}:", encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_digest_sha256() {
        let digest = content_digest_sha256(b"hello world");
        assert!(digest.starts_with("sha-256=:"));
        assert!(digest.ends_with(":"));
        // SHA-256 of "hello world" = base64 "uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek="
        assert_eq!(digest, "sha-256=:uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=:");
    }

    #[test]
    fn test_content_digest_empty_body() {
        let digest = content_digest_sha256(b"");
        assert!(digest.starts_with("sha-256=:"));
        assert!(digest.ends_with(":"));
    }
}
