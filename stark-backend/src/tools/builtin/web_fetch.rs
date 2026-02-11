use crate::tools::http_retry::{is_reqwest_error_retryable, HttpRetryManager};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Deserialize a usize from either a number or a string
fn deserialize_usize_lenient<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_u64().map(|v| v as usize)),
        Some(Value::String(s)) => Ok(s.parse().ok()),
        _ => Ok(None),
    }
}

/// Cache entry for fetch results
struct CacheEntry {
    result: ToolResult,
    expires_at: Instant,
}

/// Simple in-memory cache with TTL
struct FetchCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl FetchCache {
    fn new(ttl_secs: u64) -> Self {
        FetchCache {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    fn get(&self, key: &str) -> Option<ToolResult> {
        let entries = self.entries.read().ok()?;
        if let Some(entry) = entries.get(key) {
            if entry.expires_at > Instant::now() {
                return Some(entry.result.clone());
            }
        }
        None
    }

    fn set(&self, key: String, result: ToolResult) {
        if let Ok(mut entries) = self.entries.write() {
            // Clean expired entries occasionally
            if entries.len() > 50 {
                let now = Instant::now();
                entries.retain(|_, v| v.expires_at > now);
            }
            entries.insert(
                key,
                CacheEntry {
                    result,
                    expires_at: Instant::now() + self.ttl,
                },
            );
        }
    }
}

/// Web fetch tool to retrieve and parse content from URLs
pub struct WebFetchTool {
    definition: ToolDefinition,
    cache: FetchCache,
}

impl WebFetchTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The URL to fetch content from (HTTP/HTTPS only)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "extract_mode".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Output format: 'markdown' for readable markdown, 'text' for plain text, 'raw' for unprocessed content".to_string(),
                default: Some(json!("markdown")),
                items: None,
                enum_values: Some(vec![
                    "markdown".to_string(),
                    "text".to_string(),
                    "raw".to_string(),
                ]),
            },
        );
        properties.insert(
            "max_chars".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum content length to return (default: 50000 characters)"
                    .to_string(),
                default: Some(json!(50000)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "headers".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "Optional HTTP headers to include in the request. Environment variables like $VAR_NAME will be expanded.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "method".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "HTTP method to use (default: GET)".to_string(),
                default: Some(json!("GET")),
                items: None,
                enum_values: Some(vec![
                    "GET".to_string(),
                    "POST".to_string(),
                    "PUT".to_string(),
                    "PATCH".to_string(),
                    "DELETE".to_string(),
                ]),
            },
        );
        properties.insert(
            "body".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "Request body for POST/PUT/PATCH requests. Will be sent as JSON.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "bearer_auth_token".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Bearer token for Authorization header. Use $VAR_NAME to reference an installed API key (e.g. '$FOMOLT_API_KEY'). Will be sent as 'Authorization: Bearer <token>'.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        WebFetchTool {
            definition: ToolDefinition {
                name: "web_fetch".to_string(),
                description: "Fetch content from a URL and extract readable text or markdown. Blocks private/internal URLs for security.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["url".to_string()],
                },
                group: ToolGroup::Web,
                hidden: false,
            },
            cache: FetchCache::new(900), // 15 minute cache
        }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct WebFetchParams {
    url: String,
    #[serde(alias = "max_length", default, deserialize_with = "deserialize_usize_lenient")]
    max_chars: Option<usize>,
    extract_mode: Option<String>,
    // Legacy parameter support
    extract_text: Option<bool>,
    // Optional custom headers
    headers: Option<HashMap<String, String>>,
    // HTTP method (GET, POST, PUT, PATCH, DELETE)
    method: Option<String>,
    // Request body for POST/PUT/PATCH
    body: Option<Value>,
    // Bearer auth token (convenience shorthand for Authorization header)
    bearer_auth_token: Option<String>,
}

