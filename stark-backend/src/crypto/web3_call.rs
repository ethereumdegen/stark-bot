//! Web3 contract function calls — read-only or write (queued).

use crate::tx_queue::TxQueueManager;
use crate::wallet::WalletProvider;
use crate::web3::execute_standalone_call;
use serde_json::Value;
use std::sync::Arc;

pub async fn web3_call(
    abi: &str,
    contract: &str,
    function: &str,
    params: &[Value],
    value: &str,
    call_only: bool,
    network: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
    tx_queue: &Arc<TxQueueManager>,
) -> Result<Value, String> {
    execute_standalone_call(
        abi, contract, function, params, value,
        call_only, network, wallet_provider, tx_queue,
    ).await
}
