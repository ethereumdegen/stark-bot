//! Module manifest parser — reads `module.toml` into typed structs.
//!
//! The manifest is the single source of truth for dynamically loaded modules.
//! It describes the module metadata, service configuration, tool definitions,
//! and skill content — everything starkbot needs to load a module without
//! compiling module-specific code.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level module manifest (deserialized from `module.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct ModuleManifest {
    pub module: ModuleInfo,
    pub service: ServiceConfig,
    #[serde(default)]
    pub skill: Option<SkillConfig>,
    #[serde(default)]
    pub agent: Option<AgentConfig>,
    #[serde(default)]
    pub platforms: Option<PlatformConfig>,
    #[serde(default)]
    pub tools: Vec<ToolManifest>,
    /// External HTTP endpoints exposed publicly via `/ext/{module}/{method}`.
    #[serde(default)]
    pub ext_endpoints: Vec<ExtEndpointManifest>,
}

/// Basic module metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct ModuleInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
}

/// Service configuration — how to reach and launch the microservice.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    /// Shell command to start the service (e.g. "uv run service.py").
    /// When set, takes priority over binary discovery. Run from the module directory.
    #[serde(default)]
    pub command: Option<String>,
    pub default_port: u16,
    /// Environment variable that overrides the port (e.g. "WALLET_MONITOR_PORT")
    #[serde(default)]
    pub port_env_var: Option<String>,
    /// Environment variable that overrides the full URL (e.g. "WALLET_MONITOR_URL")
    #[serde(default)]
    pub url_env_var: Option<String>,
    #[serde(default)]
    pub has_dashboard: bool,
    /// Dashboard style: "html" for traditional HTML dashboards, "tui" for Rich ANSI dashboards.
    #[serde(default)]
    pub dashboard_style: Option<String>,
    /// Multiple dashboard styles (e.g. ["tui", "html"] for dual-mode modules).
    /// Takes priority over `dashboard_style` when present.
    #[serde(default)]
    pub dashboard_styles: Option<Vec<String>>,
    #[serde(default = "default_health_endpoint")]
    pub health_endpoint: String,
    /// RPC endpoint for backup export (e.g. "/rpc/backup/export"). POST, returns JSON.
    #[serde(default)]
    pub backup_endpoint: Option<String>,
    /// RPC endpoint for backup restore (e.g. "/rpc/backup/restore"). POST with JSON body.
    #[serde(default)]
    pub restore_endpoint: Option<String>,
    /// Path for dashboard data (e.g. "/"). GET, returns HTML or JSON.
    #[serde(default)]
    pub dashboard_endpoint: Option<String>,
    /// Extra environment variables the service needs.
    #[serde(default)]
    pub env_vars: HashMap<String, EnvVarSpec>,
}

impl ServiceConfig {
    /// Resolve the canonical list of dashboard styles for this module.
    /// Priority: `dashboard_styles` (plural) > `dashboard_style` (singular) > `has_dashboard` flag.
    pub fn resolved_dashboard_styles(&self) -> Vec<String> {
        if let Some(ref styles) = self.dashboard_styles {
            return styles.clone();
        }
        if let Some(ref style) = self.dashboard_style {
            return vec![style.clone()];
        }
        if self.has_dashboard {
            return vec!["html".to_string()];
        }
        vec![]
    }
}

fn default_health_endpoint() -> String {
    "/rpc/status".to_string()
}

/// Spec for a required/optional environment variable.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvVarSpec {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// Skill content configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillConfig {
    /// Relative path to the skill markdown file (e.g. "skill.md").
    /// Optional when `skill_dir` is used instead.
    #[serde(default)]
    pub content_file: Option<String>,
    /// Relative path to a full skill folder (e.g. "skill").
    /// The folder should contain a `.md` file + optional scripts, ABIs, presets.
    #[serde(default)]
    pub skill_dir: Option<String>,
}

/// Agent configuration — points to an agent folder inside the module.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    /// Relative path to the agent directory (e.g. "agent").
    /// Must contain `agent.md` and optionally `hooks/`.
    pub dir: String,
}

