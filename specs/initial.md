# Decisions:
# - Spec mirrors current FetchKit tool behavior (no new features) unless noted below.
# - Rust is the source of truth: library + CLI + MCP server + Python bindings.
# - HTML conversion is built-in (no external HTML conversion deps).
# - `FetchRequest` and `FetchResponse` are defined in this crate (no external dependency).

# FetchKit Specification

## Abstract

Define a standalone Rust crate named `fetchkit` that implements the existing FetchKit tool
behavior: fetch URL content, optional HTML conversion, strict timeouts, and metadata-only
responses for binary content. The crate also ships a CLI, an MCP server, and Python bindings
that expose the same tool contract.

## Requirements

### Scope

- Provide a reusable library API and a CLI wrapper.
- Provide an MCP server exposing the tool.
- Provide Python bindings that expose the same tool contract.
- No crawling, no JS execution, no cookies, no auth.

### Library

#### Tool Contract

The library defines a tool contract that can be exposed via CLI, MCP, and Python bindings.

- Input schema (args schema): JSON schema equivalent of `FetchRequest`.
- Output schema: JSON schema equivalent of `FetchResponse`.
- Schemas are derived programmatically at runtime and reflect tool builder options
  (disabled options are omitted).
- Async executor: accepts input and produces output.
- Executor streams status updates during fetch/conversion.
- Status protocol is generic and includes estimated completion percentage and ETA.
- Status should be available as a class/object so it can be streamed and also queried.
- Executor can be canceled (Rust-only for now).
- `description`: description applicable for tool execution.
- `system_prompt`: empty string for this tool.
- `docs` / `llmtxt`: full description with examples on how to use the tool.

#### Tool Builder

Provide a builder to configure tool options, including:
- Support for `as_markdown` argument.
- Support allow/block list of URL prefixes.
- Support enabling/disabling request options (feature gating).
- Support User-Agent override (e.g., `user_agent`).
- Support `block_private_ips(bool)` for SSRF prevention (default: `true`).
- Support `enable_save_to_file(bool)` for file download (default: `false`).
  When enabled, adds `save_to_file` to input schema and `saved_path`/`bytes_written` to output.
  Requires a `FileSaver` implementation at execution time.

#### Types

- `FetchRequest`
  - `url: String` (required)
  - `method: HttpMethod` (optional, default GET)
  - `as_markdown: bool` (optional, feature-gated)
  - `as_text: bool` (optional, feature-gated)
  - `save_to_file: Option<String>` (optional, feature-gated via `enable_save_to_file`)
- `HttpMethod` enum: `Get`, `Head`
  - Case-insensitive parser accepts only GET/HEAD.
- `FetchResponse`
  - `url: String`
  - `status_code: u16`
  - `content_type: Option<String>`
  - `size: Option<u64>` (see Size rules)
  - `last_modified: Option<String>`
  - `filename: Option<String>`
  - `format: Option<String>` ("markdown" | "text" | "raw"; omitted for HEAD/binary)
  - `content: Option<String>` (omitted for HEAD/binary)
  - `truncated: Option<bool>` (omitted for HEAD/binary)
  - `method: Option<String>` (set to "HEAD" for HEAD)
  - `error: Option<String>` (binary content only)
  - `saved_path: Option<String>` (set when save_to_file succeeds)
  - `bytes_written: Option<u64>` (set when save_to_file succeeds)
- `FetchError` enum
  - Missing url
  - Invalid url scheme
  - Invalid method
  - Blocked URL (prefix list or DNS policy)
  - Client build failure
  - Request error (timeout/connect/other)
  - Save error (file save failure)
  - Saver not available (save_to_file requested but feature disabled or no saver)
- `ToolStatus` (or equivalent)
  - `phase: String` (generic label, e.g., "validate", "connect", "fetch", "convert")
  - `message: Option<String>`
  - `percent_complete: Option<f32>`
  - `eta_ms: Option<u64>`

#### Function

- `async fn fetch(req: FetchRequest) -> Result<FetchResponse, FetchError>`
  - Used by the tool executor implementation.

### CLI

- Binary name: `fetchkit`.
- CLI provides a convenient interface optimized for LLM consumption.
- Subcommands:
  - `fetch <URL>` - Fetch URL and convert to markdown
  - `mcp` - Run as MCP server over stdio
- Fetch subcommand options:
  - `<URL>` (positional, required)
  - `--output <md|json>` / `-o` (optional, default `md`)
  - `--user-agent <UA>` (optional, overrides default User-Agent)
  - `--help` (standard help)
