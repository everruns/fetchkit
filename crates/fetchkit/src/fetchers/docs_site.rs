//! Documentation site fetcher with llms.txt support
//!
//! Detects known documentation sites and the llms.txt standard,
//! returning clean content optimized for LLM consumption.
//!
//! Design: Matches known documentation site patterns (ReadTheDocs, docs.rs,
//! Docusaurus, etc.) and explicit llms.txt/llms-full.txt URLs. For matched
//! sites, probes for llms.txt before fetching the page. Falls through to
//! DefaultFetcher for non-docs URLs.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderValue, USER_AGENT};
use std::time::Duration;
use url::Url;

/// Timeout for API/probe requests
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Max size for llms.txt content (2 MB)
const MAX_LLMS_TXT_SIZE: usize = 2 * 1024 * 1024;

/// Known documentation site patterns (host suffixes or exact matches)
const DOCS_HOSTS: &[&str] = &[
    ".readthedocs.io",
    ".readthedocs.org",
    "docs.rs",
    ".gitbook.io",
    ".netlify.app", // Many docs sites use Netlify
    ".vercel.app",  // Many docs sites use Vercel
];

/// Known documentation site host prefixes
const DOCS_HOST_PREFIXES: &[&str] = &["docs.", "wiki.", "developer.", "devdocs."];

/// Documentation site fetcher with llms.txt support
///
/// Matches known documentation sites and explicit llms.txt URLs.
/// For matched sites, probes for llms-full.txt/llms.txt at the origin
/// before returning content.
pub struct DocsSiteFetcher;

impl DocsSiteFetcher {
    pub fn new() -> Self {
        Self
    }

    /// Check if a URL is a direct llms.txt request
    fn is_llms_txt_url(url: &Url) -> bool {
        let path = url.path();
        path == "/llms.txt" || path == "/llms-full.txt"
    }

    /// Check if a URL belongs to a known documentation site
    fn is_docs_site(url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let host = host.to_ascii_lowercase();

        // Check known host suffixes
        for suffix in DOCS_HOSTS {
            if host.ends_with(suffix) {
                return true;
            }
        }

        // Check known host prefixes
        for prefix in DOCS_HOST_PREFIXES {
            if host.starts_with(prefix) {
                return true;
            }
        }

        false
    }
}

impl Default for DocsSiteFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for DocsSiteFetcher {
    fn name(&self) -> &'static str {
        "docs_site"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::is_llms_txt_url(url) || Self::is_docs_site(url)
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;

        let user_agent = options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        let mut client_builder = reqwest::Client::builder()
            .connect_timeout(PROBE_TIMEOUT)
            .timeout(PROBE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(5));

        if !options.respect_proxy_env {
            // THREAT[TM-NET-004]: Ignore ambient proxy env by default
            client_builder = client_builder.no_proxy();
        }

        let client = client_builder
            .build()
            .map_err(FetchError::ClientBuildError)?;

        let ua_header = HeaderValue::from_str(user_agent)
            .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT));

        // If this IS a direct llms.txt URL, fetch it directly
        if Self::is_llms_txt_url(&url) {
            return fetch_llms_txt_direct(&client, &request.url, &ua_header, request).await;
        }

        // For docs sites, probe for llms.txt at origin
        let origin = format!(
            "{}://{}{}",
            url.scheme(),
            url.host_str().unwrap_or_default(),
            url.port().map(|p| format!(":{}", p)).unwrap_or_default()
        );

        // Try llms-full.txt first, then llms.txt
        let probe_urls = [
            (format!("{}/llms-full.txt", origin), "llms-full.txt"),
            (format!("{}/llms.txt", origin), "llms.txt"),
        ];

        for (probe_url, source) in &probe_urls {
            if let Some(content) = try_fetch_llms_txt(&client, probe_url, &ua_header).await {
                return Ok(FetchResponse {
                    url: request.url.clone(),
                    status_code: 200,
                    content_type: Some("text/plain".to_string()),
                    format: Some("documentation".to_string()),
                    content: Some(format!("<!-- Source: {} -->\n\n{}", source, content)),
                    ..Default::default()
                });
            }
        }

        // No llms.txt — fetch the docs page directly and return raw content
        let response = client
            .get(&request.url)
            .header(USER_AGENT, ua_header)
            .send()
            .await
            .map_err(FetchError::from_reqwest)?;

        let status_code = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body = response
            .text()
            .await
            .map_err(|e| FetchError::RequestError(e.to_string()))?;

        // If HTML, convert to markdown for cleaner docs consumption
        let (content, format) = if content_type
            .as_deref()
            .is_some_and(|ct| ct.contains("text/html"))
        {
            (
                crate::convert::html_to_markdown(&body),
                "markdown".to_string(),
            )
        } else {
            (body, "documentation".to_string())
        };

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code,
            content_type,
            format: Some(format),
            content: Some(content),
            ..Default::default()
        })
    }
}

