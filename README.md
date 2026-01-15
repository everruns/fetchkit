# webfetch

AI-friendly web content fetching tool designed for LLM consumption. Rust library with CLI, MCP server, and Python bindings.

## Features

- **HTTP fetching** - GET and HEAD methods with streaming support
- **HTML-to-Markdown** - Built-in conversion optimized for LLMs
- **HTML-to-Text** - Plain text extraction with clean formatting
- **Binary detection** - Returns metadata only for images, PDFs, etc.
- **Timeout handling** - 1s first-byte, 30s body with partial content on timeout
- **URL filtering** - Allow/block lists for controlled access
- **MCP server** - Model Context Protocol support for AI tool integration

## Installation

### From Git (recommended)

```bash
cargo install --git https://github.com/anthropics/webfetch webfetch-cli
```

### From Source

```bash
git clone https://github.com/anthropics/webfetch
cd webfetch
cargo install --path crates/webfetch-cli
```

## CLI Usage

```bash
# Basic fetch
webfetch --url https://example.com

# Convert to markdown
webfetch --url https://example.com --as-markdown

# Convert to plain text
webfetch --url https://example.com --as-text

# HEAD request (metadata only)
webfetch --url https://example.com --method HEAD

# Custom user agent
webfetch --url https://example.com --user-agent "MyBot/1.0"

# Show full documentation
webfetch --llmtxt
```

Output is JSON to stdout:

```json
{
  "url": "https://example.com",
  "status_code": 200,
  "content_type": "text/html",
  "size": 1256,
  "format": "markdown",
  "content": "# Example Domain\n\nThis domain is for use in illustrative examples...",
  "truncated": false,
  "method": "GET"
}
```

## MCP Server

Run as a Model Context Protocol server:

```bash
webfetch mcp
```

Exposes `webfetch` as a tool over JSON-RPC 2.0 stdio transport. Compatible with Claude Desktop and other MCP clients.

## Library Usage

Add to `Cargo.toml`:

```toml
[dependencies]
webfetch = { git = "https://github.com/anthropics/webfetch" }
```

### Basic Fetch

```rust
use webfetch::{fetch, WebFetchRequest};

#[tokio::main]
async fn main() {
    let request = WebFetchRequest {
        url: "https://example.com".to_string(),
        method: None,
        as_markdown: Some(true),
        as_text: None,
    };

    let response = fetch(request).await;
    println!("{}", response.content.unwrap_or_default());
}
```

### With Tool Builder

```rust
use webfetch::Tool;

let tool = Tool::builder()
    .enable_markdown(true)
    .enable_text(false)
    .user_agent("MyBot/1.0")
    .allow_prefix("https://docs.example.com")
    .block_prefix("https://internal.example.com")
    .build();

let response = tool.fetch(request).await;
```

## Python Bindings

```bash
pip install webfetch
```

```python
from webfetch import fetch, WebFetchRequest, WebFetchTool

# Simple fetch
response = fetch("https://example.com", as_markdown=True)
print(response.content)

# With configuration
tool = WebFetchTool(
    enable_markdown=True,
    user_agent="MyBot/1.0",
    allow_prefixes=["https://docs.example.com"]
)
response = tool.fetch(WebFetchRequest(url="https://example.com"))
```

## Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `url` | string | Fetched URL |
| `status_code` | int | HTTP status code |
| `content_type` | string? | Content-Type header |
| `size` | int? | Content size in bytes |
| `last_modified` | string? | Last-Modified header |
| `filename` | string? | From Content-Disposition |
| `format` | string | "markdown", "text", or "raw" |
| `content` | string? | Page content |
| `truncated` | bool | True if content was cut off |
| `method` | string | HTTP method used |
| `error` | string? | Error message if failed |

## Error Handling

Errors are returned in the `error` field:

- `InvalidUrl` - Malformed URL
- `UrlBlocked` - URL blocked by filter
- `NetworkError` - Connection failed
- `Timeout` - Request timed out
- `HttpError` - 4xx/5xx response
- `ContentError` - Failed to read body
- `BinaryContent` - Binary content not supported

## Configuration

### Timeouts

- **First-byte**: 1 second (connect + initial response)
- **Body**: 30 seconds total

Partial content is returned on body timeout with `truncated: true`.

### Binary Content

Automatically detected and returns metadata only for:
- Images, audio, video, fonts
- PDFs, archives (zip, tar, rar, 7z)
- Office documents

### HTML Conversion

**Markdown mode** (`--as-markdown`):
- Headers: `h1-h6` → `#` to `######`
- Lists: Proper nesting with 2-space indent
- Code: Fenced blocks and inline backticks
- Links: `[text](url)` format
- Strips: scripts, styles, iframes, SVGs

**Text mode** (`--as-text`):
- Plain text extraction
- Normalized whitespace
- Newlines for block elements

## License

MIT
