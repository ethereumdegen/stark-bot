/// Fields required to build a SIWA (Sign In With Agent) plaintext message.
pub struct SiwaMessageFields {
    pub domain: String,
    pub address: String,
    pub uri: String,
    pub agent_id: String,
    pub agent_registry: String,
    pub chain_id: u64,
    pub nonce: String,
    pub issued_at: String,
    pub expiration_time: String,
    pub statement: Option<String>,
}

/// Build the SIWA plaintext message in the canonical format.
///
/// The resulting string is intended to be signed with EIP-191 personal_sign.
pub fn build_siwa_message(f: &SiwaMessageFields) -> String {
    let mut msg = format!(
        "{domain} wants you to sign in with your Agent account:\n\
         {address}",
        domain = f.domain,
        address = f.address,
    );

    // Optional statement block (blank line before and after)
    if let Some(ref stmt) = f.statement {
        msg.push_str(&format!("\n\n{}", stmt));
    }

    msg.push_str(&format!(
        "\n\n\
         URI: {uri}\n\
         Version: 1\n\
         Agent ID: {agent_id}\n\
         Agent Registry: {agent_registry}\n\
         Chain ID: {chain_id}\n\
         Nonce: {nonce}\n\
         Issued At: {issued_at}\n\
         Expiration Time: {expiration_time}",
        uri = f.uri,
        agent_id = f.agent_id,
        agent_registry = f.agent_registry,
        chain_id = f.chain_id,
        nonce = f.nonce,
        issued_at = f.issued_at,
        expiration_time = f.expiration_time,
    ));

    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_siwa_message_without_statement() {
        let fields = SiwaMessageFields {
            domain: "example.com".to_string(),
            address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            uri: "https://example.com".to_string(),
            agent_id: "42".to_string(),
            agent_registry: "0xRegistryAddress".to_string(),
            chain_id: 8453,
            nonce: "abc123".to_string(),
            issued_at: "2025-01-01T00:00:00Z".to_string(),
            expiration_time: "2025-01-01T01:00:00Z".to_string(),
            statement: None,
        };

        let msg = build_siwa_message(&fields);

        let expected = "\
example.com wants you to sign in with your Agent account:
0x1234567890abcdef1234567890abcdef12345678

URI: https://example.com
Version: 1
Agent ID: 42
Agent Registry: 0xRegistryAddress
Chain ID: 8453
Nonce: abc123
Issued At: 2025-01-01T00:00:00Z
Expiration Time: 2025-01-01T01:00:00Z";

        assert_eq!(msg, expected);
    }

    #[test]
    fn test_build_siwa_message_with_statement() {
        let fields = SiwaMessageFields {
            domain: "app.example.com".to_string(),
            address: "0xABCDEF".to_string(),
            uri: "https://app.example.com/auth".to_string(),
            agent_id: "7".to_string(),
            agent_registry: "0xReg".to_string(),
            chain_id: 1,
            nonce: "nonce456".to_string(),
            issued_at: "2025-06-15T12:00:00Z".to_string(),
            expiration_time: "2025-06-15T13:00:00Z".to_string(),
            statement: Some("I accept the Terms of Service.".to_string()),
        };

        let msg = build_siwa_message(&fields);

        assert!(msg.starts_with("app.example.com wants you to sign in with your Agent account:\n0xABCDEF"));
        assert!(msg.contains("\n\nI accept the Terms of Service.\n\n"));
        assert!(msg.contains("URI: https://app.example.com/auth\n"));
        assert!(msg.contains("Version: 1\n"));
        assert!(msg.contains("Agent ID: 7\n"));
        assert!(msg.contains("Chain ID: 1\n"));
        assert!(msg.contains("Nonce: nonce456\n"));
        assert!(msg.contains("Issued At: 2025-06-15T12:00:00Z\n"));
        assert!(msg.contains("Expiration Time: 2025-06-15T13:00:00Z"));
    }
}