- Global options:
  - `--llmtxt` (full help with examples and tool details)
  - `--help` (standard help)
- Output format (default `md`):
  - Markdown with YAML frontmatter containing metadata
  - Frontmatter fields: `url`, `status_code`, `source_content_type`, `source_size`,
    `last_modified`, `filename`, `truncated`
  - Content follows frontmatter (markdown-converted HTML or error message)
- Output format (`json`):
  - JSON-serialized `FetchResponse` to stdout
- Exit code: non-zero for `FetchError`.
- `--llmtxt` outputs the tool `docs/llmtxt` content and exits.

### MCP Server

- Expose a single `fetchkit` tool over MCP.
- Input schema: `{ url: string }` (required).
- Output: Markdown with YAML frontmatter (same format as CLI `--output md`).
- Tool description: "Fetch URL and return markdown with metadata frontmatter. Optimized for LLM consumption."

### Python Bindings

- Provide a Python package that exposes the tool contract.
- Bindings should surface the same input/output schema and errors.

### Request Validation

- `url` is required.
- Only `http://` and `https://` URLs allowed.
- Invalid URL: `Invalid URL: must start with http:// or https://`.
- Invalid method: `Invalid method: must be GET or HEAD`.
- Allow/block list prefixes (if configured) are applied before fetch.
  - If allow list is non-empty, URL must match at least one allow prefix.
  - If block list matches, request is denied even if allow list matches.
  - Matching is URL-aware: scheme and host are normalized, trailing dots are ignored,
    path matches respect segment boundaries, and an explicit prefix port must match.
    If the prefix omits a port, any port on the same scheme+host matches.

### SSRF Prevention (DNS Policy)

By default, FetchKit blocks connections to private/reserved IP ranges:
- Resolves hostnames to IP addresses before connecting (resolve-then-check).
- Validates resolved IPs against blocked ranges (loopback, private, link-local,
  cloud metadata, carrier-grade NAT, documentation, benchmarking, multicast, broadcast).
- Handles IPv6-mapped IPv4 addresses via canonicalization.
- Pins validated IP via `reqwest::ClientBuilder::resolve()` to prevent DNS rebinding.
- Blocked by default; opt out via `ToolBuilder::block_private_ips(false)`.
- See `specs/threat-model.md` for full threat analysis.

### HTTP Behavior

- User-Agent: configurable via tool builder or CLI/MCP/Python options
  (default `Everruns FetchKit/1.0`).
- Accept header:
  - Markdown: `text/html, text/markdown, text/plain, */*;q=0.8`
  - Text: `text/html, text/plain, */*;q=0.8`
  - Raw: `*/*`
- HEAD requests use HTTP HEAD method, return metadata only.
- Redirects:
  - Follow at most 10 hops.
  - Each hop is resolved and validated independently against the DNS policy.
  - Redirects to non-HTTP(S) schemes are rejected.

### Timeouts

- First-byte timeout (connect + first response byte): 1s.
- Body timeout: 30s total.
- On body timeout:
  - Return partial body
  - Set `truncated: true`
  - Append `\n\n[..more content timed out...]` to content

### Response Rules

#### Status Handling

- Always return `status_code` when HTTP response received.
- 4xx/5xx are success responses (not tool errors).

#### Binary Content

- Detect binary by Content-Type prefix:
  - `image/`, `audio/`, `video/`, `application/octet-stream`, `application/pdf`,
    `application/zip`, `application/gzip`, `application/x-tar`, `application/x-rar`,
    `application/x-7z`, `application/vnd.ms-`, `application/vnd.openxmlformats`, `font/`.
- For binary:
  - Return metadata (`content_type`, `size`, `filename`, `last_modified`)
  - Include `error: "Binary content is not supported. Only textual content (HTML, text, JSON, etc.) can be fetched."`
  - Omit `content`, `format`, `truncated`

#### HEAD

- Return metadata only.
- Include `method: "HEAD"`.
- Omit `content`, `format`, `truncated`.

#### Save to File

When `save_to_file` is set on the request:
- Binary content is NOT rejected (accepted for file saves).
- Raw bytes are saved via the `FileSaver` trait implementation.
- Response includes `saved_path` and `bytes_written` instead of `content`.
- `content` is omitted (no inline content when saving).
- The `FileSaver` trait provides path validation (traversal prevention) and async save.
- `LocalFileSaver` is the built-in implementation for CLI/local use:
  - Resolves paths relative to a configurable base directory.
  - Rejects path traversal via lexical normalization (note: symlinks not resolved).
  - Creates parent directories as needed.

