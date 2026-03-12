//! Fetcher system for specialized content fetching
//!
//! Design: Each fetcher handles specific URL patterns with custom logic.
//! FetcherRegistry dispatches to the first matching fetcher.

mod default;
mod github_repo;

pub use default::DefaultFetcher;
pub use github_repo::GitHubRepoFetcher;

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::types::{FetchRequest, FetchResponse};
use async_trait::async_trait;
use tracing::debug;
use url::Url;

/// Trait for specialized content fetchers
///
/// Implement this trait to create custom fetchers for specific URL patterns.
/// Each fetcher declares what URLs it can handle via `matches()` and
/// performs the actual fetch via `fetch()`.
#[async_trait]
pub trait Fetcher: Send + Sync {
    /// Unique identifier for this fetcher (for logging/debugging)
    fn name(&self) -> &'static str;

    /// Returns true if this fetcher can handle the given URL
    ///
    /// Called by the registry to determine which fetcher to use.
    /// More specific fetchers should be registered before generic ones.
    fn matches(&self, url: &Url) -> bool;

    /// Fetch content from the URL
    ///
    /// Called only if `matches()` returned true.
    /// Returns a FetchResponse on success or FetchError on failure.
    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError>;
}

/// Registry of fetchers that dispatches to the appropriate handler
///
/// Maintains an ordered list of fetchers. When fetching a URL, iterates
/// through fetchers and uses the first one that matches.
///
/// # Examples
///
/// ```
/// use fetchkit::FetcherRegistry;
///
/// // Create registry with built-in fetchers
/// let registry = FetcherRegistry::with_defaults();
///
/// // Or create empty and register custom fetchers
/// let mut registry = FetcherRegistry::new();
/// registry.register(Box::new(fetchkit::DefaultFetcher::new()));
/// ```
pub struct FetcherRegistry {
    fetchers: Vec<Box<dyn Fetcher>>,
}

impl Default for FetcherRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FetcherRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            fetchers: Vec::new(),
        }
    }

    /// Create a registry with default fetchers pre-registered
    ///
    /// Includes (in order of priority):
    /// 1. GitHubRepoFetcher - handles GitHub repository URLs
    /// 2. DefaultFetcher - handles all HTTP/HTTPS URLs
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Register specialized fetchers first (higher priority)
        registry.register(Box::new(GitHubRepoFetcher::new()));
        // Default fetcher last (catches all remaining URLs)
        registry.register(Box::new(DefaultFetcher::new()));
        registry
    }

    /// Register a fetcher
    ///
    /// Fetchers are checked in registration order, so register more
    /// specific fetchers before generic ones.
    pub fn register(&mut self, fetcher: Box<dyn Fetcher>) {
        self.fetchers.push(fetcher);
    }

    /// Fetch a URL using the appropriate fetcher
    ///
    /// Iterates through registered fetchers and uses the first one
    /// that matches the URL. Returns an error if no fetcher matches
    /// (shouldn't happen with DefaultFetcher registered).
    pub async fn fetch(
        &self,
        request: FetchRequest,
        options: FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        // Validate URL scheme early
        if !request.url.starts_with("http://") && !request.url.starts_with("https://") {
            return Err(FetchError::InvalidUrlScheme);
        }

        // Parse URL for matching
        let parsed_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;

        // THREAT[TM-INPUT-002]: Normalize URL before prefix matching to prevent
        // encoding-based bypasses (case, trailing dots, default ports)
        // THREAT[TM-INPUT-007]: URL-aware prefix matching prevents subdomain tricks
        // (e.g., blocking "http://internal.example.com" won't match "http://internal.example.com.evil.com")
        if !options.allow_prefixes.is_empty() {
            let allowed = options
                .allow_prefixes
                .iter()
                .any(|prefix| url_matches_policy_prefix(&parsed_url, prefix));
            if !allowed {
                debug!(url = %request.url, "URL not in allow list");
                return Err(FetchError::BlockedUrl);
            }
        }

        if options
            .block_prefixes
            .iter()
            .any(|prefix| url_matches_policy_prefix(&parsed_url, prefix))
        {
            debug!(url = %request.url, "URL matched block list");
            return Err(FetchError::BlockedUrl);
        }

        // Find matching fetcher
        for fetcher in &self.fetchers {
            if fetcher.matches(&parsed_url) {
                tracing::debug!(fetcher = fetcher.name(), url = %request.url, "Using fetcher");
                return fetcher.fetch(&request, &options).await;
            }
        }

        // No fetcher matched (shouldn't happen with DefaultFetcher)
        Err(FetchError::FetcherError(
            "No fetcher available for URL".to_string(),
        ))
    }
}

