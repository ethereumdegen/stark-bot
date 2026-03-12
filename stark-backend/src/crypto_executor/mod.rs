//! CryptoExecutor — dispatches CryptoInstruction to standalone crypto functions.

pub mod instruction;

pub use instruction::CryptoInstruction;

use crate::crypto;
use crate::gateway::events::EventBroadcaster;
use crate::tx_queue::TxQueueManager;
use crate::wallet::WalletProvider;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct CryptoExecutor {
    pub wallet_provider: Arc<dyn WalletProvider>,
    pub tx_queue: Arc<TxQueueManager>,
    pub broadcaster: Arc<EventBroadcaster>,
    pub credits_session: Option<Arc<crate::credits_session::CreditsSessionClient>>,
    pub db: Option<Arc<crate::db::Database>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub data: Value,
}

impl CryptoExecutor {
    pub async fn execute(&self, instruction: CryptoInstruction) -> Result<ExecutionResult, String> {
        match instruction {
            CryptoInstruction::GetAddress => {
                let address = self.wallet_provider.get_address();
                Ok(ExecutionResult {
                    success: true,
                    data: json!({ "address": address }),
                })
            }

            CryptoInstruction::GetBalance { network } => {
                let address = self.wallet_provider.get_address();
                let balance = crypto::helpers::get_balance(&address, &network, &self.wallet_provider).await?;
                Ok(ExecutionResult {
                    success: true,
                    data: json!({ "address": address, "balance_wei": balance, "network": network }),
                })
            }

            CryptoInstruction::SendEth { network, to, amount_raw } => {
                let network_str = network.as_deref().unwrap_or("base");
                let result = crypto::send_eth::send_eth(
                    network_str, &to, &amount_raw, &self.wallet_provider, &self.tx_queue,
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::BroadcastTx { uuid } => {
                let result = crypto::broadcast_tx::broadcast_tx(
                    &uuid, &self.tx_queue, &self.wallet_provider, &self.broadcaster, self.db.as_ref(),
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::ListQueuedTx => {
                let txs = self.tx_queue.list_pending();
                Ok(ExecutionResult {
                    success: true,
                    data: json!({ "transactions": txs }),
                })
            }

            CryptoInstruction::TokenLookup { symbol, network } => {
                let result = crypto::token_utils::lookup_token(&symbol, &network)?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::SwapToken { sell_token, buy_token, amount, network } => {
                let result = crypto::swap_token::swap_token(
                    &sell_token, &buy_token, &amount, &network,
                    &self.wallet_provider, &self.tx_queue, self.credits_session.as_ref(),
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::BridgeUsdc { amount, from_network, to_network, recipient } => {
                let result = crypto::bridge_usdc::bridge_usdc(
                    &amount, &from_network, &to_network, recipient.as_deref(),
                    &self.wallet_provider, &self.tx_queue,
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::Web3Call { abi, contract, function, params, value, network, call_only } => {
                let network_str = network.as_deref().unwrap_or("base");
                let result = crypto::web3_call::web3_call(
                    &abi, &contract, &function, &params, &value, call_only,
                    network_str, &self.wallet_provider, &self.tx_queue,
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::SignMessage { message } => {
                let result = crypto::sign::sign_message(&message, &self.wallet_provider).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::SignRawTx { to, data, value, chain_id, gas, nonce } => {
                let result = crypto::sign::sign_raw_tx(
                    &to, &data, &value, chain_id, gas.as_deref(), nonce,
                    &self.wallet_provider,
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::X402Post { url, body, headers, network } => {
                let result = crypto::x402_ops::x402_post(
                    &url, &body, &headers, &network, &self.wallet_provider,
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::DecodeCalldata { data, abi } => {
                let result = crypto::helpers::decode_calldata(&data, &abi)?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::Erc8128Fetch { url, method, body } => {
                let result = crypto::auth::erc8128_fetch(
                    &url, &method, body.as_ref(), &self.wallet_provider,
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::SiwaAuth { server_url, domain, uri, agent_id } => {
                let result = crypto::auth::siwa_auth(
                    &server_url, &domain, &uri, agent_id.as_deref(),
                    &self.wallet_provider, self.db.as_ref(),
                ).await?;
                Ok(ExecutionResult { success: true, data: result })
            }

            CryptoInstruction::X402Fetch { preset, network } => {
                // Placeholder — preset fetch requires preset registry
                Ok(ExecutionResult {
                    success: false,
                    data: json!({ "error": format!("X402Fetch preset '{}' on '{}' not yet implemented in executor", preset, network) }),
                })
            }

            CryptoInstruction::X402Rpc { preset, network } => {
                Ok(ExecutionResult {
                    success: false,
                    data: json!({ "error": format!("X402Rpc preset '{}' on '{}' not yet implemented in executor", preset, network) }),
                })
            }

            CryptoInstruction::Web3PresetCall { preset, network } => {
                Ok(ExecutionResult {
                    success: false,
                    data: json!({ "error": format!("Web3PresetCall '{}' on '{:?}' not yet implemented in executor", preset, network) }),
                })
            }
        }
    }
}
