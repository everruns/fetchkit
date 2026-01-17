//! FetchKit - AI-friendly web content fetching library
//!
//! This crate provides a reusable library API for fetching web content,
//! with optional HTML to markdown/text conversion.
//!
//! ## Fetcher System
//!
//! FetchKit uses a pluggable fetcher system where specialized fetchers
//! handle specific URL patterns. The [`FetcherRegistry`] dispatches
//! requests to the appropriate fetcher based on URL matching.
//!
//! Built-in fetchers:
//! - [`DefaultFetcher`] - General HTTP/HTTPS fetcher with HTML conversion
//! - [`GitHubRepoFetcher`] - GitHub repository metadata and README

pub mod client;
mod convert;
mod error;
pub mod fetchers;
mod tool;
mod types;

pub use client::{fetch, fetch_with_options, FetchOptions};
pub use convert::{html_to_markdown, html_to_text};
pub use error::FetchError;
pub use fetchers::{DefaultFetcher, Fetcher, FetcherRegistry, GitHubRepoFetcher};
pub use tool::{Tool, ToolBuilder, ToolStatus};
pub use types::{FetchRequest, FetchResponse, HttpMethod};

/// Default User-Agent string
pub const DEFAULT_USER_AGENT: &str = "Everruns FetchKit/1.0";

/// Tool description for LLM consumption
pub const TOOL_DESCRIPTION: &str = r#"Fetches content from a URL and optionally converts HTML to markdown or text.

- Supports GET and HEAD methods
- Converts HTML to markdown or plain text
- Returns metadata for binary content
- Strict timeouts for reliability"#;

/// Extended documentation for LLM consumption (llmtxt)
pub const TOOL_LLMTXT: &str = r#"# FetchKit Tool

Fetches content from a URL and optionally converts HTML to markdown or text.

## Capabilities
- HTTP GET and HEAD requests
- HTML to Markdown conversion
- HTML to plain text conversion
- Binary content detection (returns metadata only)
- Automatic timeout handling

## Input Parameters
- `url` (required): The URL to fetch (must be http:// or https://)
- `method` (optional): GET or HEAD (default: GET)
- `as_markdown` (optional): Convert HTML to markdown
- `as_text` (optional): Convert HTML to plain text

## Output Fields
- `url`: The fetched URL
- `status_code`: HTTP status code
- `content_type`: Content-Type header value
- `size`: Content size in bytes
- `last_modified`: Last-Modified header value
- `filename`: Extracted filename
- `format`: "markdown", "text", or "raw"
- `content`: The fetched/converted content
- `truncated`: True if content was truncated due to timeout
- `method`: "HEAD" for HEAD requests
- `error`: Error message for binary content

## Examples

### Fetch a webpage as markdown
```json
{"url": "https://example.com", "as_markdown": true}
```

### Check if a URL exists (HEAD request)
```json
{"url": "https://example.com/file.pdf", "method": "HEAD"}
```

### Fetch raw content
```json
{"url": "https://api.example.com/data.json"}
```

## Error Handling
- Invalid URLs return an error
- Binary content returns metadata with error message
- Timeouts return partial content with truncated flag
"#;
