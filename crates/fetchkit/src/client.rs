//! HTTP client for FetchKit
//!
//! This module provides the main entry points for fetching URLs.
//! The actual fetch logic is implemented by fetchers in the [`fetchers`](crate::fetchers) module.

use crate::dns::DnsPolicy;
use crate::error::FetchError;
use crate::fetchers::FetcherRegistry;
use crate::types::{FetchRequest, FetchResponse};
use url::Url;

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
    /// DNS resolution policy for SSRF prevention
    pub dns_policy: DnsPolicy,
    /// Maximum response body size in bytes (default: 10 MB)
    /// Protects against TM-DOS-001 (unbounded body) and TM-DOS-003 (gzip bombs)
    pub max_body_size: Option<usize>,
    /// Enable save_to_file parameter in requests
    pub enable_save_to_file: bool,
    /// Whether to respect HTTP_PROXY/HTTPS_PROXY/NO_PROXY from the environment
    pub respect_proxy_env: bool,
    /// Restrict outbound requests to these ports. Empty means any port.
    pub allowed_ports: Vec<u16>,
    /// Block exact hosts and suffix rules. Leading '.' means suffix match.
    pub blocked_hosts: Vec<String>,
    /// Restrict redirects to the original host only.
    pub same_host_redirects_only: bool,
}

impl FetchOptions {
    pub(crate) fn validate_url(&self, url: &Url) -> Result<(), FetchError> {
        self.validate_host(url)?;
        self.validate_port(url)?;
        Ok(())
    }

    pub(crate) fn validate_redirect_target(
        &self,
        current_url: &Url,
        next_url: &Url,
    ) -> Result<(), FetchError> {
        self.validate_url(next_url)?;

        if self.same_host_redirects_only
            && normalized_host(current_url) != normalized_host(next_url)
        {
            return Err(FetchError::BlockedUrl);
        }

        Ok(())
    }

    fn validate_host(&self, url: &Url) -> Result<(), FetchError> {
        let Some(host) = normalized_host(url) else {
            return Ok(());
        };

        if self
            .blocked_hosts
            .iter()
            .any(|rule| host_matches_rule(&host, rule))
        {
            return Err(FetchError::BlockedUrl);
        }

        Ok(())
    }

    fn validate_port(&self, url: &Url) -> Result<(), FetchError> {
        if self.allowed_ports.is_empty() {
            return Ok(());
        }

        let port = url.port_or_known_default().ok_or(FetchError::BlockedUrl)?;
        if self.allowed_ports.contains(&port) {
            Ok(())
        } else {
            Err(FetchError::BlockedUrl)
        }
    }
}

fn normalized_host(url: &Url) -> Option<String> {
    url.host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

fn host_matches_rule(host: &str, rule: &str) -> bool {
    let normalized_rule = rule.trim_end_matches('.').to_ascii_lowercase();

    if let Some(suffix) = normalized_rule.strip_prefix('.') {
        host == suffix || host.ends_with(&format!(".{suffix}"))
    } else {
        host == normalized_rule
    }
}

/// Fetch a URL and return the response
///
/// Uses the default fetcher registry with all built-in fetchers.
/// Markdown and text conversions are enabled by default.
/// For custom options, use [`fetch_with_options`].
///
/// # Examples
///
/// ```no_run
/// use fetchkit::{FetchRequest, fetch};
///
/// # async fn example() -> Result<(), fetchkit::FetchError> {
/// let response = fetch(FetchRequest::new("https://example.com")).await?;
/// println!("Status: {}", response.status_code);
/// # Ok(())
/// # }
/// ```
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
        // Safe by default: private IPs blocked
        assert!(options.dns_policy.block_private);
        assert!(options.max_body_size.is_none());
        assert!(!options.enable_save_to_file);
        assert!(!options.respect_proxy_env);
        assert!(options.allowed_ports.is_empty());
        assert!(options.blocked_hosts.is_empty());
        assert!(!options.same_host_redirects_only);
    }

    #[test]
    fn test_validate_url_blocks_configured_host_and_port() {
        let options = FetchOptions {
            allowed_ports: vec![443],
            blocked_hosts: vec!["localhost".to_string(), ".internal".to_string()],
            ..Default::default()
        };

        assert!(matches!(
            options.validate_url(&Url::parse("https://api.internal").unwrap()),
            Err(FetchError::BlockedUrl)
        ));
        assert!(matches!(
            options.validate_url(&Url::parse("https://example.com:8443").unwrap()),
            Err(FetchError::BlockedUrl)
        ));
        assert!(options
            .validate_url(&Url::parse("https://example.com").unwrap())
            .is_ok());
    }

    #[test]
    fn test_validate_redirect_target_blocks_cross_host_when_enabled() {
        let options = FetchOptions {
            same_host_redirects_only: true,
            ..Default::default()
        };

        let current = Url::parse("https://example.com/start").unwrap();
        let next = Url::parse("https://www.example.com/end").unwrap();

        assert!(matches!(
            options.validate_redirect_target(&current, &next),
            Err(FetchError::BlockedUrl)
        ));
    }
}
