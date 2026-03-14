// Decisions:
// - Ignore ambient proxy env by default. Shared agent runtimes should not inherit network routing.
// - Cap decompressed textual bodies. LLM-oriented fetches do not need unbounded response growth.
//! Default HTTP fetcher
//!
//! Handles general HTTP/HTTPS URLs with HTML conversion support.
//! This is the fallback fetcher that handles any URL not matched by
//! specialized fetchers.

use crate::client::FetchOptions;
use crate::convert::{filter_excessive_newlines, html_to_markdown, html_to_text, is_html};
use crate::error::FetchError;
use crate::fetchers::Fetcher;
use crate::file_saver::FileSaver;
use crate::types::{FetchRequest, FetchResponse, HttpMethod};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_DISPOSITION, LOCATION, USER_AGENT};
use std::time::Duration;
use tracing::{debug, error, warn};
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

// THREAT[TM-DOS-002]: First-byte timeout prevents slowloris / slow-start attacks
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(1);

// THREAT[TM-DOS-002]: Body timeout caps total request duration
const BODY_TIMEOUT: Duration = Duration::from_secs(30);

/// Truncation message appended when body is cut short (timeout or size limit)
const TRUNCATION_MESSAGE: &str = "\n\n[..content truncated...]";

// THREAT[TM-SSRF-010]: Maximum redirects to follow with IP validation at each hop
const MAX_REDIRECTS: usize = 10;

// THREAT[TM-DOS-001]: Default max body size (10 MB) to prevent memory exhaustion
// THREAT[TM-DOS-003]: Also protects against compressed content bombs (gzip bombs)
const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

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

/// Build headers for HTTP requests
fn build_headers(options: &FetchOptions, accept: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let user_agent = options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(user_agent)
            .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_str(accept).unwrap_or_else(|_| HeaderValue::from_static("*/*")),
    );
    headers
}

/// Extract common response metadata from headers
struct ResponseMeta {
    content_type: Option<String>,
    last_modified: Option<String>,
    content_length: Option<u64>,
    filename: Option<String>,
}

