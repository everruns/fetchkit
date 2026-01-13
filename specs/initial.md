# Decisions:
# - Spec mirrors current WebFetch tool behavior (no new features) unless noted below.
# - Rust is the source of truth: library + CLI + MCP server + Python bindings.
# - HTML conversion is built-in (no external HTML conversion deps).
# - `WebFetchRequest` and `WebFetchResponse` are defined in this crate (no external dependency).

# WebFetch Specification

## Abstract

Define a standalone Rust crate named `webfetch` that implements the existing WebFetch tool
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

- Input schema (args schema): JSON schema equivalent of `WebFetchRequest`.
- Output schema: JSON schema equivalent of `WebFetchResponse`.
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
- Support User-Agent override (e.g., `allow_ua`).

#### Types

- `WebFetchRequest`
  - `url: String` (required)
  - `method: HttpMethod` (optional, default GET)
  - `as_markdown: bool` (optional, feature-gated)
  - `as_text: bool` (optional, feature-gated)
- `HttpMethod` enum: `Get`, `Head`
  - Case-insensitive parser accepts only GET/HEAD.
- `WebFetchResponse`
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
- `FetchError` enum
  - Missing url
  - Invalid url scheme
  - Invalid method
  - Client build failure
  - Request error (timeout/connect/other)
- `ToolStatus` (or equivalent)
  - `phase: String` (generic label, e.g., "validate", "connect", "fetch", "convert")
  - `message: Option<String>`
  - `percent_complete: Option<f32>`
  - `eta_ms: Option<u64>`

#### Function

- `async fn fetch(req: WebFetchRequest) -> Result<WebFetchResponse, FetchError>`
  - Used by the tool executor implementation.

### CLI

- Binary name: `webfetch`.
- CLI provides a convenient interface that matches this spec (args map to the tool input schema).
- Flags:
  - `--url <URL>` (required)
  - `--method <GET|HEAD>` (optional, default GET)
  - `--as-markdown` (optional)
  - `--as-text` (optional)
  - `--help` (standard help)
  - `--llmtxt` (full help with examples and tool details)
  - `--user-agent <UA>` (optional, overrides default User-Agent)
- Output: JSON-serialized `WebFetchResponse` to stdout.
- Exit code: non-zero for `FetchError`.
- `--llmtxt` outputs the tool `docs/llmtxt` content and exits.

### MCP Server

- Expose the tool contract over MCP.
- Input/output schemas and status updates must match the library tool contract.

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

### HTTP Behavior

- User-Agent: configurable via tool builder or CLI/MCP/Python options
  (default `Everruns WebFetch/1.0`).
- Accept header:
  - Markdown: `text/html, text/markdown, text/plain, */*;q=0.8`
  - Text: `text/html, text/plain, */*;q=0.8`
  - Raw: `*/*`
- HEAD requests use HTTP HEAD method, return metadata only.

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
- Blocked prefix -> tool error string "Blocked URL: prefix not allowed".
- First-byte timeout -> "Request timed out: server did not respond within 1 second".
- Connect error -> "Failed to connect to server".
- Other request errors -> "Request failed: <error>".
- Client build failure -> "Failed to create HTTP client".
- Read errors during streaming: log error, return partial content if any.
- Non-timeout read errors: if partial content is returned, set `truncated: true`.

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

Integration (mock HTTP server):
- GET/HEAD with expected fields.
- HTML -> markdown/text conversion.
- Binary content metadata response.
- 4xx/5xx status handling.
- Last-Modified extraction.
- Size correctness for text and binary.
- Body timeout truncation.