/// Build a lookup map from env var names to values using the ToolContext's API keys.
/// Maps each ApiKeyId's env_vars() aliases to the stored value, then adds custom keys.
fn build_env_var_map(context: &ToolContext) -> HashMap<String, String> {
    use crate::controllers::api_keys::ApiKeyId;
    let mut map = HashMap::new();

    // Built-in keys with their env_var aliases
    for key_id in ApiKeyId::iter() {
        if let Some(value) = context.get_api_key_by_id(key_id) {
            if !value.is_empty() {
                if let Some(env_vars) = key_id.env_vars() {
                    for env_var in env_vars {
                        map.insert(env_var.to_string(), value.clone());
                    }
                }
            }
        }
    }

    // Custom keys from the runtime store (e.g., ALLIUM_API_KEY → $ALLIUM_API_KEY)
    for name in context.list_api_key_names() {
        if map.contains_key(&name) {
            continue; // built-in already handled
        }
        if let Some(value) = context.get_api_key(&name) {
            if !value.is_empty() {
                map.insert(name, value);
            }
        }
    }

    map
}

/// Expand variable references in a string (e.g., $VAR_NAME or ${VAR_NAME})
/// using API keys from the ToolContext rather than process environment variables.
fn expand_context_vars(s: &str, var_map: &HashMap<String, String>) -> String {
    let mut result = s.to_string();

    // Handle ${VAR_NAME} format
    let re_braces = regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    result = re_braces.replace_all(&result, |caps: &regex::Captures| {
        var_map.get(&caps[1]).cloned().unwrap_or_default()
    }).to_string();

    // Handle $VAR_NAME format
    let re_simple = regex::Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    result = re_simple.replace_all(&result, |caps: &regex::Captures| {
        var_map.get(&caps[1]).cloned().unwrap_or_default()
    }).to_string();

    result
}

