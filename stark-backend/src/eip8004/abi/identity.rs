//! Identity Registry ABI encoding
//!
//! ERC-721 based agent identity registry with EIP-8004 extensions.

use super::common::*;

// Function selectors
pub const REGISTER_SELECTOR: [u8; 4] = [0x82, 0xfb, 0xdc, 0x9c]; // register(string)
pub const TOKEN_URI_SELECTOR: [u8; 4] = [0xc8, 0x7b, 0x56, 0xdd]; // tokenURI(uint256)
pub const OWNER_OF_SELECTOR: [u8; 4] = [0x63, 0x52, 0x21, 0x1e]; // ownerOf(uint256)
pub const TOTAL_SUPPLY_SELECTOR: [u8; 4] = [0x18, 0x16, 0x0d, 0xdd]; // totalSupply()
pub const SET_AGENT_URI_SELECTOR: [u8; 4] = [0x93, 0x1d, 0xcb, 0x66]; // setAgentURI(uint256,string)
pub const GET_AGENT_WALLET_SELECTOR: [u8; 4] = [0xa8, 0x7d, 0x94, 0x2c]; // getAgentWallet(uint256)
pub const SET_AGENT_WALLET_SELECTOR: [u8; 4] = [0xd4, 0x5c, 0x44, 0x35]; // setAgentWallet(uint256,address,uint256,bytes)
pub const GET_METADATA_SELECTOR: [u8; 4] = [0xa3, 0xdb, 0x80, 0xe2]; // getMetadata(uint256,string)
pub const SET_METADATA_SELECTOR: [u8; 4] = [0x5d, 0x3a, 0x1f, 0x9d]; // setMetadata(uint256,string,bytes)
pub const BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31]; // balanceOf(address)
pub const TOKEN_OF_OWNER_BY_INDEX_SELECTOR: [u8; 4] = [0x2f, 0x74, 0x5c, 0x59]; // tokenOfOwnerByIndex(address,uint256)

/// Encode register(string agentURI) call
pub fn encode_register(agent_uri: &str) -> Vec<u8> {
    let mut calldata = Vec::new();

    // Function selector
    calldata.extend_from_slice(&REGISTER_SELECTOR);

    // Offset to string data (32 bytes from start of params)
    calldata.extend(encode_uint256(32));

    // String data
    calldata.extend(encode_string(agent_uri));

    calldata
}

/// Encode tokenURI(uint256 tokenId) call
pub fn encode_token_uri(token_id: u64) -> Vec<u8> {
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&TOKEN_URI_SELECTOR);
    calldata.extend(encode_uint256(token_id));
    calldata
}

/// Encode ownerOf(uint256 tokenId) call
pub fn encode_owner_of(token_id: u64) -> Vec<u8> {
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&OWNER_OF_SELECTOR);
    calldata.extend(encode_uint256(token_id));
    calldata
}

/// Encode totalSupply() call
pub fn encode_total_supply() -> Vec<u8> {
    TOTAL_SUPPLY_SELECTOR.to_vec()
}

/// Encode setAgentURI(uint256 agentId, string newURI) call
pub fn encode_set_agent_uri(agent_id: u64, new_uri: &str) -> Vec<u8> {
    let mut calldata = Vec::new();

    calldata.extend_from_slice(&SET_AGENT_URI_SELECTOR);
    calldata.extend(encode_uint256(agent_id));

    // Offset to string data (64 bytes from start of params)
    calldata.extend(encode_uint256(64));

    // String data
    calldata.extend(encode_string(new_uri));

    calldata
}

/// Encode getAgentWallet(uint256 agentId) call
pub fn encode_get_agent_wallet(agent_id: u64) -> Vec<u8> {
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&GET_AGENT_WALLET_SELECTOR);
    calldata.extend(encode_uint256(agent_id));
    calldata
}

/// Encode setAgentWallet(uint256 agentId, address newWallet, uint256 deadline, bytes signature) call
pub fn encode_set_agent_wallet(
    agent_id: u64,
    new_wallet: &str,
    deadline: u64,
    signature: &[u8],
) -> Vec<u8> {
    let mut calldata = Vec::new();

    calldata.extend_from_slice(&SET_AGENT_WALLET_SELECTOR);
    calldata.extend(encode_uint256(agent_id));
    calldata.extend(encode_address(new_wallet));
    calldata.extend(encode_uint256(deadline));

    // Offset to bytes data
    calldata.extend(encode_uint256(128));

    // Bytes length and data
    calldata.extend(encode_uint256(signature.len() as u64));
    calldata.extend_from_slice(signature);

    // Pad to 32-byte boundary
    let padding = (32 - (signature.len() % 32)) % 32;
    calldata.extend(vec![0u8; padding]);

    calldata
}