fn extract_response_meta(headers: &HeaderMap, url: &str) -> ResponseMeta {
    ResponseMeta {
        content_type: headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
        last_modified: headers
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
        content_length: headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok()),
        filename: extract_filename(headers, url),
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
        if request.url.is_empty() {
            return Err(FetchError::MissingUrl);
        }

        let method = request.effective_method();
        let wants_markdown = options.enable_markdown && request.wants_markdown();
        let wants_text = options.enable_text && request.wants_text();
        let max_body_size = options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE);

        let accept = if wants_markdown {
            "text/html, text/markdown, text/plain, */*;q=0.8"
        } else if wants_text {
            "text/html, text/plain, */*;q=0.8"
        } else {
            "*/*"
        };

        let headers = build_headers(options, accept);
        let parsed_url = url::Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let reqwest_method = match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Head => reqwest::Method::HEAD,
        };

        // THREAT[TM-SSRF-010]: Follow redirects manually so every hop is re-validated.
        let response =
            send_request_following_redirects(parsed_url, reqwest_method, headers, options).await?;

        let status_code = response.status().as_u16();
        let final_url = response.url().to_string();
        let meta = extract_response_meta(response.headers(), &final_url);

        // Handle HEAD request
        if method == HttpMethod::Head {
            return Ok(FetchResponse {
                url: final_url,
                status_code,
                content_type: meta.content_type,
                size: meta.content_length,
                last_modified: meta.last_modified,
                filename: meta.filename,
                method: Some("HEAD".to_string()),
                ..Default::default()
            });
        }

        // Check for binary content
        if let Some(ref ct) = meta.content_type {
            if is_binary_content_type(ct) {
                return Ok(FetchResponse {
                    url: final_url,
                    status_code,
                    content_type: meta.content_type,
                    size: meta.content_length,
                    last_modified: meta.last_modified,
                    filename: meta.filename,
                    error: Some(
                        "Binary content is not supported. Only textual content (HTML, text, JSON, etc.) can be fetched."
                            .to_string(),
                    ),
                    ..Default::default()
                });
            }
        }

        // THREAT[TM-DOS-001]: Read body with timeout and size limit
        // THREAT[TM-DOS-003]: Size limit also protects against compressed content bombs
        let (body, truncated) = read_body_with_timeout(response, BODY_TIMEOUT, max_body_size).await;
        let size = body.len() as u64;

        // Convert to string
        let content = String::from_utf8_lossy(&body).to_string();

        // Determine format and convert if needed
        // THREAT[TM-DOS-006]: Conversion input is bounded by max_body_size
        let (format, final_content) = if is_html(&meta.content_type, &content) {
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

        // Add truncation messages
        if truncated {
            final_content.push_str(TRUNCATION_MESSAGE);
        }

        Ok(FetchResponse {
            url: final_url,
            status_code,
            content_type: meta.content_type,
            size: Some(size),
            last_modified: meta.last_modified,
            filename: meta.filename,
            format: Some(format),
            content: Some(final_content),
            truncated: if truncated { Some(true) } else { None },
            ..Default::default()
        })
    }

    /// Fetch and save to file — binary-aware override.
    ///
    /// Unlike `fetch()`, this does NOT reject binary content. Downloads raw bytes
    /// and saves them through the provided [`FileSaver`].
    async fn fetch_to_file(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
        saver: &dyn FileSaver,
    ) -> Result<FetchResponse, FetchError> {
        let save_path = match &request.save_to_file {
            Some(path) => path.clone(),
            None => return self.fetch(request, options).await,
        };

        if request.url.is_empty() {
            return Err(FetchError::MissingUrl);
        }

        let method = request.effective_method();
        let max_body_size = options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE);

        let headers = build_headers(options, "*/*");
        let parsed_url = url::Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let reqwest_method = match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Head => reqwest::Method::HEAD,
        };

        // THREAT[TM-SSRF-010]: Follow redirects manually with IP validation at each hop
        let response =
            send_request_following_redirects(parsed_url, reqwest_method, headers, options).await?;

        let status_code = response.status().as_u16();
        let final_url = response.url().to_string();
        let meta = extract_response_meta(response.headers(), &final_url);

        // HEAD request — return metadata only
        if method == HttpMethod::Head {
            return Ok(FetchResponse {
                url: final_url,
                status_code,
                content_type: meta.content_type,
                size: meta.content_length,
                last_modified: meta.last_modified,
                filename: meta.filename,
                method: Some("HEAD".to_string()),
                ..Default::default()
            });
        }

        // Read raw body (no binary rejection for file saves)
        let (body, truncated) = read_body_with_timeout(response, BODY_TIMEOUT, max_body_size).await;
        let size = body.len() as u64;

        // Save through the FileSaver
        let save_result = saver
            .save(&save_path, &body)
            .await
            .map_err(|e| FetchError::SaveError(e.to_string()))?;

        Ok(FetchResponse {
            url: final_url,
            status_code,
            content_type: meta.content_type,
            size: Some(size),
            last_modified: meta.last_modified,
            filename: meta.filename,
            truncated: if truncated { Some(true) } else { None },
            saved_path: Some(save_result.path),
            bytes_written: Some(save_result.bytes_written),
            // No inline content when saving to file
            ..Default::default()
        })
    }
}

async fn send_request_following_redirects(
    initial_url: Url,
    method: reqwest::Method,
    headers: HeaderMap,
    options: &FetchOptions,
) -> Result<reqwest::Response, FetchError> {
    let mut current_url = initial_url;

    for redirect_count in 0..=MAX_REDIRECTS {
        let client = build_client_for_url(&current_url, headers.clone(), options)?;
        let response = client
            .request(method.clone(), current_url.clone())
            .send()
            .await
            .map_err(FetchError::from_reqwest)?;

        let Some(next_url) = redirect_target(&current_url, &response, options)? else {
            return Ok(response);
        };

        if redirect_count == MAX_REDIRECTS {
            return Err(FetchError::RequestError("too many redirects".to_string()));
        }

        debug!(
            from = %current_url,
            to = %next_url,
            hop = redirect_count + 1,
            "Following redirect with IP validation"
        );

        current_url = next_url;
    }

    unreachable!("redirect loop must return before exhausting iterations");
}