#[async_trait]
impl Tool for WebFetchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: WebFetchParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let max_chars = params.max_chars.unwrap_or(50000);

        // Handle extract_mode with legacy extract_text fallback
        let extract_mode = params.extract_mode.unwrap_or_else(|| {
            if params.extract_text == Some(false) {
                "raw".to_string()
            } else {
                "markdown".to_string()
            }
        });

        // Validate URL scheme
        if !params.url.starts_with("http://") && !params.url.starts_with("https://") {
            return ToolResult::error("URL must start with http:// or https://");
        }

        // Parse and validate URL
        let url = match url::Url::parse(&params.url) {
            Ok(u) => u,
            Err(e) => return ToolResult::error(format!("Invalid URL: {}", e)),
        };

        // Check for private/internal hostnames
        if let Err(e) = validate_public_url(&url) {
            return ToolResult::error(e);
        }

        // Build cache key (include method - don't cache POST/PUT/PATCH/DELETE)
        let method = params.method.as_deref().unwrap_or("GET").to_uppercase();
        let should_cache = method == "GET";
        let cache_key = format!("{}:{}:{}", params.url, extract_mode, max_chars);

        // Check cache first (only for GET requests)
        if should_cache {
            if let Some(cached) = self.cache.get(&cache_key) {
                log::debug!("web_fetch: returning cached result for URL '{}'", params.url);
                return cached;
            }
        }

        let mut builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("StarkBot/1.0 (Web Fetch Tool)")
            .redirect(reqwest::redirect::Policy::limited(5));

        if let Some(ref proxy_url) = context.proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(proxy_url) {
                builder = builder.proxy(proxy);
            }
        }

        let client = builder.build()
            .unwrap_or_else(|_| reqwest::Client::new());

        // Extract host for retry tracking
        let retry_key = url.host_str().unwrap_or("unknown").to_string();
        let retry_manager = HttpRetryManager::global();

        // Build request with appropriate method
        let mut request = match method.as_str() {
            "POST" => client.post(&params.url),
            "PUT" => client.put(&params.url),
            "PATCH" => client.patch(&params.url),
            "DELETE" => client.delete(&params.url),
            _ => client.get(&params.url),
        };

        // Add request body for POST/PUT/PATCH
        if let Some(ref body) = params.body {
            if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
                // Log the body type and content for debugging API issues
                let body_type = match body {
                    Value::Object(_) => "object",
                    Value::String(_) => "string (WARNING: may cause issues with GraphQL APIs)",
                    Value::Array(_) => "array",
                    _ => "other",
                };
                log::debug!("web_fetch POST body type={}, content={}", body_type,
                    serde_json::to_string(body).unwrap_or_else(|_| "<serialize error>".to_string()));

                // If body is a string that looks like JSON, try parsing it as a JSON object
                // This handles the case where the AI passes a JSON string instead of a JSON object
                let effective_body = if let Value::String(s) = body {
                    match serde_json::from_str::<Value>(s) {
                        Ok(parsed) if parsed.is_object() => {
                            log::info!("web_fetch: auto-parsed JSON string body into object");
                            parsed
                        }
                        _ => body.clone(),
                    }
                } else {
                    body.clone()
                };

                // Serialize body manually and use .body() instead of .json()
                // to avoid duplicate Content-Type headers (.json() auto-adds one,
                // which can conflict with custom headers and cause API rejections)
                let body_str = serde_json::to_string(&effective_body)
                    .unwrap_or_else(|_| effective_body.to_string());
                // Only add default Content-Type if custom headers don't already include one
                let has_custom_content_type = params.headers.as_ref()
                    .map(|h| h.keys().any(|k| k.eq_ignore_ascii_case("content-type")))
                    .unwrap_or(false);
                if !has_custom_content_type {
                    request = request.header("Content-Type", "application/json");
                }
                request = request.body(body_str);
            }
        }

        // Build env var map once (needed for both bearer_auth_token and headers)
        let var_map = build_env_var_map(context);

        // Add bearer auth token if provided
        if let Some(ref token) = params.bearer_auth_token {
            let expanded_token = expand_context_vars(token, &var_map);
            if !expanded_token.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", expanded_token));
            }
        }

        // Add optional headers (can override bearer auth if needed)
        if let Some(ref headers) = params.headers {
            for (key, value) in headers {
                let expanded_value = expand_context_vars(value, &var_map);
                if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                    if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&expanded_value) {
                        request = request.header(header_name, header_value);
                    }
                }
            }
        }

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                let error_msg = format!("Failed to fetch URL: {}", e);
                if is_reqwest_error_retryable(&e) {
                    let delay = retry_manager.record_error(&retry_key);
                    return ToolResult::retryable_error(error_msg, delay);
                }
                return ToolResult::error(error_msg);
            }
        };

        let final_url = response.url().to_string();
        let status = response.status();

        if !status.is_success() {
            // Extract the response body to include in the error message (truncate to avoid huge HTML pages)
            let body = response.text().await.unwrap_or_default();
            let truncated_body = if body.len() > 2000 {
                format!("{}...\n[truncated, {} total bytes]", &body[..2000], body.len())
            } else {
                body
            };
            let error_msg = if truncated_body.is_empty() {
                format!("HTTP error: {} for URL: {}", status, params.url)
            } else {
                format!("HTTP error: {} for URL: {}\n\nResponse body:\n{}", status, params.url, truncated_body)
            };
            if HttpRetryManager::is_retryable_status(status.as_u16()) {
                let delay = retry_manager.record_error(&retry_key);
                return ToolResult::retryable_error(error_msg, delay);
            }
            return ToolResult::error(error_msg);
        }

        // Success - reset backoff for this host
        retry_manager.record_success(&retry_key);

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("Failed to read response body: {}", e)),
        };

        let original_length = body.len();
        let is_html = content_type.contains("text/html");

        let content = match extract_mode.as_str() {
            "raw" => body,
            "text" if is_html => extract_text_from_html(&body),
            "markdown" if is_html => extract_markdown_from_html(&body),
            _ => body, // For non-HTML, return as-is
        };

        // Truncate if necessary
        let truncated = content.len() > max_chars;
        let final_content = if truncated {
            format!(
                "{}\n\n[Content truncated at {} characters. Original length: {} characters]",
                &content[..max_chars],
                max_chars,
                content.len()
            )
        } else {
            content
        };

        let result = ToolResult::success(final_content).with_metadata(json!({
            "url": params.url,
            "final_url": final_url,
            "content_type": content_type,
            "extract_mode": extract_mode,
            "truncated": truncated,
            "original_length": original_length,
            "cached": false
        }));

        // Cache successful GET results only
        if should_cache {
            self.cache.set(cache_key, result.clone());
        }

        result
    }
}

