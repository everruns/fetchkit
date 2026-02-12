# fetchkit

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
cargo install --git https://github.com/everruns/fetchkit fetchkit-cli
```

### From Source

```bash
git clone https://github.com/everruns/fetchkit
cd fetchkit
cargo install --path crates/fetchkit-cli
```

## CLI Usage

```bash
# Fetch URL (outputs markdown with frontmatter)
fetchkit fetch https://example.com

# Output as JSON instead
fetchkit fetch https://example.com -o json

# Custom user agent
fetchkit fetch https://example.com --user-agent "MyBot/1.0"

# Show full documentation
fetchkit --llmtxt
```

Default output is markdown with YAML frontmatter:

```markdown
---
url: https://example.com
status_code: 200
source_content_type: text/html; charset=UTF-8
source_size: 1256
---
# Example Domain

This domain is for use in illustrative examples in documents...
```

JSON output (`-o json`):

```json
{
  "url": "https://example.com",
  "status_code": 200,
  "content_type": "text/html",
  "size": 1256,
  "format": "markdown",
  "content": "# Example Domain\n\nThis domain is for use in illustrative examples..."
}
```

## MCP Server

Run as a Model Context Protocol server:

```bash
fetchkit mcp
```

Exposes `fetchkit` tool over JSON-RPC 2.0 stdio transport. Returns markdown with frontmatter (same format as CLI). Compatible with Claude Desktop and other MCP clients.

## Library Usage

Add to `Cargo.toml`:

```toml
[dependencies]
fetchkit = { git = "https://github.com/everruns/fetchkit" }
```

### Basic Fetch

```rust
use fetchkit::{fetch, FetchRequest};

#[tokio::main]
async fn main() {
    let request = FetchRequest::new("https://example.com").as_markdown();

    let response = fetch(request).await.unwrap();
    println!("{}", response.content.unwrap_or_default());
}
```

### With Tool Builder

```rust
use fetchkit::{FetchRequest, ToolBuilder};

let tool = ToolBuilder::new()
    .enable_markdown(true)
    .enable_text(false)
    .user_agent("MyBot/1.0")
    .allow_prefix("https://docs.example.com")
    .block_prefix("https://internal.example.com")
    .build();

let request = FetchRequest::new("https://example.com");
let response = tool.execute(request).await.unwrap();
```

## Python Bindings

```bash
pip install fetchkit
```

```python
from fetchkit_py import fetch, FetchRequest, FetchKitTool

# Simple fetch
response = fetch("https://example.com", as_markdown=True)
print(response.content)

# With configuration
tool = FetchKitTool(
    enable_markdown=True,
    user_agent="MyBot/1.0",
    allow_prefixes=["https://docs.example.com"]
)
response = tool.fetch("https://example.com")
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
| `format` | string? | "markdown", "text", "raw", or "github_repo" |
| `content` | string? | Page content |
| `truncated` | bool? | True if content was cut off |
| `method` | string? | "HEAD" for HEAD requests |
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

HTML is automatically converted to markdown:
- Headers: `h1-h6` → `#` to `######`
- Lists: Proper nesting with 2-space indent
- Code: Fenced blocks and inline backticks
- Links: `[text](url)` format
- Strips: scripts, styles, iframes, SVGs

## License

MIT
