//! ERC20 ABI encoding/decoding helpers
//!
//! Manual ABI encoding for ERC20 calls without the abigen! macro.

use ethers::abi::{Token, AbiDecode};
use ethers::types::{Address, U256};

/// Function selector for balanceOf(address)
const BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

/// Function selector for decimals()
const DECIMALS_SELECTOR: [u8; 4] = [0x31, 0x3c, 0xe5, 0x67];

/// Function selector for symbol()
const SYMBOL_SELECTOR: [u8; 4] = [0x95, 0xd8, 0x9b, 0x41];

/// Function selector for nonces(address) - EIP-2612
const NONCES_SELECTOR: [u8; 4] = [0x7e, 0xce, 0xbe, 0x00];

/// Encode a balanceOf(address) call
pub fn encode_balance_of(address: Address) -> Vec<u8> {
    let mut data = BALANCE_OF_SELECTOR.to_vec();
    data.extend_from_slice(&ethers::abi::encode(&[Token::Address(address)]));
    data
}

/// Decode a balance response (uint256)
pub fn decode_balance(data: &[u8]) -> Result<U256, String> {
    if data.len() < 32 {
        return Err(format!("Balance response too short: {} bytes", data.len()));
    }
    U256::decode(data)
        .map_err(|e| format!("Failed to decode balance: {}", e))
}

/// Encode a decimals() call
pub fn encode_decimals() -> Vec<u8> {
    DECIMALS_SELECTOR.to_vec()
}

/// Decode a decimals response (uint8)
pub fn decode_decimals(data: &[u8]) -> Result<u8, String> {
    if data.len() < 32 {
        return Err(format!("Decimals response too short: {} bytes", data.len()));
    }
    // Decimals is returned as uint8 in a 32-byte word
    let value = U256::decode(data)
        .map_err(|e| format!("Failed to decode decimals: {}", e))?;

    // Should fit in u8
    if value > U256::from(255u8) {
        return Err("Decimals value too large".to_string());
    }
    Ok(value.as_u32() as u8)
}

/// Encode a symbol() call
pub fn encode_symbol() -> Vec<u8> {
    SYMBOL_SELECTOR.to_vec()
}

/// Decode a symbol response (string)
pub fn decode_symbol(data: &[u8]) -> Result<String, String> {
    if data.len() < 64 {
        return Err(format!("Symbol response too short: {} bytes", data.len()));
    }

    // ABI-encoded string: offset (32 bytes) + length (32 bytes) + data
    String::decode(data)
        .map_err(|e| format!("Failed to decode symbol: {}", e))
}

/// Encode a nonces(address) call - EIP-2612 permit nonce
pub fn encode_nonces(address: Address) -> Vec<u8> {
    let mut data = NONCES_SELECTOR.to_vec();
    data.extend_from_slice(&ethers::abi::encode(&[Token::Address(address)]));
    data
}

/// Decode a nonces response (uint256)
pub fn decode_nonces(data: &[u8]) -> Result<U256, String> {
    if data.len() < 32 {
        return Err(format!("Nonces response too short: {} bytes", data.len()));
    }
    U256::decode(data)
        .map_err(|e| format!("Failed to decode nonces: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Address;
    use std::str::FromStr;

    #[test]
    fn test_encode_balance_of() {
        let address = Address::from_str("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913").unwrap();
        let encoded = encode_balance_of(address);

        // Should be 4 bytes selector + 32 bytes address
        assert_eq!(encoded.len(), 36);
        assert_eq!(&encoded[0..4], &BALANCE_OF_SELECTOR);
    }

    #[test]
    fn test_selectors() {
        // Verify selectors are correct by computing from function signatures
        assert_eq!(
            BALANCE_OF_SELECTOR,
            keccak256(b"balanceOf(address)")[0..4]
        );
        assert_eq!(
            DECIMALS_SELECTOR,
            keccak256(b"decimals()")[0..4]
        );
        assert_eq!(
            SYMBOL_SELECTOR,
            keccak256(b"symbol()")[0..4]
        );
    }

    #[test]
    fn test_decode_balance() {
        // Encode a known balance
        let balance = U256::from(1_000_000u64); // 1 USDC
        let encoded = balance.encode();

        let decoded = decode_balance(&encoded).unwrap();
        assert_eq!(decoded, balance);
    }

    #[test]
    fn test_decode_decimals() {
        // Encode decimals = 6 (USDC)
        let decimals = U256::from(6u8);
        let encoded = decimals.encode();

        let decoded = decode_decimals(&encoded).unwrap();
        assert_eq!(decoded, 6);
    }
}