/// Validate that a URL points to a public host (not private/internal)
fn validate_public_url(url: &url::Url) -> Result<(), String> {
    let host = url.host_str().ok_or("URL has no host")?;

    // Block localhost and common internal hostnames
    let blocked_hosts = [
        "localhost",
        "127.0.0.1",
        "0.0.0.0",
        "::1",
        "[::1]",
        "metadata.google.internal",
        "metadata.google",
        "169.254.169.254", // AWS/GCP metadata
    ];

    let host_lower = host.to_lowercase();
    if blocked_hosts.contains(&host_lower.as_str()) {
        return Err(format!("Access to internal host '{}' is blocked", host));
    }

    // Block .local, .internal, .localhost TLDs
    if host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".lan")
    {
        return Err(format!("Access to internal domain '{}' is blocked", host));
    }

    // Try to resolve and check if it's a private IP
    let port = url.port().unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
    if let Ok(addrs) = format!("{}:{}", host, port).to_socket_addrs() {
        for addr in addrs {
            if is_private_ip(addr.ip()) {
                return Err(format!(
                    "URL resolves to private IP address '{}', access blocked",
                    addr.ip()
                ));
            }
        }
    }

    Ok(())
}

/// Check if an IP address is private/internal
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_private()           // 10.x, 172.16-31.x, 192.168.x
                || ipv4.is_loopback()   // 127.x
                || ipv4.is_link_local() // 169.254.x
                || ipv4.is_broadcast()
                || ipv4.is_documentation()
                || ipv4.is_unspecified()
                // Cloud metadata IPs
                || ipv4.octets()[0] == 169 && ipv4.octets()[1] == 254
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback() || ipv6.is_unspecified()
            // Note: is_unique_local() and is_unicast_link_local() are unstable
        }
    }
}