/// Fetch a direct llms.txt URL
async fn fetch_llms_txt_direct(
    client: &reqwest::Client,
    url: &str,
    ua_header: &HeaderValue,
    request: &FetchRequest,
) -> Result<FetchResponse, FetchError> {
    let response = client
        .get(url)
        .header(USER_AGENT, ua_header.clone())
        .send()
        .await
        .map_err(FetchError::from_reqwest)?;

    let status_code = response.status().as_u16();

    if !response.status().is_success() {
        return Ok(FetchResponse {
            url: request.url.clone(),
            status_code,
            error: Some(format!("HTTP {}", status_code)),
            ..Default::default()
        });
    }

    let body = response
        .text()
        .await
        .map_err(|e| FetchError::RequestError(e.to_string()))?;

    Ok(FetchResponse {
        url: request.url.clone(),
        status_code: 200,
        content_type: Some("text/plain".to_string()),
        format: Some("documentation".to_string()),
        content: Some(body),
        ..Default::default()
    })
}

/// Try to fetch an llms.txt URL. Returns Some(content) on success.
async fn try_fetch_llms_txt(
    client: &reqwest::Client,
    url: &str,
    ua_header: &HeaderValue,
) -> Option<String> {
    let response = client
        .get(url)
        .header(USER_AGENT, ua_header.clone())
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    // Reject HTML error pages masquerading as 200 OK
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if content_type.contains("text/html") {
        return None;
    }

    let body = response.bytes().await.ok()?;

    if body.len() > MAX_LLMS_TXT_SIZE {
        return None;
    }

    let text = String::from_utf8(body.to_vec()).ok()?;

    if text.trim().is_empty() {
        return None;
    }

    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_llms_txt_url() {
        let url = Url::parse("https://example.com/llms.txt").unwrap();
        assert!(DocsSiteFetcher::is_llms_txt_url(&url));

        let url = Url::parse("https://example.com/llms-full.txt").unwrap();
        assert!(DocsSiteFetcher::is_llms_txt_url(&url));

        let url = Url::parse("https://example.com/other.txt").unwrap();
        assert!(!DocsSiteFetcher::is_llms_txt_url(&url));
    }

    #[test]
    fn test_is_docs_site() {
        // ReadTheDocs
        let url = Url::parse("https://my-project.readthedocs.io/en/latest/").unwrap();
        assert!(DocsSiteFetcher::is_docs_site(&url));

        // docs.rs
        let url = Url::parse("https://docs.rs/tokio/latest/tokio/").unwrap();
        assert!(DocsSiteFetcher::is_docs_site(&url));

        // docs. prefix
        let url = Url::parse("https://docs.python.org/3/library/").unwrap();
        assert!(DocsSiteFetcher::is_docs_site(&url));

        // developer. prefix
        let url = Url::parse("https://developer.mozilla.org/en-US/docs/Web").unwrap();
        assert!(DocsSiteFetcher::is_docs_site(&url));

        // GitBook
        let url = Url::parse("https://my-project.gitbook.io/docs/").unwrap();
        assert!(DocsSiteFetcher::is_docs_site(&url));

        // Non-docs site
        let url = Url::parse("https://github.com/owner/repo").unwrap();
        assert!(!DocsSiteFetcher::is_docs_site(&url));

        let url = Url::parse("https://example.com/page").unwrap();
        assert!(!DocsSiteFetcher::is_docs_site(&url));
    }

    #[test]
    fn test_fetcher_matches() {
        let fetcher = DocsSiteFetcher::new();

        // llms.txt URLs match
        let url = Url::parse("https://example.com/llms.txt").unwrap();
        assert!(fetcher.matches(&url));

        // Docs sites match
        let url = Url::parse("https://docs.rs/tokio/latest/tokio/").unwrap();
        assert!(fetcher.matches(&url));

        // Non-docs sites don't match
        let url = Url::parse("https://github.com/owner/repo").unwrap();
        assert!(!fetcher.matches(&url));
    }
}