#### Size

- For binary: `size` from `Content-Length` if present.
- For text/HTML: `size` equals bytes read from body stream (before conversion).

#### Filename

- Prefer `Content-Disposition` `filename=` (quoted or unquoted).
- Fallback to last URL path segment if it contains `.`.

#### HTML Detection

Content is HTML if:
- `Content-Type` contains `text/html` or `application/xhtml`, OR
- Body starts with `<!DOCTYPE` or `<html`.

### Format Conversion

- `as_markdown` takes precedence over `as_text`.
- If HTML:
  - `as_markdown` -> `format: "markdown"`, convert HTML to markdown
  - `as_text` -> `format: "text"`, strip to plain text
- If not HTML:
  - Always `format: "raw"` and return raw body, even if flags set.

### HTML to Markdown

- Strip content inside: `script`, `style`, `noscript`, `iframe`, `svg`.
- `h1`..`h6` -> `#`..`######`.
- Block elements (`p`, `div`, `section`, `article`, `main`, `header`, `footer`):
  - On close, add blank line.
- `br` -> newline, `hr` -> `---`.
- Lists:
  - Track depth for `ul`/`ol`.
  - `li` adds newline and `- ` with two-space indentation per depth.
- `strong`/`b` -> `**`, `em`/`i` -> `*`.
- `pre` -> fenced code block, inline `code` -> backticks (not inside pre).
- `blockquote` -> `> ` prefix.
- `a href="..."` uses naive inline format: `](href)` on open tag (no link text tracking).
- Decode entities: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&#39;`, `&nbsp;`,
  `&mdash;`, `&ndash;`, `&copy;`, `&reg;`.

### HTML to Text

- Strip content inside: `script`, `style`, `noscript`, `iframe`, `svg`.
- Newline on: `p`, `div`, `br`, `h1`..`h6`, `li`, `tr`.
- Same entity decoding as markdown.
- Normalize whitespace via `clean_whitespace` (collapse runs, trim, keep max 2 newlines).

### Newline Filtering

- After conversion or raw response, apply `filter_excessive_newlines`:
  - Keep at most 2 consecutive `\n`.
  - Preserve other whitespace (spaces/tabs).

### Error Handling

- Missing url -> tool error string "Missing required parameter: url".
- Invalid URL -> tool error string "Invalid URL: must start with http:// or https://".
- Invalid method -> tool error string "Invalid method: must be GET or HEAD".
- Blocked URL (prefix or DNS policy) -> tool error string "Blocked URL: not allowed by policy".
- First-byte timeout -> "Request timed out: server did not respond within 1 second".
- Connect error -> "Failed to connect to server".
- Other request errors -> "Request failed: <error>".
- Client build failure -> "Failed to create HTTP client".
- Read errors during streaming: log error, return partial content if any.
- Non-timeout read errors: if partial content is returned, set `truncated: true`.
- Save error -> "Failed to save file: <details>".
- Saver not available -> "File saving not available" (feature disabled or no saver provided).

### Logging

- Emit logging via `tracing` or `log` (best-effort; library must not assume a subscriber).
- Log internal failures with `tracing::error!`.
- Log body timeout with `tracing::warn!`.

### Dependencies

- Use permissive-license deps only.
- Required capabilities: async HTTP client, async runtime, JSON serialization, URL parsing, logging.

### Tests

Unit:
- URL validation, method parsing.
- Binary content detection.
- HTML conversion, entity decoding.
- Newline filtering behavior.
- DNS policy IP range blocking (IPv4, IPv6, mapped addresses).

Integration (mock HTTP server):
- GET/HEAD with expected fields.
- HTML -> markdown/text conversion.
- Binary content metadata response.
- 4xx/5xx status handling.
- Last-Modified extraction.
- Size correctness for text and binary.
- Body timeout truncation.

SSRF security:
- Private IP blocking (loopback, 10.x, 172.16.x, 192.168.x).
- Cloud metadata endpoint blocking (169.254.169.254).
- IPv6 loopback/mapped address blocking.
- Non-HTTP scheme blocking (file, ftp, data, gopher).
- Default-blocks-loopback verification.
- Explicit opt-out verification.
- Script stripping in converted content.
