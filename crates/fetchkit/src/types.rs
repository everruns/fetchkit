//! Core types for Fetchkit

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use url::Url;

use crate::error::FetchError;

/// HTTP method for the request
///
/// # Examples
///
/// ```
/// use fetchkit::HttpMethod;
///
/// let method: HttpMethod = "HEAD".parse().unwrap();
/// assert_eq!(method, HttpMethod::Head);
/// assert_eq!(method.to_string(), "HEAD");
///
/// assert_eq!(HttpMethod::default(), HttpMethod::Get);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// HTTP GET request
    #[default]
    Get,
    /// HTTP HEAD request
    Head,
}

/// Optional browser-rendering backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RenderMode {
    /// Lightweight rakers-based JavaScript/DOM rendering.
    Rakers,
}

impl FromStr for HttpMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "GET" => Ok(HttpMethod::Get),
            "HEAD" => Ok(HttpMethod::Head),
            _ => Err("Invalid method: must be GET or HEAD".to_string()),
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Head => write!(f, "HEAD"),
        }
    }
}

/// Request to fetch a URL
///
/// # Examples
///
/// ```
/// use fetchkit::{FetchRequest, HttpMethod};
///
/// // Simple GET request
/// let req = FetchRequest::new("https://example.com");
/// assert_eq!(req.effective_method(), HttpMethod::Get);
///
/// // Request with markdown conversion
/// let req = FetchRequest::new("https://example.com").as_markdown();
/// assert!(req.wants_markdown());
///
/// // HEAD request
/// let req = FetchRequest::new("https://example.com")
///     .method(HttpMethod::Head);
/// assert_eq!(req.effective_method(), HttpMethod::Head);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FetchRequest {
    /// The URL to fetch. HTTP/HTTPS URLs are accepted as-is; bare domain URLs
    /// such as `example.com/docs` are normalized to `https://example.com/docs`.
    pub url: String,

    /// HTTP method (optional, default GET)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,

    /// Convert HTML to markdown (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_markdown: Option<bool>,

    /// Convert HTML to plain text (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_text: Option<bool>,

    /// Save response body to this path instead of returning content inline.
    /// Requires a `FileSaver` to be provided at execution time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_to_file: Option<String>,

    /// Content extraction focus:
    /// - "full" (default) returns everything
    /// - "main" strips semantic boilerplate
    /// - "readable" selects the densest article-like content block
    /// - "agent" selects the best low-noise content strategy for AI agents
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_focus: Option<String>,

    /// ETag value for conditional requests (If-None-Match header).
    /// When set, the server may return 304 Not Modified if content unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_none_match: Option<String>,

    /// Last-Modified value for conditional requests (If-Modified-Since header).
    /// When set, the server may return 304 Not Modified if content unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_modified_since: Option<String>,

    /// Discover same-origin links after fetching the seed URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crawl: Option<bool>,

    /// Maximum pages to fetch when crawl discovery is enabled, including the seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<usize>,

    /// Optional browser rendering backend. Disabled unless the tool/options
    /// explicitly enable the requested backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<RenderMode>,
}

impl FetchRequest {
    /// Create a new request with the given URL
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    /// Normalize browser-like bare host URLs before validation and fetching.
    pub(crate) fn normalize_url_for_fetch(&mut self) -> Result<(), FetchError> {
        self.url = canonical_fetch_url(&self.url)?;
        Ok(())
    }

    /// Return a normalized copy for APIs that accept `&FetchRequest`.
    pub(crate) fn normalized_for_fetch(&self) -> Result<Self, FetchError> {
        let mut request = self.clone();
        request.normalize_url_for_fetch()?;
        Ok(request)
    }

    /// Set the HTTP method
    pub fn method(mut self, method: HttpMethod) -> Self {
        self.method = Some(method);
        self
    }

    /// Enable markdown conversion
    pub fn as_markdown(mut self) -> Self {
        self.as_markdown = Some(true);
        self
    }

    /// Enable text conversion
    pub fn as_text(mut self) -> Self {
        self.as_text = Some(true);
        self
    }

    /// Set save-to-file path
    pub fn save_to_file(mut self, path: impl Into<String>) -> Self {
        self.save_to_file = Some(path.into());
        self
    }

    /// Set content focus mode ("full", "main", "readable", or "agent")
    pub fn content_focus(mut self, focus: impl Into<String>) -> Self {
        self.content_focus = Some(focus.into());
        self
    }

    /// Set ETag for conditional request
    pub fn if_none_match(mut self, etag: impl Into<String>) -> Self {
        self.if_none_match = Some(etag.into());
        self
    }

    /// Set If-Modified-Since for conditional request
    pub fn if_modified_since(mut self, date: impl Into<String>) -> Self {
        self.if_modified_since = Some(date.into());
        self
    }

    /// Enable bounded same-origin crawl discovery.
    pub fn crawl(mut self, enable: bool) -> Self {
        self.crawl = Some(enable);
        self
    }

