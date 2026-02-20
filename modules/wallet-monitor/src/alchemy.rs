//! Alchemy Enhanced API client for wallet monitoring.
//!
//! Supports Ethereum Mainnet and Base chains via `alchemy_getAssetTransfers`.

use serde::{Deserialize, Serialize};

pub fn alchemy_base_url(chain: &str, api_key: &str) -> String {
    match chain {
        "base" => format!("https://base-mainnet.g.alchemy.com/v2/{}", api_key),
        _ => format!("https://eth-mainnet.g.alchemy.com/v2/{}", api_key),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetTransfer {
    #[serde(rename = "blockNum")]
    pub block_num: String,
    pub hash: String,
    pub from: String,
    pub to: Option<String>,
    pub value: Option<f64>,
    pub asset: Option<String>,
    pub category: String,
    #[serde(rename = "rawContract")]
    pub raw_contract: Option<RawContract>,
    pub metadata: Option<TransferMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawContract {
    pub value: Option<String>,
    pub address: Option<String>,
    pub decimal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferMetadata {
    #[serde(rename = "blockTimestamp")]
    pub block_timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlchemyResponse {
    result: Option<AssetTransferResult>,
    error: Option<AlchemyError>,
}

#[derive(Debug, Deserialize)]
struct AssetTransferResult {
    transfers: Vec<AssetTransfer>,
    #[serde(rename = "pageKey")]
    page_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlchemyError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct BlockNumberResponse {
    result: Option<String>,
    error: Option<AlchemyError>,
}

pub async fn get_asset_transfers(
    client: &reqwest::Client,
    chain: &str,
    api_key: &str,
    address: &str,
    from_block: Option<i64>,
    direction: &str,
) -> Result<Vec<AssetTransfer>, String> {
    let url = alchemy_base_url(chain, api_key);
    let from_block_hex = from_block
        .map(|b| format!("0x{:x}", b))
        .unwrap_or_else(|| "0x0".to_string());

    // "internal" category is only supported on ETH and MATIC, not Base/L2s
    let categories = match chain {
        "base" => serde_json::json!(["external", "erc20"]),
        _ => serde_json::json!(["external", "internal", "erc20"]),
    };

    let mut params = serde_json::json!({
        "fromBlock": from_block_hex,
        "toBlock": "latest",
        "category": categories,
        "withMetadata": true,
        "maxCount": "0x3e8",
    });

    if direction == "from" {
        params["fromAddress"] = serde_json::json!(address);
    } else {
        params["toAddress"] = serde_json::json!(address);
    }

    let mut all_transfers = Vec::new();
    let mut page_key: Option<String> = None;

    loop {
        let mut request_params = params.clone();
        if let Some(ref pk) = page_key {
            request_params["pageKey"] = serde_json::json!(pk);
        }

        let body = serde_json::json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "alchemy_getAssetTransfers",
            "params": [request_params]
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Alchemy request failed: {}", e))?;

        let response: AlchemyResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Alchemy response: {}", e))?;

        if let Some(err) = response.error {
            return Err(format!("Alchemy API error: {}", err.message));
        }

        if let Some(result) = response.result {
            all_transfers.extend(result.transfers);
            if let Some(pk) = result.page_key {
                page_key = Some(pk);
            } else {
                break;
            }
        } else {
            break;
        }

        if all_transfers.len() > 5000 {
            break;
        }
    }

    Ok(all_transfers)
}

pub async fn get_block_number(
    client: &reqwest::Client,
    chain: &str,
    api_key: &str,
) -> Result<i64, String> {
    let url = alchemy_base_url(chain, api_key);
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": []
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("eth_blockNumber request failed: {}", e))?;

    let response: BlockNumberResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse block number response: {}", e))?;

    if let Some(err) = response.error {
        return Err(format!("eth_blockNumber error: {}", err.message));
    }

    if let Some(hex) = response.result {
        let hex = hex.trim_start_matches("0x");
        i64::from_str_radix(hex, 16).map_err(|e| format!("Failed to parse block number: {}", e))
    } else {
        Err("No block number in response".to_string())
    }
}

pub fn parse_block_number(hex: &str) -> i64 {
    let hex = hex.trim_start_matches("0x");
    i64::from_str_radix(hex, 16).unwrap_or(0)
}