/// Supported platforms list.
#[derive(Debug, Clone, Deserialize)]
pub struct PlatformConfig {
    #[serde(default)]
    pub supported: Vec<String>,
}

/// A tool definition from the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    #[serde(default = "default_tool_group")]
    pub group: String,
    #[serde(default = "default_rpc_method")]
    pub rpc_method: String,
    pub rpc_endpoint: String,
    #[serde(default)]
    pub parameters: HashMap<String, ToolParameterManifest>,
    /// Parameters that are required (if not specified, inferred from individual param `required` fields).
    #[serde(default)]
    pub required_params: Option<Vec<String>>,
}

fn default_tool_group() -> String {
    "web".to_string()
}

fn default_rpc_method() -> String {
    "POST".to_string()
}

/// A single tool parameter from the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolParameterManifest {
    #[serde(rename = "type", default = "default_param_type")]
    pub param_type: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "enum", default)]
    pub enum_values: Option<Vec<String>>,
    #[serde(default)]
    pub default: Option<toml::Value>,
}

fn default_param_type() -> String {
    "string".to_string()
}

/// An external HTTP endpoint declaration from `[[ext_endpoints]]` in the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtEndpointManifest {
    /// Method name used in the URL path: `/ext/{module_name}/{method_name}`
    pub method_name: String,
    /// Human-readable description of this endpoint
    #[serde(default)]
    pub description: Option<String>,
    /// The RPC endpoint on the module service to proxy to (e.g. "/rpc/ext/tell-a-joke")
    pub rpc_endpoint: String,
    /// Allowed HTTP methods (e.g. ["POST"], ["GET", "POST"])
    #[serde(default = "default_http_methods")]
    pub http_methods: Vec<String>,
    /// If true, the backend verifies x402 payments before forwarding to the module.
    #[serde(default)]
    pub x402: bool,
    /// x402 payment price (e.g. "0.01") — used when x402 = true
    #[serde(default)]
    pub x402_price: Option<String>,
    /// x402 payment currency (e.g. "USDC") — used when x402 = true
    #[serde(default)]
    pub x402_currency: Option<String>,
    /// x402 payee address — used when x402 = true
    #[serde(default)]
    pub x402_payee: Option<String>,
    /// x402 network (e.g. "base", "base-sepolia") — defaults to "base"
    #[serde(default)]
    pub x402_network: Option<String>,
}

fn default_http_methods() -> Vec<String> {
    vec!["POST".to_string()]
}

impl ModuleManifest {
    /// Load a manifest from a `module.toml` file path.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        Self::from_str(&content)
    }

    /// Parse a manifest from a TOML string.
    pub fn from_str(content: &str) -> Result<Self, String> {
        toml::from_str(content).map_err(|e| format!("Failed to parse module.toml: {}", e))
    }

    /// Find an ext endpoint declaration by method name.
    pub fn find_ext_endpoint(&self, method: &str) -> Option<&ExtEndpointManifest> {
        self.ext_endpoints.iter().find(|ep| ep.method_name == method)
    }

    /// Build the service URL from environment variables or defaults.
    pub fn service_url(&self) -> String {
        // First check the URL env var
        if let Some(ref url_var) = self.service.url_env_var {
            if let Ok(url) = std::env::var(url_var) {
                return url;
            }
        }
        // Then check the port env var
        let port = if let Some(ref port_var) = self.service.port_env_var {
            std::env::var(port_var)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(self.service.default_port)
        } else {
            self.service.default_port
        };
        format!("http://127.0.0.1:{}", port)
    }
}