    /// Set max pages for crawl discovery, including the seed page.
    pub fn max_pages(mut self, max_pages: usize) -> Self {
        self.max_pages = Some(max_pages);
        self
    }

    /// Render HTML through the rakers backend before conversion.
    pub fn render_rakers(mut self) -> Self {
        self.render = Some(RenderMode::Rakers);
        self
    }

    /// Get the effective method (default to GET)
    pub fn effective_method(&self) -> HttpMethod {
        self.method.unwrap_or_default()
    }

    /// Check if markdown conversion is requested
    pub fn wants_markdown(&self) -> bool {
        self.as_markdown.unwrap_or(false)
    }

    /// Check if text conversion is requested
    pub fn wants_text(&self) -> bool {
        self.as_text.unwrap_or(false)
    }

    /// Check if main-content focus is requested
    pub fn wants_main_content(&self) -> bool {
        self.content_focus
            .as_deref()
            .map(|f| f.eq_ignore_ascii_case("main"))
            .unwrap_or(false)
    }

    /// Check if readable content focus is requested.
    pub fn wants_readable_content(&self) -> bool {
        self.content_focus
            .as_deref()
            .map(|f| f.eq_ignore_ascii_case("readable"))
            .unwrap_or(false)
    }

    /// Check if agent content focus is requested.
    pub fn wants_agent_content(&self) -> bool {
        self.content_focus
            .as_deref()
            .map(|f| f.eq_ignore_ascii_case("agent"))
            .unwrap_or(false)
    }

    /// Check if bounded crawl discovery is requested.
    pub fn wants_crawl(&self) -> bool {
        self.crawl.unwrap_or(false)
    }

    /// Check if rakers rendering is requested.
    pub fn wants_rakers_render(&self) -> bool {
        self.render == Some(RenderMode::Rakers)
    }
}

fn canonical_fetch_url(raw_url: &str) -> Result<String, FetchError> {
    if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
        return Ok(raw_url.to_string());
    }

    if raw_url.is_empty()
        || raw_url.starts_with("//")
        || raw_url.starts_with('/')
        || raw_url.contains("://")
        || raw_url.contains('\\')
        || raw_url.chars().any(char::is_whitespace)
        || raw_url.chars().any(char::is_control)
    {
        return Err(FetchError::InvalidUrlScheme);
    }

    let candidate = format!("https://{raw_url}");
    let parsed = Url::parse(&candidate).map_err(|_| FetchError::InvalidUrlScheme)?;
    let host = parsed.host_str().ok_or(FetchError::InvalidUrlScheme)?;

    if !is_domain_like_bare_host(host)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(FetchError::InvalidUrlScheme);
    }

    Ok(candidate)
}

fn is_domain_like_bare_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host.contains('.')
        && host.parse::<IpAddr>().is_err()
        && host.chars().any(|ch| ch.is_ascii_alphabetic())
}

/// A link extracted from the page with its text and href.
///
/// # Examples
///
/// ```
/// use fetchkit::PageLink;
///
/// let link = PageLink {
///     text: "Example".to_string(),
///     href: "https://example.com".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PageLink {
    /// Link text
    pub text: String,
    /// Link href
    pub href: String,
}

/// An agent-oriented resource advertised by a site or found at a conventional path.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AgentResource {
    /// Absolute resource URL.
    pub url: String,
    /// Stable resource kind, such as `llms-txt`, `auth`, or `mcp`.
    pub kind: String,
    /// Discovery source: `http-link`, `html-link`, `metadata`, or `probe`.
    pub source: String,
    /// Link relation when the site advertised one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    /// Advertised or returned media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Human-readable title when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether FetchKit confirmed the resource with a request.
    pub verified: bool,
}

/// Structured metadata extracted from an HTML page.
///
/// All fields are optional — only populated when the corresponding
/// HTML elements or meta tags are present.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PageMetadata {
    /// Page title from `<title>` or `og:title`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Page description from `<meta name="description">` or `og:description`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Language from `<html lang="...">`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Canonical URL from `<link rel="canonical">`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,

    /// Author from `<meta name="author">`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// Published date from `<meta property="article:published_time">` or `<time>`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,

    /// Modified date from `<meta property="article:modified_time">`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_date: Option<String>,

    /// Links extracted from the page
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub links: Vec<PageLink>,

    /// Agent-oriented resources advertised by or discovered for the site.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub agent_resources: Vec<AgentResource>,

    /// Headings outline (e.g. `["# Title", "## Section 1", "## Section 2"]`)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub headings: Vec<String>,

    /// Content extraction method used for the returned content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_method: Option<String>,
}

impl PageMetadata {
    /// Returns true if all fields are empty/None.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.language.is_none()
            && self.canonical_url.is_none()
            && self.author.is_none()
            && self.published_date.is_none()
            && self.modified_date.is_none()
            && self.links.is_empty()
            && self.agent_resources.is_empty()
            && self.headings.is_empty()
            && self.extraction_method.is_none()
    }
}

