//! Core types for WebFetch

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// HTTP method for the request
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// HTTP GET request
    #[default]
    Get,
    /// HTTP HEAD request
    Head,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WebFetchRequest {
    /// The URL to fetch (required, must be http:// or https://)
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
}

impl WebFetchRequest {
    /// Create a new request with the given URL
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
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
}

/// Response from a fetch operation
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WebFetchResponse {
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
        let req = WebFetchRequest::new("https://example.com")
            .method(HttpMethod::Head)
            .as_markdown();

        assert_eq!(req.url, "https://example.com");
        assert_eq!(req.method, Some(HttpMethod::Head));
        assert_eq!(req.as_markdown, Some(true));
    }

    #[test]
    fn test_request_effective_method() {
        let req = WebFetchRequest::new("https://example.com");
        assert_eq!(req.effective_method(), HttpMethod::Get);

        let req = req.method(HttpMethod::Head);
        assert_eq!(req.effective_method(), HttpMethod::Head);
    }

    #[test]
    fn test_request_serialization() {
        let req = WebFetchRequest::new("https://example.com").as_markdown();
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"url\":\"https://example.com\""));
        assert!(json.contains("\"as_markdown\":true"));
    }

    #[test]
    fn test_response_serialization() {
        let resp = WebFetchResponse {
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
