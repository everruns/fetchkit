//! HTTP client for FetchKit
//!
//! This module provides the main entry points for fetching URLs.
//! The actual fetch logic is implemented by fetchers in the [`fetchers`](crate::fetchers) module.

use crate::error::FetchError;
use crate::fetchers::FetcherRegistry;
use crate::types::{FetchRequest, FetchResponse};

/// Fetch options that can be configured via tool builder
#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    /// Custom User-Agent
    pub user_agent: Option<String>,
    /// Allow list of URL prefixes
    pub allow_prefixes: Vec<String>,
    /// Block list of URL prefixes
    pub block_prefixes: Vec<String>,
    /// Enable as_markdown option
    pub enable_markdown: bool,
    /// Enable as_text option
    pub enable_text: bool,
}

/// Fetch a URL and return the response
///
/// Uses the default fetcher registry with all built-in fetchers.
/// Markdown and text conversions are enabled by default.
/// For custom options, use [`fetch_with_options`].
pub async fn fetch(req: FetchRequest) -> Result<FetchResponse, FetchError> {
    let options = FetchOptions {
        enable_markdown: true,
        enable_text: true,
        ..Default::default()
    };
    fetch_with_options(req, options).await
}

/// Fetch a URL with custom options
///
/// Uses the default fetcher registry with all built-in fetchers.
/// For custom fetcher configuration, use [`FetcherRegistry`] directly.
pub async fn fetch_with_options(
    req: FetchRequest,
    options: FetchOptions,
) -> Result<FetchResponse, FetchError> {
    // Validate URL early
    if req.url.is_empty() {
        return Err(FetchError::MissingUrl);
    }

    // Use registry with default fetchers
    let registry = FetcherRegistry::with_defaults();
    registry.fetch(req, options).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_empty_url() {
        let req = FetchRequest::new("");
        let result = fetch(req).await;
        assert!(matches!(result, Err(FetchError::MissingUrl)));
    }

    #[tokio::test]
    async fn test_fetch_invalid_scheme() {
        let req = FetchRequest::new("ftp://example.com");
        let result = fetch(req).await;
        assert!(matches!(result, Err(FetchError::InvalidUrlScheme)));
    }

    #[tokio::test]
    async fn test_fetch_options_default() {
        let options = FetchOptions::default();
        assert!(options.user_agent.is_none());
        assert!(options.allow_prefixes.is_empty());
        assert!(options.block_prefixes.is_empty());
        assert!(!options.enable_markdown);
        assert!(!options.enable_text);
    }
}
