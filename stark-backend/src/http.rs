use once_cell::sync::Lazy;
use reqwest::Client;
use std::time::Duration;

/// Global shared HTTP client singleton.
///
/// Reuses a single connection pool across all HTTP requests in the application.
/// `Client::clone()` is just an `Arc` increment — virtually free.
///
/// Callers that need auth headers should add them per-request via `.headers()`.
/// Callers that need a different timeout should override per-request via `.timeout()`.
static SHARED_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(5)
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create shared HTTP client")
});

/// Returns a reference to the global shared HTTP client.
pub fn shared_client() -> &'static Client {
    &SHARED_CLIENT
}

/// Build a new HTTP client configured to route all requests through the given proxy URL.
/// Uses the same pool/timeout settings as the shared client.
pub fn build_proxy_client(proxy_url: &str) -> Result<Client, reqwest::Error> {
    Client::builder()
        .proxy(reqwest::Proxy::all(proxy_url)?)
        .pool_max_idle_per_host(5)
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(120))
        .build()
}
