//! Default HTTP fetcher
//!
//! Handles general HTTP/HTTPS URLs with HTML conversion support.
//! This is the fallback fetcher that handles any URL not matched by
//! specialized fetchers.

use crate::client::FetchOptions;
use crate::convert::{filter_excessive_newlines, html_to_markdown, html_to_text, is_html};
use crate::error::FetchError;
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse, HttpMethod};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_DISPOSITION, USER_AGENT};
use std::time::Duration;
use tracing::{error, warn};
use url::Url;

/// Binary content type prefixes
const BINARY_PREFIXES: &[&str] = &[
    "image/",
    "audio/",
    "video/",
    "application/octet-stream",
    "application/pdf",
    "application/zip",
    "application/gzip",
    "application/x-tar",
    "application/x-rar",
    "application/x-7z",
    "application/vnd.ms-",
    "application/vnd.openxmlformats",
    "font/",
];

/// First-byte timeout (connect + first response byte)
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(1);

/// Body timeout (total)
const BODY_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout message appended to truncated content
const TIMEOUT_MESSAGE: &str = "\n\n[..more content timed out...]";

/// Default HTTP fetcher
///
/// Handles all HTTP/HTTPS URLs with:
/// - GET and HEAD methods
/// - HTML to markdown/text conversion
/// - Binary content detection
/// - Timeout handling with partial content
pub struct DefaultFetcher;

impl DefaultFetcher {
    /// Create a new default fetcher
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for DefaultFetcher {
    fn name(&self) -> &'static str {
        "default"
    }

    fn matches(&self, _url: &Url) -> bool {
        // Default fetcher matches all URLs
        true
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        // Validate URL
        if request.url.is_empty() {
            return Err(FetchError::MissingUrl);
        }

        let method = request.effective_method();
        let wants_markdown = options.enable_markdown && request.wants_markdown();
        let wants_text = options.enable_text && request.wants_text();

        // Build headers
        let mut headers = HeaderMap::new();
        let user_agent = options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(user_agent)
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
        );

        // Set Accept header based on conversion mode
        let accept = if wants_markdown {
            "text/html, text/markdown, text/plain, */*;q=0.8"
        } else if wants_text {
            "text/html, text/plain, */*;q=0.8"
        } else {
            "*/*"
        };
        headers.insert(ACCEPT, HeaderValue::from_static(accept));

        // Build client
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(FIRST_BYTE_TIMEOUT)
            .timeout(FIRST_BYTE_TIMEOUT)
            .build()
            .map_err(FetchError::ClientBuildError)?;

        // Build request
        let reqwest_method = match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Head => reqwest::Method::HEAD,
        };

        let http_request = client.request(reqwest_method, &request.url);

        // Send request
        let response = http_request
            .send()
            .await
            .map_err(FetchError::from_reqwest)?;

        let status_code = response.status().as_u16();
        let resp_headers = response.headers().clone();

        // Extract metadata
        let content_type = resp_headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let last_modified = resp_headers
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let content_length: Option<u64> = resp_headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        let filename = extract_filename(&resp_headers, &request.url);

        // Handle HEAD request
        if method == HttpMethod::Head {
            return Ok(FetchResponse {
                url: request.url.clone(),
                status_code,
                content_type,
                size: content_length,
                last_modified,
                filename,
                method: Some("HEAD".to_string()),
                ..Default::default()
            });
        }

        // Check for binary content
        if let Some(ref ct) = content_type {
            if is_binary_content_type(ct) {
                return Ok(FetchResponse {
                    url: request.url.clone(),
                    status_code,
                    content_type,
                    size: content_length,
                    last_modified,
                    filename,
                    error: Some(
                        "Binary content is not supported. Only textual content (HTML, text, JSON, etc.) can be fetched."
                            .to_string(),
                    ),
                    ..Default::default()
                });
            }
        }

        // Read body with timeout
        let (body, truncated) = read_body_with_timeout(response, BODY_TIMEOUT).await;
        let size = body.len() as u64;

        // Convert to string
        let content = String::from_utf8_lossy(&body).to_string();

        // Determine format and convert if needed
        let (format, final_content) = if is_html(&content_type, &content) {
            if wants_markdown {
                ("markdown".to_string(), html_to_markdown(&content))
            } else if wants_text {
                ("text".to_string(), html_to_text(&content))
            } else {
                ("raw".to_string(), content)
            }
        } else {
            ("raw".to_string(), content)
        };

        // Apply newline filtering
        let mut final_content = filter_excessive_newlines(&final_content);

        // Add timeout message if truncated
        if truncated {
            final_content.push_str(TIMEOUT_MESSAGE);
        }

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code,
            content_type,
            size: Some(size),
            last_modified,
            filename,
            format: Some(format),
            content: Some(final_content),
            truncated: if truncated { Some(true) } else { None },
            ..Default::default()
        })
    }
}

