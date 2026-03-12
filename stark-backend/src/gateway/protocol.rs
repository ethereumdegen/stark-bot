//! Gateway protocol types for WebSocket communication.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A gateway event broadcast to all connected WebSocket clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayEvent {
    pub event: String,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl GatewayEvent {
    pub fn new(event: impl Into<String>, data: Value) -> Self {
        Self {
            event: event.into(),
            data,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    pub fn custom(event: impl Into<String>, data: Value) -> Self {
        Self::new(event, data)
    }

    pub fn tx_pending(_task_id: u64, tx_hash: &str, network: &str, explorer_url: &str) -> Self {
        Self::new("tx.pending", serde_json::json!({
            "tx_hash": tx_hash,
            "network": network,
            "explorer_url": explorer_url,
        }))
    }

    pub fn tx_confirmed(_task_id: u64, tx_hash: &str, network: &str, status: &str) -> Self {
        Self::new("tx.confirmed", serde_json::json!({
            "tx_hash": tx_hash,
            "network": network,
            "status": status,
        }))
    }
}

/// JSON-RPC request from a WebSocket client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC response sent back to a WebSocket client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn success(id: String, result: Value) -> Self {
        Self { id, result: Some(result), error: None }
    }

    pub fn error(id: String, error: RpcError) -> Self {
        Self { id, result: None, error: Some(error) }
    }
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i32, message: String) -> Self {
        Self { code, message }
    }

    pub fn parse_error() -> Self {
        Self { code: -32700, message: "Parse error".to_string() }
    }

    pub fn invalid_params(message: String) -> Self {
        Self { code: -32602, message }
    }

    pub fn method_not_found() -> Self {
        Self { code: -32601, message: "Method not found".to_string() }
    }

    pub fn internal_error(message: String) -> Self {
        Self { code: -32603, message }
    }
}