/// Encode getMetadata(uint256 agentId, string metadataKey) call
pub fn encode_get_metadata(agent_id: u64, metadata_key: &str) -> Vec<u8> {
    let mut calldata = Vec::new();

    calldata.extend_from_slice(&GET_METADATA_SELECTOR);
    calldata.extend(encode_uint256(agent_id));

    // Offset to string data
    calldata.extend(encode_uint256(64));

    // String data
    calldata.extend(encode_string(metadata_key));

    calldata
}

/// Encode setMetadata(uint256 agentId, string metadataKey, bytes metadataValue) call
pub fn encode_set_metadata(agent_id: u64, metadata_key: &str, metadata_value: &[u8]) -> Vec<u8> {
    let mut calldata = Vec::new();

    calldata.extend_from_slice(&SET_METADATA_SELECTOR);
    calldata.extend(encode_uint256(agent_id));

    // Calculate offsets
    let string_offset = 96; // After agentId (32) + string offset (32) + bytes offset (32)
    let string_encoded = encode_string(metadata_key);
    let bytes_offset = string_offset + string_encoded.len();

    calldata.extend(encode_uint256(string_offset as u64));
    calldata.extend(encode_uint256(bytes_offset as u64));

    // String data
    calldata.extend(string_encoded);

    // Bytes data
    calldata.extend(encode_uint256(metadata_value.len() as u64));
    calldata.extend_from_slice(metadata_value);

    // Pad to 32-byte boundary
    let padding = (32 - (metadata_value.len() % 32)) % 32;
    calldata.extend(vec![0u8; padding]);

    calldata
}

/// Encode balanceOf(address owner) call
pub fn encode_balance_of(owner: &str) -> Vec<u8> {
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&BALANCE_OF_SELECTOR);
    calldata.extend(encode_address(owner));
    calldata
}

/// Encode tokenOfOwnerByIndex(address owner, uint256 index) call
pub fn encode_token_of_owner_by_index(owner: &str, index: u64) -> Vec<u8> {
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&TOKEN_OF_OWNER_BY_INDEX_SELECTOR);
    calldata.extend(encode_address(owner));
    calldata.extend(encode_uint256(index));
    calldata
}

/// Decode tokenURI result
pub fn decode_token_uri_result(data: &[u8]) -> Result<String, String> {
    if data.len() < 64 {
        return Err("Response too short".to_string());
    }

    // First 32 bytes is offset to string
    let offset = decode_uint256(&data[..32]) as usize;

    decode_string(data, offset).ok_or_else(|| "Failed to decode string".to_string())
}

/// Decode address result (ownerOf, getAgentWallet)
pub fn decode_address_result(data: &[u8]) -> Result<String, String> {
    if data.len() < 32 {
        return Err("Response too short".to_string());
    }
    Ok(decode_address(&data[..32]))
}

/// Decode uint256 result (totalSupply)
pub fn decode_uint256_result(data: &[u8]) -> Result<u64, String> {
    if data.len() < 32 {
        return Err("Response too short".to_string());
    }
    Ok(decode_uint256(&data[..32]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_register() {
        let calldata = encode_register("ipfs://QmTest");
        assert!(calldata.starts_with(&REGISTER_SELECTOR));
        assert!(calldata.len() >= 68); // 4 + 32 + 32 minimum
    }

    #[test]
    fn test_encode_token_uri() {
        let calldata = encode_token_uri(42);
        assert!(calldata.starts_with(&TOKEN_URI_SELECTOR));
        assert_eq!(calldata.len(), 36); // 4 + 32
    }

    #[test]
    fn test_encode_owner_of() {
        let calldata = encode_owner_of(1);
        assert!(calldata.starts_with(&OWNER_OF_SELECTOR));
        assert_eq!(calldata.len(), 36);
    }

    #[test]
    fn test_encode_balance_of() {
        let calldata = encode_balance_of("0x1234567890AbCdEf1234567890aBcDeF12345678");
        assert!(calldata.starts_with(&BALANCE_OF_SELECTOR));
        assert_eq!(calldata.len(), 36); // 4 + 32
    }

    #[test]
    fn test_encode_token_of_owner_by_index() {
        let calldata = encode_token_of_owner_by_index("0x1234567890AbCdEf1234567890aBcDeF12345678", 0);
        assert!(calldata.starts_with(&TOKEN_OF_OWNER_BY_INDEX_SELECTOR));
        assert_eq!(calldata.len(), 68); // 4 + 32 + 32
    }
}