// THREAT[TM-INPUT-002]: URL-aware prefix matching normalizes both the URL and the prefix
// before comparison, preventing bypasses via encoding, case, or trailing dots.
// THREAT[TM-INPUT-007]: Compares parsed URL components (scheme, host, path) instead of
// raw strings, so "http://internal.example.com" won't match "http://internal.example.com.evil.com".
fn url_matches_policy_prefix(url: &Url, prefix: &str) -> bool {
    let Ok(prefix_url) = Url::parse(prefix) else {
        tracing::warn!(
            prefix,
            "Invalid policy prefix; falling back to raw string matching"
        );
        return url.as_str().starts_with(prefix);
    };

    if url.scheme() != prefix_url.scheme() {
        return false;
    }

    // Host comparison with trailing-dot normalization
    if normalized_host(url) != normalized_host(&prefix_url) {
        return false;
    }

    // Port matching: if the prefix specifies an explicit port, URLs must match that port.
    // If the prefix has no explicit port (uses default), match any port on that host.
    // This lets "http://127.0.0.1" block all ports on 127.0.0.1.
    if prefix_url.port().is_some()
        && url.port_or_known_default() != prefix_url.port_or_known_default()
    {
        return false;
    }

    if !path_matches_prefix(url.path(), prefix_url.path()) {
        return false;
    }

    match prefix_url.query() {
        Some(prefix_query) => url.query() == Some(prefix_query),
        None => true,
    }
}

fn normalized_host(url: &Url) -> Option<String> {
    url.host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

fn path_matches_prefix(path: &str, prefix_path: &str) -> bool {
    if prefix_path == "/" {
        return true;
    }

    if path == prefix_path {
        return true;
    }

    path.strip_prefix(prefix_path)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_with_defaults() {
        let registry = FetcherRegistry::with_defaults();
        assert_eq!(registry.fetchers.len(), 2);
        assert_eq!(registry.fetchers[0].name(), "github_repo");
        assert_eq!(registry.fetchers[1].name(), "default");
    }

    #[test]
    fn test_empty_registry() {
        let registry = FetcherRegistry::new();
        assert!(registry.fetchers.is_empty());
    }

    // THREAT[TM-INPUT-007]: URL-aware prefix matching tests
    #[test]
    fn test_policy_prefix_matches_same_origin_and_path_boundary() {
        let url = Url::parse("https://docs.example.com/api/v1").unwrap();
        assert!(url_matches_policy_prefix(
            &url,
            "https://docs.example.com/api"
        ));
        assert!(url_matches_policy_prefix(&url, "https://docs.example.com"));
        assert!(!url_matches_policy_prefix(
            &url,
            "https://docs.example.com/ap"
        ));
    }

    #[test]
    fn test_policy_prefix_rejects_lookalike_hosts() {
        let url = Url::parse("https://docs.example.com.evil.test/path").unwrap();
        assert!(!url_matches_policy_prefix(&url, "https://docs.example.com"));
    }

    #[test]
    fn test_policy_prefix_normalizes_case_default_port_and_trailing_dot() {
        let url = Url::parse("https://docs.example.com/path").unwrap();
        assert!(url_matches_policy_prefix(
            &url,
            "HTTPS://DOCS.EXAMPLE.COM.:443"
        ));
    }

    #[test]
    fn test_url_prefix_scheme_mismatch() {
        let url = Url::parse("http://example.com/page").unwrap();
        assert!(!url_matches_policy_prefix(&url, "https://example.com"));
    }

    #[test]
    fn test_url_prefix_port_handling() {
        let url = Url::parse("http://example.com:8080/page").unwrap();
        assert!(url_matches_policy_prefix(&url, "http://example.com:8080"));
        // Prefix without explicit port matches any port on that host
        assert!(url_matches_policy_prefix(&url, "http://example.com"));
        // Prefix with explicit port must match exactly
        assert!(!url_matches_policy_prefix(&url, "http://example.com:9090"));
    }

    // THREAT[TM-INPUT-002]: Normalization tests
    #[test]
    fn test_url_prefix_case_normalization() {
        // url crate normalizes host to lowercase
        let url = Url::parse("http://EXAMPLE.COM/page").unwrap();
        assert!(url_matches_policy_prefix(&url, "http://example.com"));
    }
}
