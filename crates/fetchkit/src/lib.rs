//! FetchKit - AI-friendly web content fetching library
//!
//! This crate provides a reusable library API for fetching web content,
//! with optional HTML to markdown/text conversion optimized for LLM consumption.
//!
//! # Quick Start
//!
//! ```no_run
//! use fetchkit::{FetchRequest, fetch};
//!
//! # async fn example() -> Result<(), fetchkit::FetchError> {
//! let request = FetchRequest::new("https://example.com").as_markdown();
//! let response = fetch(request).await?;
//! println!("Content: {}", response.content.unwrap_or_default());
//! # Ok(())
//! # }
//! ```
//!
//! # Tool Builder
//!
//! For more control, use the [`ToolBuilder`] to configure options:
//!
//! ```no_run
//! use fetchkit::{FetchRequest, ToolBuilder};
//!
//! # async fn example() -> Result<(), fetchkit::FetchError> {
//! let tool = ToolBuilder::new()
//!     .enable_markdown(true)
//!     .user_agent("MyBot/1.0")
//!     .block_prefix("https://blocked.example.com")
//!     .build();
//!
//! let request = FetchRequest::new("https://example.com");
//! let response = tool.execute(request).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # HTML Conversion
//!
//! Convert HTML to markdown or plain text directly:
//!
//! ```
//! use fetchkit::{html_to_markdown, html_to_text};
//!
//! let html = "<h1>Hello</h1><p>World</p>";
//! let md = html_to_markdown(html);
//! assert!(md.contains("# Hello"));
//!
//! let text = html_to_text(html);
//! assert!(text.contains("Hello"));
//! ```
//!
//! # Fetcher System
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
mod dns;
mod error;
pub mod fetchers;
pub mod file_saver;
mod tool;
mod types;

pub use client::{fetch, fetch_with_options, FetchOptions};
pub use convert::{html_to_markdown, html_to_text};
pub use dns::DnsPolicy;
pub use error::FetchError;
pub use fetchers::{DefaultFetcher, Fetcher, FetcherRegistry, GitHubRepoFetcher};
pub use file_saver::{FileSaveError, FileSaver, LocalFileSaver, SaveResult};
pub use tool::{Tool, ToolBuilder, ToolStatus};
pub use types::{FetchRequest, FetchResponse, HttpMethod};

/// Default User-Agent string
pub const DEFAULT_USER_AGENT: &str = "Everruns FetchKit/1.0";

// -- Tool description fragments (composed dynamically by Tool methods) --

/// Base tool description (always included)
pub(crate) const TOOL_DESCRIPTION_BASE: &str = "\
Fetches content from a URL and optionally converts HTML to markdown or text.

- Supports GET and HEAD methods
- Converts HTML to markdown or plain text
- Returns metadata for binary content
- Strict timeouts for reliability";

/// Save-to-file line appended to description when enabled
pub(crate) const TOOL_DESCRIPTION_SAVE: &str = "\n- File download (save_to_file)";

// -- TOOL_LLMTXT fragments --

pub(crate) const TOOL_LLMTXT_HEADER: &str = "\
# FetchKit Tool

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
- `as_text` (optional): Convert HTML to plain text";

pub(crate) const TOOL_LLMTXT_SAVE_INPUT: &str = "\
\n- `save_to_file` (optional): Save response body to this path instead of returning inline content. \
Accepts binary content (images, PDFs, archives). Requires file saving to be enabled.";

pub(crate) const TOOL_LLMTXT_OUTPUT_BASE: &str = "

## Output Fields
- `url`: The fetched URL
- `status_code`: HTTP status code
- `content_type`: Content-Type header value
- `size`: Content size in bytes
- `last_modified`: Last-Modified header value
- `filename`: Extracted filename
- `format`: \"markdown\", \"text\", or \"raw\"
- `content`: The fetched/converted content
- `truncated`: True if content was truncated due to timeout
- `method`: \"HEAD\" for HEAD requests
- `error`: Error message for binary content";

pub(crate) const TOOL_LLMTXT_SAVE_OUTPUT: &str = "\
\n- `saved_path`: Path where file was saved (when save_to_file was used)
- `bytes_written`: Bytes written to file (when save_to_file was used)";

pub(crate) const TOOL_LLMTXT_EXAMPLES_BASE: &str = "

## Examples

### Fetch a webpage as markdown
```json
{\"url\": \"https://example.com\", \"as_markdown\": true}
```

### Check if a URL exists (HEAD request)
```json
{\"url\": \"https://example.com/file.pdf\", \"method\": \"HEAD\"}
```

### Fetch raw content
```json
{\"url\": \"https://api.example.com/data.json\"}
```";

pub(crate) const TOOL_LLMTXT_SAVE_EXAMPLE: &str = "

### Download a file
```json
{\"url\": \"https://example.com/image.png\", \"save_to_file\": \"image.png\"}
```";

pub(crate) const TOOL_LLMTXT_ERRORS_BASE: &str = "

## Error Handling
- Invalid URLs return an error
- Binary content returns metadata with error message
- Timeouts return partial content with truncated flag";

pub(crate) const TOOL_LLMTXT_SAVE_ERRORS: &str = "\
\n- Binary content is accepted when using save_to_file\n\
- File saving errors include path validation and IO failures";

/// Compose full TOOL_DESCRIPTION with all features (for backward compat / CLI)
pub const TOOL_DESCRIPTION: &str = "\
Fetches content from a URL and optionally converts HTML to markdown or text.\n\
\n\
- Supports GET and HEAD methods\n\
- Converts HTML to markdown or plain text\n\
- Returns metadata for binary content\n\
- Strict timeouts for reliability\n\
- File download (save_to_file)";

/// Compose full TOOL_LLMTXT with all features (for backward compat / CLI)
pub static TOOL_LLMTXT: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| build_llmtxt(true));

/// Build llmtxt string with optional save_to_file sections
pub(crate) fn build_llmtxt(include_save: bool) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str(TOOL_LLMTXT_HEADER);
    if include_save {
        s.push_str(TOOL_LLMTXT_SAVE_INPUT);
    }
    s.push_str(TOOL_LLMTXT_OUTPUT_BASE);
    if include_save {
        s.push_str(TOOL_LLMTXT_SAVE_OUTPUT);
    }
    s.push_str(TOOL_LLMTXT_EXAMPLES_BASE);
    if include_save {
        s.push_str(TOOL_LLMTXT_SAVE_EXAMPLE);
    }
    s.push_str(TOOL_LLMTXT_ERRORS_BASE);
    if include_save {
        s.push_str(TOOL_LLMTXT_SAVE_ERRORS);
    }
    s.push('\n');
    s
}