/// Extract readable markdown from HTML
fn extract_markdown_from_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut current_tag = String::new();
    let mut tag_stack: Vec<String> = Vec::new();
    let mut last_was_block = false;

    let html_lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let chars_lower: Vec<char> = html_lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Check for script/style tags
        if i + 7 < chars_lower.len() {
            let slice: String = chars_lower[i..i + 7].iter().collect();
            if slice == "<script" {
                in_script = true;
            }
            if slice == "</scrip" {
                in_script = false;
            }
        }
        if i + 6 < chars_lower.len() {
            let slice: String = chars_lower[i..i + 6].iter().collect();
            if slice == "<style" {
                in_style = true;
            }
            if slice == "</styl" {
                in_style = false;
            }
        }

        if c == '<' {
            in_tag = true;
            current_tag.clear();
            i += 1;
            continue;
        }

        if c == '>' {
            in_tag = false;
            let tag_lower = current_tag.to_lowercase();
            let tag_name = tag_lower.split_whitespace().next().unwrap_or("");
            let is_closing = tag_name.starts_with('/');
            let base_tag = tag_name.trim_start_matches('/');

            // Handle markdown formatting based on tags
            match base_tag {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    if !is_closing {
                        if !result.ends_with('\n') && !result.is_empty() {
                            result.push_str("\n\n");
                        }
                        let level = base_tag.chars().last().unwrap().to_digit(10).unwrap_or(1);
                        result.push_str(&"#".repeat(level as usize));
                        result.push(' ');
                        tag_stack.push(base_tag.to_string());
                    } else {
                        tag_stack.pop();
                        result.push_str("\n\n");
                        last_was_block = true;
                    }
                }
                "p" | "div" | "article" | "section" => {
                    if is_closing && !last_was_block {
                        result.push_str("\n\n");
                        last_was_block = true;
                    } else if !is_closing {
                        last_was_block = false;
                    }
                }
                "br" => {
                    result.push('\n');
                }
                "li" => {
                    if !is_closing {
                        if !result.ends_with('\n') {
                            result.push('\n');
                        }
                        result.push_str("- ");
                    } else {
                        result.push('\n');
                    }
                }
                "strong" | "b" => {
                    result.push_str("**");
                }
                "em" | "i" => {
                    result.push('*');
                }
                "code" => {
                    result.push('`');
                }
                "pre" => {
                    if !is_closing {
                        result.push_str("\n```\n");
                    } else {
                        result.push_str("\n```\n");
                    }
                }
                "a" => {
                    if !is_closing {
                        // Extract href
                        if let Some(href_start) = tag_lower.find("href=\"") {
                            let href_content = &current_tag[href_start + 6..];
                            if let Some(href_end) = href_content.find('"') {
                                let href = &href_content[..href_end];
                                tag_stack.push(format!("a:{}", href));
                            }
                        }
                        result.push('[');
                    } else {
                        result.push(']');
                        // Find matching opening tag with href
                        if let Some(pos) = tag_stack.iter().rposition(|t| t.starts_with("a:")) {
                            let href = tag_stack[pos].strip_prefix("a:").unwrap_or("");
                            result.push_str(&format!("({})", href));
                            tag_stack.remove(pos);
                        }
                    }
                }
                "blockquote" => {
                    if !is_closing {
                        result.push_str("\n> ");
                    } else {
                        result.push('\n');
                    }
                }
                "hr" => {
                    result.push_str("\n---\n");
                }
                _ => {}
            }
            current_tag.clear();
            i += 1;
            continue;
        }

        if in_tag {
            current_tag.push(c);
            i += 1;
            continue;
        }

        if !in_script && !in_style {
            // Handle HTML entities
            if c == '&' {
                let remaining: String = chars[i..].iter().take(10).collect();
                if remaining.starts_with("&nbsp;") {
                    result.push(' ');
                    i += 6;
                    continue;
                } else if remaining.starts_with("&amp;") {
                    result.push('&');
                    i += 5;
                    continue;
                } else if remaining.starts_with("&lt;") {
                    result.push('<');
                    i += 4;
                    continue;
                } else if remaining.starts_with("&gt;") {
                    result.push('>');
                    i += 4;
                    continue;
                } else if remaining.starts_with("&quot;") {
                    result.push('"');
                    i += 6;
                    continue;
                } else if remaining.starts_with("&apos;") {
                    result.push('\'');
                    i += 6;
                    continue;
                } else if remaining.starts_with("&#") {
                    if let Some(end) = remaining.find(';') {
                        let code_str = &remaining[2..end];
                        let code = if code_str.starts_with('x') || code_str.starts_with('X') {
                            u32::from_str_radix(&code_str[1..], 16).ok()
                        } else {
                            code_str.parse::<u32>().ok()
                        };
                        if let Some(code) = code {
                            if let Some(ch) = char::from_u32(code) {
                                result.push(ch);
                                i += end + 1;
                                continue;
                            }
                        }
                    }
                }
            }

            result.push(c);
            if !c.is_whitespace() {
                last_was_block = false;
            }
        }

        i += 1;
    }

    // Clean up the result
    clean_text(&result)
}

