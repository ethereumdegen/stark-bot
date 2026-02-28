//! DynamicModule — a Module implementation loaded from a `module.toml` manifest
//! at runtime. No compiled module-specific code needed.

use async_trait::async_trait;
use crate::db::Database;
use crate::tools::registry::Tool;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::dynamic_tool::DynamicModuleTool;
use super::manifest::ModuleManifest;
use super::{ExtEndpointInfo, RawProxyResponse};

/// A module loaded dynamically from a manifest file on disk.
pub struct DynamicModule {
    manifest: ModuleManifest,
    /// Directory containing the module (e.g. ~/.starkbot/modules/wallet_monitor/)
    module_dir: PathBuf,
    /// Cached skill content (loaded once from disk)
    skill_content: Option<String>,
    /// Resolved path to skill directory (if `skill_dir` is configured)
    skill_dir_path: Option<PathBuf>,
    /// Resolved path to agent directory (if `agent` is configured)
    agent_dir_path: Option<PathBuf>,
}

impl DynamicModule {
    /// Create a DynamicModule from a parsed manifest and its containing directory.
    pub fn new(manifest: ModuleManifest, module_dir: PathBuf) -> Self {
        // Pre-load skill content if configured (legacy content_file path)
        let skill_content = manifest.skill.as_ref().and_then(|skill_cfg| {
            skill_cfg.content_file.as_ref()
        }).and_then(|content_file| {
            let skill_path = module_dir.join(content_file);
            std::fs::read_to_string(&skill_path)
                .map_err(|e| {
                    log::warn!(
                        "[MODULE] Failed to read skill file {}: {}",
                        skill_path.display(),
                        e
                    );
                    e
                })
                .ok()
        });

        // Resolve skill_dir path if configured
        let skill_dir_path = manifest.skill.as_ref()
            .and_then(|skill_cfg| skill_cfg.skill_dir.as_ref())
            .map(|dir| module_dir.join(dir))
            .filter(|p| p.is_dir());

        // Resolve agent_dir path if configured
        let agent_dir_path = manifest.agent.as_ref()
            .map(|agent_cfg| module_dir.join(&agent_cfg.dir))
            .filter(|p| p.join("agent.md").exists());

        DynamicModule {
            manifest,
            module_dir,
            skill_content,
            skill_dir_path,
            agent_dir_path,
        }
    }

    /// Path to the service binary for this module.
    pub fn binary_path(&self) -> PathBuf {
        self.module_dir
            .join("bin")
            .join(format!("{}-service", self.manifest.module.name))
    }

    /// The directory this module was loaded from.
    pub fn module_dir(&self) -> &PathBuf {
        &self.module_dir
    }

    /// The manifest path.
    pub fn manifest_path(&self) -> PathBuf {
        self.module_dir.join("module.toml")
    }

    /// Author from manifest (if present).
    pub fn author(&self) -> Option<&str> {
        self.manifest.module.author.as_deref()
    }

    /// Port env var from manifest (if present).
    pub fn manifest_port_env_var(&self) -> Option<String> {
        self.manifest.service.port_env_var.clone()
    }

    /// URL env var from manifest (if present).
    pub fn manifest_url_env_var(&self) -> Option<String> {
        self.manifest.service.url_env_var.clone()
    }

    /// Shell command from manifest (if present).
    pub fn manifest_command(&self) -> Option<String> {
        self.manifest.service.command.clone()
    }

    /// Generic HTTP POST to a module endpoint, returning the `data` field from `{success, data, error}`.
    async fn rpc_post(&self, endpoint: &str, body: &Value) -> Result<Value, String> {
        let url = format!("{}{}", self.manifest.service_url(), endpoint);
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("{} service unavailable: {}", self.manifest.module.name, e))?;
        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid response from {}: {}", self.manifest.module.name, e))?;
        if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            Ok(json.get("data").cloned().unwrap_or(Value::Null))
        } else {
            let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            Err(err.to_string())
        }
    }

    /// Generic HTTP POST with empty body.
    async fn rpc_post_empty(&self, endpoint: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.manifest.service_url(), endpoint);
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .send()
            .await
            .map_err(|e| format!("{} service unavailable: {}", self.manifest.module.name, e))?;
        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid response from {}: {}", self.manifest.module.name, e))?;
        if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            Ok(json.get("data").cloned().unwrap_or(Value::Null))
        } else {
            let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            Err(err.to_string())
        }
    }

    /// Raw HTTP proxy to a module endpoint — preserves status code, headers, and body.
    /// Unlike `rpc_post` which unwraps the `{success, data, error}` envelope, this
    /// passes through the response verbatim for ext endpoint proxying.
    async fn proxy_ext_raw(
        &self,
        rpc_endpoint: &str,
        http_method: &str,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    ) -> Result<RawProxyResponse, String> {
        let url = format!("{}{}", self.manifest.service_url(), rpc_endpoint);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let method = reqwest::Method::from_bytes(http_method.as_bytes())
            .map_err(|_| format!("Invalid HTTP method: {}", http_method))?;

        let mut req = client.request(method, &url);

        // Forward headers
        for (key, value) in &headers {
            if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) {
                    req = req.header(header_name, header_value);
                }
            }
        }

        // Attach body for methods that support it
        if !body.is_empty() {
            req = req.body(body);
        }

        let resp = req.send().await.map_err(|e| {
            format!("{} service unavailable: {}", self.manifest.module.name, e)
        })?;

        let status = resp.status().as_u16();
        let mut resp_headers = HashMap::new();
        for (key, value) in resp.headers() {
            if let Ok(v) = value.to_str() {
                resp_headers.insert(key.as_str().to_string(), v.to_string());
            }
        }
        let resp_body = resp.bytes().await.map_err(|e| {
            format!("Failed to read response body from {}: {}", self.manifest.module.name, e)
        })?;

        Ok(RawProxyResponse {
            status,
            headers: resp_headers,
            body: resp_body.to_vec(),
        })
    }
}

