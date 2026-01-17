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

        // Check allow/block lists before fetcher matching
        if !options.allow_prefixes.is_empty() {
            let allowed = options
                .allow_prefixes
                .iter()
                .any(|prefix| request.url.starts_with(prefix));
            if !allowed {
                return Err(FetchError::BlockedUrl);
            }
        }

        if options
            .block_prefixes
            .iter()
            .any(|prefix| request.url.starts_with(prefix))
        {
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
}