/// Extract plain text from HTML (simpler extraction)
fn extract_text_from_html(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_space = false;

    let html_lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let chars_lower: Vec<char> = html_lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Check for script/style tags
        if i + 7 < chars_lower.len() {
            let slice: String = chars_lower[i..i + 7].iter().collect();
            if slice == "<script" {
                in_script = true;
            }
            if slice == "</scrip" {
                in_script = false;
            }
        }
        if i + 6 < chars_lower.len() {
            let slice: String = chars_lower[i..i + 6].iter().collect();
            if slice == "<style" {
                in_style = true;
            }
            if slice == "</styl" {
                in_style = false;
            }
        }

        if c == '<' {
            in_tag = true;
            i += 1;
            continue;
        }

        if c == '>' {
            in_tag = false;
            // Add newline after certain tags
            if i >= 3 {
                let prev: String = chars_lower[i.saturating_sub(3)..i].iter().collect();
                if prev.contains("/p")
                    || prev.contains("br")
                    || prev.contains("/h")
                    || prev.contains("/li")
                    || prev.contains("/tr")
                    || prev.contains("/di")
                {
                    if !last_was_space {
                        text.push('\n');
                        last_was_space = true;
                    }
                }
            }
            i += 1;
            continue;
        }

        if !in_tag && !in_script && !in_style {
            // Handle HTML entities
            if c == '&' {
                let remaining: String = chars[i..].iter().take(10).collect();
                if remaining.starts_with("&nbsp;") {
                    text.push(' ');
                    i += 6;
                    continue;
                } else if remaining.starts_with("&amp;") {
                    text.push('&');
                    i += 5;
                    continue;
                } else if remaining.starts_with("&lt;") {
                    text.push('<');
                    i += 4;
                    continue;
                } else if remaining.starts_with("&gt;") {
                    text.push('>');
                    i += 4;
                    continue;
                } else if remaining.starts_with("&quot;") {
                    text.push('"');
                    i += 6;
                    continue;
                } else if remaining.starts_with("&#") {
                    // Numeric entity
                    if let Some(end) = remaining.find(';') {
                        if let Ok(code) = remaining[2..end].parse::<u32>() {
                            if let Some(ch) = char::from_u32(code) {
                                text.push(ch);
                                i += end + 1;
                                continue;
                            }
                        }
                    }
                }
            }

            // Normalize whitespace
            if c.is_whitespace() {
                if !last_was_space {
                    text.push(' ');
                    last_was_space = true;
                }
            } else {
                text.push(c);
                last_was_space = false;
            }
        }

        i += 1;
    }

    clean_text(&text)
}

/// Clean up extracted text
fn clean_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        // Collapse multiple newlines
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_from_html() {
        let html = r#"
        <html>
        <head><title>Test</title></head>
        <body>
            <h1>Hello World</h1>
            <p>This is a <b>test</b> paragraph.</p>
            <script>var x = 1;</script>
            <p>Second paragraph with &amp; entity.</p>
        </body>
        </html>
        "#;

        let text = extract_text_from_html(html);
        assert!(text.contains("Hello World"));
        assert!(text.contains("This is a test paragraph."));
        assert!(text.contains("&"));
        assert!(!text.contains("var x = 1"));
    }

    #[test]
    fn test_extract_markdown_from_html() {
        let html = r#"
        <h1>Main Title</h1>
        <p>This is <strong>bold</strong> and <em>italic</em>.</p>
        <ul>
            <li>Item 1</li>
            <li>Item 2</li>
        </ul>
        "#;

        let md = extract_markdown_from_html(html);
        assert!(md.contains("# Main Title"));
        assert!(md.contains("**bold**"));
        assert!(md.contains("*italic*"));
        assert!(md.contains("- Item 1"));
    }

    #[test]
    fn test_private_ip_detection() {
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.1.1".parse().unwrap()));
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
    }
}
