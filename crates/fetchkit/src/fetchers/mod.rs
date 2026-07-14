//! Fetcher system for specialized content fetching
//!
//! Design: Each fetcher handles specific URL patterns with custom logic.
//! FetcherRegistry dispatches to the first matching fetcher.

mod arxiv;
mod crossref;
mod default;
mod docs_site;
mod github_actions_run;
mod github_code;
mod github_issue;
mod github_release;
mod github_repo;
mod gitlab;
mod hackernews;
mod ietf_rfc;
mod jupyter_notebook;
mod package_registry;
mod pubmed;
mod rss_feed;
mod stackoverflow;
mod twitter;
mod wikipedia;
mod youtube;

pub use arxiv::ArXivFetcher;
pub use crossref::CrossrefFetcher;
pub use default::DefaultFetcher;
pub use docs_site::DocsSiteFetcher;
pub use github_actions_run::GitHubActionsRunFetcher;
pub use github_code::GitHubCodeFetcher;
pub use github_issue::GitHubIssueFetcher;
pub use github_release::GitHubReleaseFetcher;
pub use github_repo::GitHubRepoFetcher;
pub use gitlab::GitLabFetcher;
pub use hackernews::HackerNewsFetcher;
pub use ietf_rfc::IetfRfcFetcher;
pub use jupyter_notebook::JupyterNotebookFetcher;
pub use package_registry::PackageRegistryFetcher;
pub use pubmed::PubMedFetcher;
pub use rss_feed::RSSFeedFetcher;
pub use stackoverflow::StackOverflowFetcher;
pub use twitter::TwitterFetcher;
pub use wikipedia::WikipediaFetcher;
pub use youtube::YouTubeFetcher;

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::file_saver::FileSaver;
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

    /// Fetch with file saving support.
    ///
    /// Default implementation delegates to `fetch()`, then saves content through the saver.
    /// Specialized fetchers (e.g. [`DefaultFetcher`]) override this for binary-aware saving.
    async fn fetch_to_file(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
        saver: &dyn FileSaver,
    ) -> Result<FetchResponse, FetchError> {
        let save_path = preflight_save_path(request, saver).await?;
        let response = self.fetch(request, options).await?;
        if let (Some(path), Some(content)) = (save_path, &response.content) {
            let result = saver
                .save(path, content.as_bytes())
                .await
                .map_err(|e| FetchError::SaveError(e.to_string()))?;
            Ok(FetchResponse {
                saved_path: Some(result.path),
                bytes_written: Some(result.bytes_written),
                content: None,
                ..response
            })
        } else {
            Ok(response)
        }
    }
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
    /// 1. JupyterNotebookFetcher - handles GitHub and GitLab notebook blob URLs
    /// 2. GitHubCodeFetcher - handles GitHub blob/file URLs
    /// 3. GitHubIssueFetcher - handles GitHub issue/PR URLs
    /// 4. GitHubReleaseFetcher - handles GitHub release URLs
    /// 5. GitHubRepoFetcher - handles GitHub repository URLs
    /// 6. GitLabFetcher - handles GitLab project and resource URLs
    /// 7. TwitterFetcher - handles Twitter/X tweet URLs
    /// 8. StackOverflowFetcher - handles Stack Exchange Q&A URLs
    /// 9. DocsSiteFetcher - handles docs sites and llms.txt URLs
    /// 10. DefaultFetcher - handles all remaining HTTP/HTTPS URLs
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Register specialized fetchers first (higher priority)
        // Notebooks precede the generic GitHub and GitLab blob fetchers.
        registry.register(Box::new(JupyterNotebookFetcher::new()));
        // GitHub fetchers: code > issue > release > repo (most specific first)
        registry.register(Box::new(GitHubCodeFetcher::new()));
        registry.register(Box::new(GitHubActionsRunFetcher::new()));
        registry.register(Box::new(GitHubIssueFetcher::new()));
        registry.register(Box::new(GitHubReleaseFetcher::new()));
        registry.register(Box::new(GitHubRepoFetcher::new()));
        registry.register(Box::new(GitLabFetcher::new()));
        registry.register(Box::new(TwitterFetcher::new()));
        registry.register(Box::new(StackOverflowFetcher::new()));
        registry.register(Box::new(PackageRegistryFetcher::new()));
        registry.register(Box::new(WikipediaFetcher::new()));
        registry.register(Box::new(YouTubeFetcher::new()));
        registry.register(Box::new(ArXivFetcher::new()));
        registry.register(Box::new(CrossrefFetcher::new()));
        registry.register(Box::new(IetfRfcFetcher::new()));
        registry.register(Box::new(PubMedFetcher::new()));
        registry.register(Box::new(HackerNewsFetcher::new()));
        registry.register(Box::new(RSSFeedFetcher::new()));
        // DocsSiteFetcher for docs sites and llms.txt
        registry.register(Box::new(DocsSiteFetcher::new()));
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

    /// Validate URL and find the matching fetcher.
    fn validate_and_find_fetcher<'a>(
        &'a self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<(&'a dyn Fetcher, Url), FetchError> {
        // Validate URL scheme early
        if !request.url.starts_with("http://") && !request.url.starts_with("https://") {
            return Err(FetchError::InvalidUrlScheme);
        }

        let parsed_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;

        options.validate_url(&parsed_url)?;

        // THREAT[TM-INPUT-002]: Normalize URL before prefix matching to prevent
        // encoding-based bypasses (case, trailing dots, default ports)
        // THREAT[TM-INPUT-007]: URL-aware prefix matching prevents subdomain tricks
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

        for fetcher in &self.fetchers {
            if fetcher.matches(&parsed_url) {
                return Ok((fetcher.as_ref(), parsed_url));
            }
        }

        Err(FetchError::FetcherError(
            "No fetcher available for URL".to_string(),
        ))
    }

    /// Fetch a URL using the appropriate fetcher
    ///
    /// Iterates through registered fetchers and uses the first one
    /// that matches the URL. Returns an error if no fetcher matches
    /// (shouldn't happen with DefaultFetcher registered).
    pub async fn fetch(
        &self,
        mut request: FetchRequest,
        options: FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        request.normalize_url_for_fetch()?;
        let (fetcher, _) = self.validate_and_find_fetcher(&request, &options)?;
        debug!(fetcher = fetcher.name(), url = %request.url, "Using fetcher");
        fetcher.fetch(&request, &options).await
    }

    /// Fetch a URL and save to file using the appropriate fetcher
    pub async fn fetch_to_file(
        &self,
        mut request: FetchRequest,
        options: FetchOptions,
        saver: &dyn FileSaver,
    ) -> Result<FetchResponse, FetchError> {
        request.normalize_url_for_fetch()?;
        let (fetcher, _) = self.validate_and_find_fetcher(&request, &options)?;
        preflight_save_path(&request, saver).await?;
        tracing::debug!(fetcher = fetcher.name(), url = %request.url, "Using fetcher (save to file)");
        fetcher.fetch_to_file(&request, &options, saver).await
    }
}