/// Check if content type indicates binary content
fn is_binary_content_type(content_type: &str) -> bool {
    let ct_lower = content_type.to_lowercase();
    BINARY_PREFIXES
        .iter()
        .any(|prefix| ct_lower.starts_with(prefix))
}

/// Extract filename from Content-Disposition header or URL
fn extract_filename(headers: &HeaderMap, url: &str) -> Option<String> {
    // Try Content-Disposition header first
    if let Some(disposition) = headers.get(CONTENT_DISPOSITION) {
        if let Ok(value) = disposition.to_str() {
            if let Some(filename) = parse_content_disposition_filename(value) {
                return Some(filename);
            }
        }
    }

    // Fallback to URL path
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(mut segments) = parsed.path_segments() {
            if let Some(last) = segments.next_back() {
                if last.contains('.') && !last.is_empty() {
                    return Some(last.to_string());
                }
            }
        }
    }

    None
}

/// Parse filename from Content-Disposition header value
fn parse_content_disposition_filename(value: &str) -> Option<String> {
    let patterns = ["filename=\"", "filename="];
    for pattern in patterns {
        if let Some(start) = value.find(pattern) {
            let rest = &value[start + pattern.len()..];
            if pattern.ends_with('"') {
                // Quoted
                if let Some(end) = rest.find('"') {
                    return Some(rest[..end].to_string());
                }
            } else {
                // Unquoted - take until space or semicolon
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == ';')
                    .unwrap_or(rest.len());
                let filename = rest[..end].trim_matches('"');
                if !filename.is_empty() {
                    return Some(filename.to_string());
                }
            }
        }
    }
    None
}

/// Read response body with timeout, returning partial content if timeout occurs
async fn read_body_with_timeout(response: reqwest::Response, timeout: Duration) -> (Bytes, bool) {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let chunk_future = stream.next();
        let timeout_future = tokio::time::sleep_until(deadline);

        tokio::select! {
            chunk = chunk_future => {
                match chunk {
                    Some(Ok(bytes)) => {
                        body.extend_from_slice(&bytes);
                    }
                    Some(Err(e)) => {
                        error!("Error reading body chunk: {}", e);
                        let has_content = !body.is_empty();
                        return (Bytes::from(body), has_content);
                    }
                    None => {
                        // Stream complete
                        return (Bytes::from(body), false);
                    }
                }
            }
            _ = timeout_future => {
                warn!("Body timeout reached, returning partial content");
                return (Bytes::from(body), true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary_content_type() {
        assert!(is_binary_content_type("image/png"));
        assert!(is_binary_content_type("image/jpeg"));
        assert!(is_binary_content_type("audio/mp3"));
        assert!(is_binary_content_type("video/mp4"));
        assert!(is_binary_content_type("application/pdf"));
        assert!(is_binary_content_type("application/octet-stream"));
        assert!(is_binary_content_type("application/zip"));
        assert!(is_binary_content_type("application/vnd.ms-excel"));
        assert!(is_binary_content_type(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        ));
        assert!(is_binary_content_type("font/woff2"));

        assert!(!is_binary_content_type("text/html"));
        assert!(!is_binary_content_type("text/plain"));
        assert!(!is_binary_content_type("application/json"));
        assert!(!is_binary_content_type("application/javascript"));
    }

    #[test]
    fn test_parse_content_disposition_filename() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"file.pdf\""),
            Some("file.pdf".to_string())
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=file.pdf"),
            Some("file.pdf".to_string())
        );
        assert_eq!(
            parse_content_disposition_filename("inline; filename=\"report.xlsx\"; size=1234"),
            Some("report.xlsx".to_string())
        );
        assert_eq!(parse_content_disposition_filename("inline"), None);
    }

    #[test]
    fn test_extract_filename_from_url() {
        let headers = HeaderMap::new();
        assert_eq!(
            extract_filename(&headers, "https://example.com/path/to/file.pdf"),
            Some("file.pdf".to_string())
        );
        assert_eq!(
            extract_filename(&headers, "https://example.com/path/to/document"),
            None
        );
        assert_eq!(extract_filename(&headers, "https://example.com/"), None);
    }

    #[test]
    fn test_default_fetcher_matches_all() {
        let fetcher = DefaultFetcher::new();
        let url = Url::parse("https://example.com").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://github.com/owner/repo").unwrap();
        assert!(fetcher.matches(&url));
    }
}
