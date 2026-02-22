//! Module/plugin system for StarkBot
//!
//! Modules are standalone microservices that run as separate binaries.
//! Each module has its own database, HTTP server, and dashboard.
//! The main bot communicates with modules via JSON RPC over HTTP.

pub mod dynamic_module;
pub mod dynamic_tool;
pub mod loader;
pub mod manifest;
pub mod registry;
pub mod zip_parser;

use async_trait::async_trait;
use crate::db::Database;
use crate::tools::registry::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub use registry::ModuleRegistry;

/// Info about an external endpoint exposed by a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtEndpointInfo {
    /// Method name (used in URL path)
    pub method_name: String,
    /// Description of the endpoint
    pub description: Option<String>,
    /// The RPC endpoint path on the module service
    pub rpc_endpoint: String,
    /// Allowed HTTP methods
    pub http_methods: Vec<String>,
}

/// Raw HTTP response from proxying to a module (preserves status, headers, body).
pub struct RawProxyResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Trait that all modules must implement.
///
/// Modules are standalone services — they manage their own database, workers,
/// and dashboard. This trait defines the interface the main bot uses to
/// interact with them: registering tools, fetching dashboard data, and
/// performing backup/restore via RPC.
#[async_trait]
pub trait Module: Send + Sync {
    /// Unique module name (used as identifier)
    fn name(&self) -> &str;
    /// Human-readable description
    fn description(&self) -> &str;
    /// Semantic version
    fn version(&self) -> &str;
    /// Default port the service listens on
    fn default_port(&self) -> u16;

    /// The base URL of the running service (reads from env or falls back to default)
    fn service_url(&self) -> String;

    /// Whether this module provides tools to the bot
    fn has_tools(&self) -> bool;
    /// Whether this module has a standalone dashboard (served by the service itself)
    fn has_dashboard(&self) -> bool;

    /// Return tool instances to register with the bot
    fn create_tools(&self) -> Vec<Arc<dyn Tool>>;

    /// Shell command from manifest to start the service (if any)
    fn manifest_command(&self) -> Option<String> {
        None
    }

    /// Directory containing the module on disk (if available)
    fn module_dir(&self) -> Option<&PathBuf> {
        None
    }

    /// Optional: skill markdown content to install
    fn skill_content(&self) -> Option<&str> {
        None
    }

    /// Whether this module provides a skill
    fn has_skill(&self) -> bool {
        self.skill_content().is_some()
    }

    /// Return dashboard data as JSON (fetched from the service via RPC)
    async fn dashboard_data(&self, _db: &Database) -> Option<Value> {
        None
    }

    /// Return data to include in cloud backup (fetched from service)
    async fn backup_data(&self, _db: &Database) -> Option<Value> {
        None
    }

    /// Restore module data from a cloud backup (sent to service)
    async fn restore_data(&self, _db: &Database, _data: &Value) -> Result<(), String> {
        Ok(())
    }

    /// Whether this module declares any external HTTP endpoints
    fn has_ext_endpoints(&self) -> bool {
        false
    }

    /// Find an external endpoint by method name
    fn find_ext_endpoint(&self, _method: &str) -> Option<ExtEndpointInfo> {
        None
    }

    /// List all external endpoints declared by this module
    fn ext_endpoint_list(&self) -> Vec<ExtEndpointInfo> {
        Vec::new()
    }

    /// Proxy an HTTP request to a module's external endpoint, preserving raw status/headers/body
    async fn proxy_ext_request(
        &self,
        _rpc_endpoint: &str,
        _http_method: &str,
        _body: Vec<u8>,
        _headers: HashMap<String, String>,
    ) -> Result<RawProxyResponse, String> {
        Err("Module does not support ext endpoints".to_string())
    }
}