fn build_client_for_url(
    url: &Url,
    headers: HeaderMap,
    options: &FetchOptions,
) -> Result<reqwest::Client, FetchError> {
    // THREAT[TM-NET-003]: New client per request prevents connection-pool state leakage
    let mut client_builder = reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(FIRST_BYTE_TIMEOUT)
        .timeout(FIRST_BYTE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none());

    if !options.respect_proxy_env {
        // THREAT[TM-NET-004]: Ignore ambient proxy env by default in shared runtimes.
        client_builder = client_builder.no_proxy();
    }

    if options.dns_policy.block_private {
        if let Some(host) = url.host_str() {
            let port = url.port_or_known_default().unwrap_or(80);
            let validated_addr = options
                .dns_policy
                .resolve_and_validate(host, port)
                .map_err(|_| FetchError::BlockedUrl)?;
            // THREAT[TM-SSRF-001]: Resolve-then-check — validate resolved IP before connecting.
            // THREAT[TM-SSRF-005]: Pin DNS resolution to prevent DNS rebinding attacks.
            client_builder = client_builder.resolve(host, validated_addr);
        }
    }

    client_builder.build().map_err(FetchError::ClientBuildError)
}

fn redirect_target(
    base_url: &Url,
    response: &reqwest::Response,
    options: &FetchOptions,
) -> Result<Option<Url>, FetchError> {
    if !response.status().is_redirection() {
        return Ok(None);
    }

    let location = response
        .headers()
        .get(LOCATION)
        .ok_or_else(|| {
            FetchError::RequestError("redirect response missing Location header".to_string())
        })?
        .to_str()
        .map_err(|_| {
            FetchError::RequestError("redirect Location header is not valid UTF-8".to_string())
        })?;

    let next_url = base_url.join(location).map_err(|_| {
        FetchError::RequestError("redirect Location is not a valid URL".to_string())
    })?;

    // THREAT[TM-INPUT-001]: Validate scheme at each redirect hop
    if next_url.scheme() != "http" && next_url.scheme() != "https" {
        return Err(FetchError::InvalidUrlScheme);
    }

    options.validate_redirect_target(base_url, &next_url)?;

    Ok(Some(next_url))
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

/// Read response body with timeout and size limit, returning partial content if either is hit.
///
/// Returns `(body_bytes, truncated)`. `truncated` is true if the body was cut short
/// due to timeout or exceeding `max_size`.
// THREAT[TM-DOS-001]: Configurable max body size prevents unbounded memory usage
// THREAT[TM-DOS-003]: Decompressed size is checked, catching gzip/brotli bombs
async fn read_body_with_timeout(
    response: reqwest::Response,
    timeout: Duration,
    max_size: usize,
) -> (Bytes, bool) {
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
                        let remaining = max_size.saturating_sub(body.len());
                        if remaining == 0 {
                            warn!("Body size limit reached ({}), truncating", max_size);
                            return (Bytes::from(body), true);
                        }
                        if bytes.len() > remaining {
                            body.extend_from_slice(&bytes[..remaining]);
                            warn!("Body size limit reached ({}), truncating", max_size);
                            return (Bytes::from(body), true);
                        }
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
    use crate::dns::DnsPolicy;
    use crate::types::FetchRequest;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    #[tokio::test]
    async fn test_manual_redirect_following() {
        let destination = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("redirected")
                    .insert_header("content-type", "text/plain"),
            )
            .mount(&destination)
            .await;

        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/final", destination.uri())),
            )
            .mount(&origin)
            .await;

        let fetcher = DefaultFetcher::new();
        let options = FetchOptions {
            enable_markdown: true,
            enable_text: true,
            dns_policy: DnsPolicy::allow_all(),
            ..Default::default()
        };
        let request = FetchRequest::new(format!("{}/start", origin.uri())).as_markdown();
        let response = fetcher.fetch(&request, &options).await.unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content.as_deref(), Some("redirected"));
    }

    #[tokio::test]
    async fn test_redirect_target_handles_relative_location() {
        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/final"))
            .mount(&origin)
            .await;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base_url = Url::parse(&format!("{}/start", origin.uri())).unwrap();
        let response = client.get(base_url.clone()).send().await.unwrap();

        let redirect = redirect_target(&base_url, &response, &FetchOptions::default()).unwrap();
        assert_eq!(
            redirect.unwrap(),
            Url::parse(&format!("{}/final", origin.uri())).unwrap()
        );
    }

    #[tokio::test]
    async fn test_redirect_target_rejects_non_http_location() {
        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "file:///etc/passwd"),
            )
            .mount(&origin)
            .await;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base_url = Url::parse(&format!("{}/start", origin.uri())).unwrap();
        let response = client.get(base_url.clone()).send().await.unwrap();

        let redirect = redirect_target(&base_url, &response, &FetchOptions::default());
        assert!(matches!(redirect, Err(FetchError::InvalidUrlScheme)));
    }
}
