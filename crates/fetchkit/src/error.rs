//! Error types for WebFetch

use thiserror::Error;

/// Errors that can occur during fetch operations
#[derive(Debug, Error)]
pub enum FetchError {
    /// URL is missing
    #[error("Missing required parameter: url")]
    MissingUrl,

    /// URL has invalid scheme
    #[error("Invalid URL: must start with http:// or https://")]
    InvalidUrlScheme,

    /// Invalid HTTP method
    #[error("Invalid method: must be GET or HEAD")]
    InvalidMethod,

    /// URL is blocked by prefix list
    #[error("Blocked URL: prefix not allowed")]
    BlockedUrl,

    /// Failed to build HTTP client
    #[error("Failed to create HTTP client")]
    ClientBuildError(#[source] reqwest::Error),

    /// Request timed out waiting for first byte
    #[error("Request timed out: server did not respond within 1 second")]
    FirstByteTimeout,

    /// Failed to connect to server
    #[error("Failed to connect to server")]
    ConnectError(#[source] reqwest::Error),

    /// Other request error
    #[error("Request failed: {0}")]
    RequestError(String),

    /// Fetcher-specific error
    #[error("Fetcher error: {0}")]
    FetcherError(String),
}

impl FetchError {
    /// Create an error from a reqwest error
    pub fn from_reqwest(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            FetchError::FirstByteTimeout
        } else if err.is_connect() {
            FetchError::ConnectError(err)
        } else {
            FetchError::RequestError(err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
        assert_eq!(
            FetchError::MissingUrl.to_string(),
            "Missing required parameter: url"
        );
        assert_eq!(
            FetchError::InvalidUrlScheme.to_string(),
            "Invalid URL: must start with http:// or https://"
        );
        assert_eq!(
            FetchError::InvalidMethod.to_string(),
            "Invalid method: must be GET or HEAD"
        );
        assert_eq!(
            FetchError::BlockedUrl.to_string(),
            "Blocked URL: prefix not allowed"
        );
        assert_eq!(
            FetchError::FirstByteTimeout.to_string(),
            "Request timed out: server did not respond within 1 second"
        );
    }
}
