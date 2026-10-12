// Decisions:
// - Remote converters do not use `HttpTransport`: that trait is deliberately a
//   single-hop GET/HEAD adapter for user URLs. Uploads go through `ConverterClient`,
//   so a host can route them through its own egress boundary.
// - The converter endpoint is operator configuration, not user input, so it is not
//   subject to request allow/block lists. It is never derived from the fetched URL.
// - Multipart bodies are encoded here so any `ConverterClient` only has to send bytes.
//! Shared plumbing for remote document converters.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use url::Url;

use super::ConvertError;

/// Default timeout for one remote conversion.
pub(crate) const DEFAULT_REMOTE_TIMEOUT: Duration = Duration::from_secs(60);

/// Default cap on a remote converter's response body (32 MB).
pub(crate) const MAX_REMOTE_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// A credential that never appears in `Debug` or `Display` output.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Reveal the secret for use in a request header.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// One POST to a remote converter.
pub struct ConverterRequest {
    /// Converter endpoint.
    pub url: Url,
    /// Request headers, including `Content-Type` and any credentials.
    pub headers: Vec<(String, String)>,
    /// Encoded request body.
    pub body: Bytes,
    /// Overall request timeout.
    pub timeout: Duration,
    /// Maximum response body size the client should read.
    pub max_response_bytes: usize,
}

impl fmt::Debug for ConverterRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConverterRequest")
            .field("url", &self.url.as_str())
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("body_len", &self.body.len())
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Response from a remote converter.
#[derive(Debug, Clone)]
pub struct ConverterResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response Content-Type, when present.
    pub content_type: Option<String>,
    /// Response body, at most `max_response_bytes`.
    pub body: Bytes,
}

/// Sends converter uploads. Implement to route them through a host egress boundary.
#[async_trait]
pub trait ConverterClient: Send + Sync {
    /// Send one POST request and return the bounded response.
    async fn post(&self, request: ConverterRequest) -> Result<ConverterResponse, ConvertError>;
}

/// Default [`ConverterClient`] using reqwest. Redirects are not followed and
/// ambient proxy variables are ignored unless enabled.
#[derive(Debug, Clone, Default)]
pub struct ReqwestConverterClient {
    respect_proxy_env: bool,
}

impl ReqwestConverterClient {
    /// Create a client that ignores `HTTP(S)_PROXY`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Honour `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` for converter uploads.
    pub fn respect_proxy_env(mut self, respect: bool) -> Self {
        self.respect_proxy_env = respect;
        self
    }
}

#[async_trait]
impl ConverterClient for ReqwestConverterClient {
    async fn post(&self, request: ConverterRequest) -> Result<ConverterResponse, ConvertError> {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(request.timeout);
        if !self.respect_proxy_env {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|error| ConvertError::Failed(format!("converter client: {error}")))?;
        let mut post = client.post(request.url).body(request.body);
        for (name, value) in &request.headers {
            post = post.header(name, value);
        }
        let mut response = post
            .send()
            .await
            .map_err(|error| ConvertError::Failed(format!("converter request failed: {error}")))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut body = BytesMut::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| ConvertError::Failed(format!("converter response: {error}")))?
        {
            if body.len() + chunk.len() > request.max_response_bytes {
                return Err(ConvertError::Failed(format!(
                    "converter response exceeds {} bytes",
                    request.max_response_bytes
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(ConverterResponse {
            status,
            content_type,
            body: body.freeze(),
        })
    }
}

/// Encode a single-file `multipart/form-data` body. Returns `(content_type, body)`.
pub(crate) fn multipart_file(
    field: &str,
    file_name: &str,
    media_type: &str,
    data: &[u8],
) -> (String, Bytes) {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mut boundary = format!(
        "fetchkit-{nanos:x}-{:x}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    // A boundary must not occur in the payload.
    while data
        .windows(boundary.len())
        .any(|window| window == boundary.as_bytes())
    {
        boundary.push('x');
    }

    let quote = |value: &str| value.replace(['"', '\r', '\n'], "_");
    let mut body = BytesMut::with_capacity(data.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {media_type}\r\n\r\n",
            quote(field),
            quote(file_name)
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (
        format!("multipart/form-data; boundary={boundary}"),
        body.freeze(),
    )
}

/// Map an HTTP status to a converter error, or `None` on success.
pub(crate) fn status_error(name: &str, status: u16, body: &[u8]) -> Option<ConvertError> {
    if (200..300).contains(&status) {
        return None;
    }
    let detail: String = String::from_utf8_lossy(body).chars().take(300).collect();
    Some(match status {
        401 | 403 => ConvertError::Failed(format!("{name}: authentication failed ({status})")),
        413 => ConvertError::Unsupported(format!("{name}: document too large ({status})")),
        415 => ConvertError::Unsupported(format!("{name}: format not accepted ({status})")),
        _ => ConvertError::Failed(format!("{name}: HTTP {status}: {}", detail.trim())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_is_redacted() {
        let secret = SecretString::new("token-123");
        assert_eq!(format!("{secret:?}"), "SecretString(<redacted>)");
        assert_eq!(secret.expose(), "token-123");
    }

    #[test]
    fn multipart_wraps_file_with_unique_boundary() {
        let (content_type, body) = multipart_file("files", "a\"b.pdf", "application/pdf", b"%PDF");
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.starts_with(&format!("--{boundary}\r\n")));
        assert!(text.contains("name=\"files\"; filename=\"a_b.pdf\""));
        assert!(text.contains("Content-Type: application/pdf\r\n\r\n%PDF\r\n"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn status_errors_are_classified() {
        assert!(status_error("x", 200, b"").is_none());
        assert!(matches!(
            status_error("x", 401, b""),
            Some(ConvertError::Failed(message)) if message.contains("authentication")
        ));
        assert!(matches!(
            status_error("x", 413, b""),
            Some(ConvertError::Unsupported(_))
        ));
    }
}
