//! StarkHub API client — search, download, and publish modules from hub.starkbot.ai

use serde::{Deserialize, Serialize};

const DEFAULT_HUB_URL: &str = "https://hub.starkbot.ai/api";

/// Client for the StarkHub module registry API.
pub struct StarkHubClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSummary {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author_address: String,
    pub author_username: Option<String>,
    pub tools_provided: Vec<String>,
    pub install_count: i32,
    pub featured: Option<bool>,
    pub license: Option<String>,
    pub x402_cost: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDetail {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: ModuleAuthor,
    pub manifest: serde_json::Value,
    pub tools_provided: Vec<String>,
    pub install_count: i32,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub platforms: Vec<PlatformBinary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleAuthor {
    pub wallet_address: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformBinary {
    pub platform: String,
    pub file_size: i64,
    pub sha256_checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub download_url: String,
    pub sha256_checksum: String,
    pub file_size: i64,
    pub version: String,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: PaginationMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationMeta {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
}

impl StarkHubClient {
    pub fn new() -> Self {
        let base_url = std::env::var("STARKHUB_API_URL")
            .unwrap_or_else(|_| DEFAULT_HUB_URL.to_string());
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    /// Get featured modules from StarkHub.
    pub async fn get_featured_modules(&self) -> Result<Vec<ModuleSummary>, String> {
        let url = format!("{}/modules/featured", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to StarkHub: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("StarkHub returned HTTP {}", resp.status()));
        }

        resp.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))
    }

    /// Search modules on StarkHub.
    pub async fn search_modules(&self, query: &str) -> Result<Vec<ModuleSummary>, String> {
        let url = format!("{}/modules/search", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("q", query)])
            .send()
            .await
            .map_err(|e| format!("Failed to connect to StarkHub: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("StarkHub returned HTTP {}", resp.status()));
        }

        let paginated: PaginatedResponse<ModuleSummary> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(paginated.data)
    }

    /// Get module detail by @username/slug.
    pub async fn get_module(&self, username: &str, slug: &str) -> Result<ModuleDetail, String> {
        let url = format!("{}/modules/@{}/{}", self.base_url, username, slug);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to StarkHub: {}", e))?;

        if resp.status().as_u16() == 404 {
            return Err(format!("Module @{}/{} not found on StarkHub", username, slug));
        }
        if !resp.status().is_success() {
            return Err(format!("StarkHub returned HTTP {}", resp.status()));
        }

        resp.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))
    }

    /// Get download info for a module binary.
    pub async fn get_download_info(
        &self,
        username: &str,
        slug: &str,
        platform: &str,
    ) -> Result<DownloadInfo, String> {
        let url = format!(
            "{}/modules/@{}/{}/download/{}",
            self.base_url, username, slug, platform
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to StarkHub: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Download failed: {}", body));
        }

        resp.json()
            .await
            .map_err(|e| format!("Failed to parse download info: {}", e))
    }

    /// Download a module binary archive and return the bytes.
    pub async fn download_binary(&self, download_url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .http
            .get(download_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download binary: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Download returned HTTP {}", resp.status()));
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read binary data: {}", e))
    }

    /// Publish a module manifest to StarkHub.
    pub async fn publish_module(
        &self,
        manifest_toml: &str,
        auth_token: &str,
    ) -> Result<String, String> {
        let url = format!("{}/modules", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", auth_token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "manifest_toml": manifest_toml }))
            .send()
            .await
            .map_err(|e| format!("Failed to connect to StarkHub: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Publish failed: {}", body));
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(result
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string())
    }
}

/// Detect the current platform string for binary downloads.
pub fn current_platform() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "linux-x86_64" }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "linux-aarch64" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "darwin-x86_64" }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "darwin-aarch64" }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    { "unknown" }
}