/// Agent-facing content quality signals.
///
/// These are heuristic hints for tool callers. They are intended to help agents
/// decide whether to trust, retry, narrow, or escalate a fetch result.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct PageQuality {
    /// Normalized quality score from 0.0 (poor) to 1.0 (good).
    pub score: f32,

    /// Machine-readable warning labels, such as `low_content` or `truncated`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,

    /// Approximate markdown link count divided by word count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_density: Option<f32>,

    /// Content extraction method used for the returned content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_method: Option<String>,

    /// Suggested next action for agents when warnings indicate a poor result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_next_action: Option<String>,
}

/// Summary for one page visited by bounded crawl discovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CrawlPage {
    /// Final page URL.
    pub url: String,

    /// HTTP status code, when the page was fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,

    /// Page title from metadata, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Page description from metadata, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Content-Type header, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    /// Word count of returned content, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u64>,

    /// Agent quality score, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f32>,

    /// Error for this crawl page, when fetching failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Bounded same-origin crawl discovery summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CrawlResult {
    /// Seed URL requested by the caller.
    pub seed_url: String,

    /// Maximum page budget used for this crawl, including the seed.
    pub max_pages: usize,

    /// Pages visited or attempted, in discovery order.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub pages: Vec<CrawlPage>,

    /// True when more same-origin candidates existed than the page budget allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

/// Response from a fetch operation
///
/// Contains the fetched content along with metadata like status code,
/// content type, and size. Optional fields are omitted when not applicable.
///
/// # Examples
///
/// ```
/// use fetchkit::FetchResponse;
///
/// let response = FetchResponse {
///     url: "https://example.com".to_string(),
///     status_code: 200,
///     content_type: Some("text/html".to_string()),
///     format: Some("markdown".to_string()),
///     content: Some("# Example Domain".to_string()),
///     ..Default::default()
/// };
///
/// assert_eq!(response.status_code, 200);
/// assert!(response.content.unwrap().contains("Example"));
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FetchResponse {
    /// The fetched URL
    pub url: String,

    /// HTTP status code
    pub status_code: u16,

    /// Content-Type header value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    /// Content size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Last-Modified header value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,

    /// ETag header value (for conditional requests)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Extracted filename
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Content format: "markdown", "text", or "raw"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// The fetched/converted content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// True if content was truncated due to timeout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,

    /// "HEAD" for HEAD requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// Error message (for binary content)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Path where file was saved (when save_to_file was used)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,

    /// Bytes written to file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_written: Option<u64>,

    /// Structured page metadata extracted from HTML
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PageMetadata>,

    /// Agent-facing content quality signals
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<PageQuality>,

    /// Bounded same-origin crawl discovery result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crawl: Option<CrawlResult>,

    /// Word count of the final content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u64>,

    /// Chain of URLs followed during redirects (empty if no redirects)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub redirect_chain: Vec<String>,

    /// Heuristic paywall detection (soft signal, not guaranteed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_paywall: Option<bool>,

    /// Rendering backend used before conversion, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_by: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_from_str() {
        assert_eq!(HttpMethod::from_str("GET").unwrap(), HttpMethod::Get);
        assert_eq!(HttpMethod::from_str("get").unwrap(), HttpMethod::Get);
        assert_eq!(HttpMethod::from_str("Get").unwrap(), HttpMethod::Get);
        assert_eq!(HttpMethod::from_str("HEAD").unwrap(), HttpMethod::Head);
        assert_eq!(HttpMethod::from_str("head").unwrap(), HttpMethod::Head);
        assert!(HttpMethod::from_str("POST").is_err());
        assert!(HttpMethod::from_str("invalid").is_err());
    }

    #[test]
    fn test_http_method_display() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Head.to_string(), "HEAD");
    }

    #[test]
    fn test_request_builder() {
        let req = FetchRequest::new("https://example.com")
            .method(HttpMethod::Head)
            .as_markdown();

        assert_eq!(req.url, "https://example.com");
        assert_eq!(req.method, Some(HttpMethod::Head));
        assert_eq!(req.as_markdown, Some(true));
    }

    #[test]
    fn test_request_effective_method() {
        let req = FetchRequest::new("https://example.com");
        assert_eq!(req.effective_method(), HttpMethod::Get);

        let req = req.method(HttpMethod::Head);
        assert_eq!(req.effective_method(), HttpMethod::Head);
    }

    #[test]
    fn test_request_serialization() {
        let req = FetchRequest::new("https://example.com").as_markdown();
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"url\":\"https://example.com\""));
        assert!(json.contains("\"as_markdown\":true"));
    }

    #[test]
    fn test_response_serialization() {
        let resp = FetchResponse {
            url: "https://example.com".to_string(),
            status_code: 200,
            content: Some("Hello".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&resp).unwrap();
        // Optional None fields should be omitted
        assert!(!json.contains("content_type"));
        assert!(json.contains("\"content\":\"Hello\""));
    }
}
