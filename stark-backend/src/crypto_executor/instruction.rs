//! CryptoInstruction — tagged enum representing all crypto operations
//! that Starkbot can execute locally.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "instruction")]
pub enum CryptoInstruction {
    SendEth {
        network: Option<String>,
        to: String,
        amount_raw: String,
    },
    SwapToken {
        sell_token: String,
        buy_token: String,
        amount: String,
        #[serde(default = "default_base")]
        network: String,
    },
    BridgeUsdc {
        amount: String,
        from_network: String,
        to_network: String,
        #[serde(default)]
        recipient: Option<String>,
    },
    Web3Call {
        abi: String,
        contract: String,
        function: String,
        #[serde(default)]
        params: Vec<Value>,
        #[serde(default = "default_zero")]
        value: String,
        network: Option<String>,
        #[serde(default)]
        call_only: bool,
    },
    Web3PresetCall {
        preset: String,
        network: Option<String>,
    },
    BroadcastTx {
        uuid: String,
    },
    SignMessage {
        message: String,
    },
    X402Fetch {
        preset: String,
        #[serde(default = "default_base")]
        network: String,
    },
    X402Rpc {
        preset: String,
        #[serde(default = "default_base")]
        network: String,
    },
    X402Post {
        url: String,
        #[serde(default)]
        body: Value,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default = "default_base")]
        network: String,
    },
    GetBalance {
        #[serde(default = "default_base")]
        network: String,
    },
    GetAddress,
    ListQueuedTx,
    TokenLookup {
        symbol: String,
        #[serde(default = "default_base")]
        network: String,
    },
    Erc8128Fetch {
        url: String,
        #[serde(default = "default_get")]
        method: String,
        #[serde(default)]
        body: Option<Value>,
    },
    SiwaAuth {
        server_url: String,
        domain: String,
        uri: String,
        #[serde(default)]
        agent_id: Option<String>,
    },
    DecodeCalldata {
        data: String,
        abi: String,
    },
    SignRawTx {
        to: String,
        data: String,
        #[serde(default = "default_zero")]
        value: String,
        #[serde(default = "default_chain_id")]
        chain_id: u64,
        #[serde(default)]
        gas: Option<String>,
        #[serde(default)]
        nonce: Option<u64>,
    },
}

fn default_base() -> String {
    "base".to_string()
}

fn default_zero() -> String {
    "0".to_string()
}

fn default_get() -> String {
    "GET".to_string()
}

fn default_chain_id() -> u64 {
    8453
}