impl ToolManifest {
    /// Compute the list of required parameter names.
    /// If `required_params` is explicitly set, use that.
    /// Otherwise, collect parameter names where `required = true`.
    pub fn required_parameters(&self) -> Vec<String> {
        if let Some(ref explicit) = self.required_params {
            return explicit.clone();
        }
        self.parameters
            .iter()
            .filter(|(_, p)| p.required)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Parse the group string into a ToolGroup enum.
    pub fn tool_group(&self) -> crate::tools::types::ToolGroup {
        match self.group.to_lowercase().as_str() {
            "system" => crate::tools::types::ToolGroup::System,
            "web" => crate::tools::types::ToolGroup::Web,
            "filesystem" => crate::tools::types::ToolGroup::Filesystem,
            "finance" => crate::tools::types::ToolGroup::Finance,
            "development" => crate::tools::types::ToolGroup::Development,
            "exec" => crate::tools::types::ToolGroup::Exec,
            "messaging" => crate::tools::types::ToolGroup::Messaging,
            "social" => crate::tools::types::ToolGroup::Social,
            "memory" => crate::tools::types::ToolGroup::Memory,
            _ => crate::tools::types::ToolGroup::Web, // default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let toml = r#"
[module]
name = "test_module"
version = "1.0.0"
description = "A test module"

[service]
default_port = 9200
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert_eq!(manifest.module.name, "test_module");
        assert_eq!(manifest.service.default_port, 9200);
        assert!(manifest.tools.is_empty());
    }

    #[test]
    fn test_parse_manifest_with_command() {
        let toml = r#"
[module]
name = "price_tracker"
version = "0.1.0"
description = "Track token prices"

[service]
command = "uv run service.py"
default_port = 9200
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert_eq!(manifest.module.name, "price_tracker");
        assert_eq!(manifest.service.command.as_deref(), Some("uv run service.py"));
        assert_eq!(manifest.service.default_port, 9200);
    }

    #[test]
    fn test_parse_manifest_with_ext_endpoints() {
        let toml = r#"
[module]
name = "joke_service"
version = "0.1.0"
description = "A joke service with x402 payments"

[service]
command = "uv run service.py"
default_port = 9300

[[ext_endpoints]]
method_name = "tell-a-joke"
description = "Returns a random joke (x402 paid)"
rpc_endpoint = "/rpc/ext/tell-a-joke"
http_methods = ["POST"]

[[ext_endpoints]]
method_name = "list-categories"
description = "List available joke categories"
rpc_endpoint = "/rpc/ext/list-categories"
http_methods = ["GET"]
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert_eq!(manifest.ext_endpoints.len(), 2);
        assert_eq!(manifest.ext_endpoints[0].method_name, "tell-a-joke");
        assert_eq!(manifest.ext_endpoints[0].rpc_endpoint, "/rpc/ext/tell-a-joke");
        assert_eq!(manifest.ext_endpoints[0].http_methods, vec!["POST"]);
        assert_eq!(manifest.ext_endpoints[1].method_name, "list-categories");
        assert_eq!(manifest.ext_endpoints[1].http_methods, vec!["GET"]);

        // find_ext_endpoint helper
        assert!(manifest.find_ext_endpoint("tell-a-joke").is_some());
        assert!(manifest.find_ext_endpoint("nonexistent").is_none());
    }

    #[test]
    fn test_parse_manifest_without_ext_endpoints() {
        let toml = r#"
[module]
name = "basic_module"
version = "1.0.0"
description = "No ext endpoints"

[service]
default_port = 9200
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert!(manifest.ext_endpoints.is_empty());
    }

    #[test]
    fn test_parse_full_manifest() {
        let toml = r#"
[module]
name = "wallet_monitor"
version = "1.1.0"
author = "@ethereumdegen"
description = "Monitor ETH wallets"
license = "MIT"

[service]
default_port = 9100
port_env_var = "WALLET_MONITOR_PORT"
url_env_var = "WALLET_MONITOR_URL"
has_dashboard = true
health_endpoint = "/rpc/status"

[service.env_vars]
ALCHEMY_API_KEY = { required = true, description = "Alchemy API key" }

[skill]
content_file = "skill.md"

[platforms]
supported = ["linux-x86_64", "darwin-aarch64"]

[[tools]]
name = "wallet_watchlist"
description = "Manage the wallet watchlist"
group = "finance"
rpc_method = "POST"
rpc_endpoint = "/rpc/watchlist"

[tools.parameters.action]
type = "string"
description = "Action to perform"
required = true
enum = ["add", "remove", "list"]

[tools.parameters.address]
type = "string"
description = "Ethereum wallet address"
required = false
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert_eq!(manifest.module.name, "wallet_monitor");
        assert_eq!(manifest.module.author.as_deref(), Some("@ethereumdegen"));
        assert_eq!(manifest.service.default_port, 9100);
        assert!(manifest.service.has_dashboard);
        assert_eq!(manifest.tools.len(), 1);

        let tool = &manifest.tools[0];
        assert_eq!(tool.name, "wallet_watchlist");
        assert_eq!(tool.rpc_endpoint, "/rpc/watchlist");
        assert_eq!(tool.parameters.len(), 2);
        assert_eq!(tool.required_parameters(), vec!["action".to_string()]);
        assert_eq!(tool.tool_group(), crate::tools::types::ToolGroup::Finance);
    }

    #[test]
    fn test_parse_skill_dir_config() {
        let toml = r#"
[module]
name = "perps_trader"
version = "1.0.0"
description = "Perps trading module"

[service]
default_port = 9105

[skill]
skill_dir = "skill"
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        let skill = manifest.skill.unwrap();
        assert_eq!(skill.skill_dir.as_deref(), Some("skill"));
        assert!(skill.content_file.is_none());
    }

    #[test]
    fn test_parse_agent_config() {
        let toml = r#"
[module]
name = "perps_trader"
version = "1.0.0"
description = "Perps trading module"

[service]
default_port = 9105

[skill]
skill_dir = "skill"

[agent]
dir = "agent"
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        let agent = manifest.agent.unwrap();
        assert_eq!(agent.dir, "agent");
    }

    #[test]
    fn test_parse_legacy_content_file_still_works() {
        let toml = r#"
[module]
name = "wallet_monitor"
version = "1.0.0"
description = "Monitor ETH wallets"

[service]
default_port = 9100

[skill]
content_file = "skill.md"
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        let skill = manifest.skill.unwrap();
        assert_eq!(skill.content_file.as_deref(), Some("skill.md"));
        assert!(skill.skill_dir.is_none());
    }

    #[test]
    fn test_resolved_dashboard_styles() {
        // dashboard_styles (plural) takes priority
        let toml = r#"
[module]
name = "dual"
version = "1.0.0"
description = "Dual mode module"

[service]
default_port = 9200
dashboard_styles = ["tui", "html"]
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert_eq!(manifest.service.resolved_dashboard_styles(), vec!["tui", "html"]);

        // Falls back to singular dashboard_style
        let toml2 = r#"
[module]
name = "tui_only"
version = "1.0.0"
description = "TUI only"

[service]
default_port = 9200
dashboard_style = "tui"
"#;
        let manifest2 = ModuleManifest::from_str(toml2).unwrap();
        assert_eq!(manifest2.service.resolved_dashboard_styles(), vec!["tui"]);

        // Falls back to has_dashboard → ["html"]
        let toml3 = r#"
[module]
name = "html_only"
version = "1.0.0"
description = "HTML only"

[service]
default_port = 9200
has_dashboard = true
"#;
        let manifest3 = ModuleManifest::from_str(toml3).unwrap();
        assert_eq!(manifest3.service.resolved_dashboard_styles(), vec!["html"]);

        // No dashboard → empty
        let toml4 = r#"
[module]
name = "no_dash"
version = "1.0.0"
description = "No dashboard"

[service]
default_port = 9200
"#;
        let manifest4 = ModuleManifest::from_str(toml4).unwrap();
        assert!(manifest4.service.resolved_dashboard_styles().is_empty());
    }

    #[test]
    fn test_parse_no_agent_section() {
        let toml = r#"
[module]
name = "basic"
version = "1.0.0"
description = "Basic module"

[service]
default_port = 9200
"#;
        let manifest = ModuleManifest::from_str(toml).unwrap();
        assert!(manifest.agent.is_none());
    }
}