// THREAT[TM-INPUT-010]: Invalid file destinations must fail before any outbound request.
// Mitigation: reject blank paths and invoke the adapter's preflight validation first.
async fn preflight_save_path<'a>(
    request: &'a FetchRequest,
    saver: &dyn FileSaver,
) -> Result<Option<&'a str>, FetchError> {
    let Some(path) = request.save_to_file.as_deref() else {
        return Ok(None);
    };

    if path.trim().is_empty() {
        return Err(FetchError::SaveError(
            "Path not allowed: Path must name a file".to_string(),
        ));
    }

    saver
        .validate_path(path)
        .await
        .map_err(|error| FetchError::SaveError(error.to_string()))?;
    Ok(Some(path))
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
        assert_eq!(registry.fetchers[0].name(), "jupyter_notebook");
        assert_eq!(registry.fetchers[1].name(), "github_code");
        assert_eq!(registry.fetchers[2].name(), "github_actions_run");
        assert_eq!(registry.fetchers[3].name(), "github_issue");
        assert_eq!(registry.fetchers[4].name(), "github_release");
        assert_eq!(registry.fetchers[5].name(), "github_repo");
        assert_eq!(registry.fetchers[6].name(), "gitlab");
        assert_eq!(registry.fetchers[7].name(), "twitter_tweet");
        assert_eq!(registry.fetchers[8].name(), "stackoverflow");
        assert_eq!(registry.fetchers[9].name(), "package_registry");
        assert_eq!(registry.fetchers[10].name(), "wikipedia");
        assert_eq!(registry.fetchers[11].name(), "youtube");
        assert_eq!(registry.fetchers[12].name(), "arxiv");
        assert_eq!(registry.fetchers[13].name(), "crossref");
        assert_eq!(registry.fetchers[14].name(), "ietf_rfc");
        assert_eq!(registry.fetchers[15].name(), "pubmed");
        assert_eq!(registry.fetchers[16].name(), "hackernews");
        assert_eq!(registry.fetchers[17].name(), "rss_feed");
        assert_eq!(registry.fetchers[18].name(), "docs_site");
        assert_eq!(registry.fetchers[19].name(), "default");
        assert_eq!(registry.fetchers.len(), 20);
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