#[async_trait]
impl super::Module for DynamicModule {
    fn name(&self) -> &str {
        &self.manifest.module.name
    }

    fn description(&self) -> &str {
        &self.manifest.module.description
    }

    fn version(&self) -> &str {
        &self.manifest.module.version
    }

    fn default_port(&self) -> u16 {
        self.manifest.service.default_port
    }

    fn service_url(&self) -> String {
        self.manifest.service_url()
    }

    fn has_tools(&self) -> bool {
        !self.manifest.tools.is_empty()
    }

    fn has_dashboard(&self) -> bool {
        self.manifest.service.has_dashboard
            || self.manifest.service.dashboard_style.is_some()
            || self.manifest.service.dashboard_styles.is_some()
    }

    fn dashboard_style(&self) -> Option<String> {
        // Return the first style from the resolved list
        let styles = self.manifest.service.resolved_dashboard_styles();
        if styles.is_empty() {
            None
        } else {
            Some(styles[0].clone())
        }
    }

    fn dashboard_styles(&self) -> Vec<String> {
        self.manifest.service.resolved_dashboard_styles()
    }

    fn create_tools(&self) -> Vec<Arc<dyn Tool>> {
        let base_url = self.service_url();
        self.manifest
            .tools
            .iter()
            .map(|tool_manifest| {
                Arc::new(DynamicModuleTool::from_manifest(tool_manifest, &base_url)) as Arc<dyn Tool>
            })
            .collect()
    }

    fn manifest_command(&self) -> Option<String> {
        self.manifest.service.command.clone()
    }

    fn manifest_port_env_var(&self) -> Option<String> {
        self.manifest.service.port_env_var.clone()
    }

    fn manifest_env_var_keys(&self) -> Vec<String> {
        self.manifest.service.env_vars.keys().cloned().collect()
    }

    fn module_dir(&self) -> Option<&PathBuf> {
        Some(&self.module_dir)
    }

    fn skill_content(&self) -> Option<&str> {
        self.skill_content.as_deref()
    }

    fn skill_dir(&self) -> Option<&PathBuf> {
        self.skill_dir_path.as_ref()
    }

    fn agent_dir(&self) -> Option<&PathBuf> {
        self.agent_dir_path.as_ref()
    }

    async fn dashboard_data(&self, _db: &Database) -> Option<Value> {
        let endpoint = self.manifest.service.dashboard_endpoint.as_deref()?;
        let url = format!("{}{}", self.manifest.service_url(), endpoint);
        let client = reqwest::Client::new();
        let resp = client.get(&url).send().await.ok()?;
        resp.json::<Value>().await.ok()
    }

    async fn backup_data(&self, _db: &Database) -> Option<Value> {
        let endpoint = self.manifest.service.backup_endpoint.as_deref()?;
        match self.rpc_post_empty(endpoint).await {
            Ok(data) => {
                // Skip empty arrays / null
                if data.is_null() {
                    return None;
                }
                if let Some(arr) = data.as_array() {
                    if arr.is_empty() {
                        return None;
                    }
                }
                Some(data)
            }
            Err(e) => {
                log::warn!("[MODULE] backup_data for '{}' failed: {}", self.manifest.module.name, e);
                None
            }
        }
    }

    async fn restore_data(&self, _db: &Database, data: &Value) -> Result<(), String> {
        let endpoint = match self.manifest.service.restore_endpoint.as_deref() {
            Some(ep) => ep,
            None => return Ok(()), // no restore endpoint configured
        };
        let _ = self.rpc_post(endpoint, data).await?;
        log::info!(
            "[MODULE] Restored data for '{}' via {}",
            self.manifest.module.name,
            endpoint
        );
        Ok(())
    }

    fn has_ext_endpoints(&self) -> bool {
        !self.manifest.ext_endpoints.is_empty()
    }

    fn find_ext_endpoint(&self, method: &str) -> Option<ExtEndpointInfo> {
        self.manifest.find_ext_endpoint(method).map(|ep| ExtEndpointInfo {
            method_name: ep.method_name.clone(),
            description: ep.description.clone(),
            rpc_endpoint: ep.rpc_endpoint.clone(),
            http_methods: ep.http_methods.clone(),
            x402: ep.x402,
            x402_price: ep.x402_price.clone(),
            x402_currency: ep.x402_currency.clone(),
            x402_payee: ep.x402_payee.clone(),
            x402_network: ep.x402_network.clone(),
        })
    }

    fn ext_endpoint_list(&self) -> Vec<ExtEndpointInfo> {
        self.manifest
            .ext_endpoints
            .iter()
            .map(|ep| ExtEndpointInfo {
                method_name: ep.method_name.clone(),
                description: ep.description.clone(),
                rpc_endpoint: ep.rpc_endpoint.clone(),
                http_methods: ep.http_methods.clone(),
                x402: ep.x402,
                x402_price: ep.x402_price.clone(),
                x402_currency: ep.x402_currency.clone(),
                x402_payee: ep.x402_payee.clone(),
                x402_network: ep.x402_network.clone(),
            })
            .collect()
    }

    async fn proxy_ext_request(
        &self,
        rpc_endpoint: &str,
        http_method: &str,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    ) -> Result<RawProxyResponse, String> {
        self.proxy_ext_raw(rpc_endpoint, http_method, body, headers).await
    }
}
