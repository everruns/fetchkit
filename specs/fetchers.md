# Fetcher System Specification

## Abstract

Fetcher system enables specialized content fetching based on URL patterns. Each fetcher handles specific URL types (e.g., GitHub repos, binary files) with custom logic, returning structured responses optimized for LLM consumption.

## Requirements

### Fetcher Trait

Each fetcher must implement:

1. **`name()`** - Unique identifier string for logging/debugging
2. **`matches(url)`** - Returns true if this fetcher handles the URL
3. **`fetch(request, options)`** - Async fetch returning `FetchResponse` or error
4. **`fetch_to_file(request, options, saver)`** - Optional. Default implementation calls `fetch()` then saves string content via `FileSaver`. Fetchers may override for binary-aware saving (e.g., `DefaultFetcher` accepts binary content when saving to file).

### Fetcher Registry

Central dispatcher that:

1. Maintains ordered list of fetchers (most specific first)
2. Iterates fetchers, uses first matching one
3. Falls back to default fetcher if none match
4. Provides `register()` for adding custom fetchers
5. Validates URL scheme and allow/block lists before dispatching
6. Provides `fetch_to_file()` that dispatches to matched fetcher's `fetch_to_file()`

### Built-in Fetchers

#### DefaultFetcher (lowest priority)

- Matches: All HTTP/HTTPS URLs
- Behavior: Standard HTTP fetch with HTML conversion support
- Features:
  - GET and HEAD methods
  - HTML to markdown/text conversion (when enabled)
  - Binary content detection (returns metadata only)
  - Timeout handling with partial content support
  - Binary-aware file saving via `fetch_to_file()` override (accepts binary content when saving)
- Returns: Standard `FetchResponse` with format `"markdown"`, `"text"`, or `"raw"`

#### GitHubRepoFetcher

- Matches: `https://github.com/{owner}/{repo}` (exactly 2 path segments)
- Excludes: Reserved paths (settings, explore, trending, etc.)
- Behavior:
  1. Fetch repo metadata via GitHub API (`/repos/{owner}/{repo}`)
  2. Fetch README content if exists (`/repos/{owner}/{repo}/readme`)
  3. Decode base64 README content
  4. Combine into structured markdown response
- Returns: Markdown with repo metadata + README content
- Response format field: `"github_repo"`
- Metadata includes: stars, forks, issues, language, license, topics, dates

### Response Extensions

`FetchResponse.format` values:
- `"markdown"` - HTML converted to markdown
- `"text"` - HTML converted to plain text
- `"raw"` - Original content unchanged
- `"github_repo"` - GitHub repository metadata + README

### Configuration

Fetchers receive `FetchOptions` for:
- `user_agent` - Custom User-Agent string
- `allow_prefixes` - URL prefix allow list
- `block_prefixes` - URL prefix block list
- `enable_markdown` - Enable markdown conversion
- `enable_text` - Enable text conversion
- `enable_save_to_file` - Enable file saving support
- `dns_policy` - DNS resolution policy for SSRF prevention (default: block private IPs)

### Extensibility

Design supports hundreds of fetchers by:
- Each fetcher in separate file under `fetchers/` module
- Simple registration pattern via `registry.register()`
- No compile-time limit on fetcher count
- Priority determined by registration order

### Error Handling

- Fetcher errors bubble up as `FetchError`
- If specialized fetcher fails, does NOT fall back to default (explicit failure)
- `FetchError::FetcherError(String)` for fetcher-specific errors
- GitHub API errors return response with error field set

### SSRF Protection

Both built-in fetchers integrate resolve-then-check DNS validation:
- Resolve hostname to IP before connecting
- Validate IP against blocked ranges (private, loopback, link-local, etc.)
- Pin validated IP via `reqwest::ClientBuilder::resolve()` to prevent DNS rebinding
- Enabled by default via `DnsPolicy::default()` (blocks private IPs)
- See `specs/threat-model.md` for threat IDs: TM-SSRF-001 through TM-SSRF-010

## Module Structure

```
crates/fetchkit/src/
├── dns.rs               # DnsPolicy - SSRF prevention via resolve-then-check
├── file_saver.rs        # FileSaver trait, LocalFileSaver, SaveResult, FileSaveError
├── fetchers/
│   ├── mod.rs           # Fetcher trait, FetcherRegistry
│   ├── default.rs       # DefaultFetcher (with binary-aware fetch_to_file override)
│   └── github_repo.rs   # GitHubRepoFetcher
```

## API

```rust
// Fetcher trait
#[async_trait]
pub trait Fetcher: Send + Sync {
    fn name(&self) -> &'static str;
    fn matches(&self, url: &Url) -> bool;
    async fn fetch(&self, request: &FetchRequest, options: &FetchOptions)
        -> Result<FetchResponse, FetchError>;
    async fn fetch_to_file(&self, request: &FetchRequest, options: &FetchOptions,
        saver: &dyn FileSaver) -> Result<FetchResponse, FetchError>;
        // Default: delegates to fetch(), then saves content through saver
}

// Registry
pub struct FetcherRegistry {
    fetchers: Vec<Box<dyn Fetcher>>,
}

impl FetcherRegistry {
    pub fn new() -> Self;           // Empty registry
    pub fn with_defaults() -> Self; // Pre-populated with built-in fetchers
    pub fn register(&mut self, fetcher: Box<dyn Fetcher>);
    pub async fn fetch(&self, request: FetchRequest, options: FetchOptions)
        -> Result<FetchResponse, FetchError>;
    pub async fn fetch_to_file(&self, request: FetchRequest, options: FetchOptions,
        saver: &dyn FileSaver) -> Result<FetchResponse, FetchError>;
}

// Convenience functions
pub async fn fetch(req: FetchRequest) -> Result<FetchResponse, FetchError>;
pub async fn fetch_with_options(req: FetchRequest, options: FetchOptions)
    -> Result<FetchResponse, FetchError>;
```

## Testing

### Unit Tests
- Per-fetcher tests with mocked HTTP (wiremock)
- URL matching logic tests
- Response parsing tests

### Integration Tests
- Registry dispatch tests
- End-to-end fetch tests with mock server

### Example-based Tests
Run with: `cargo run -p fetchkit --example fetch_urls`

Tests real URLs:
- Simple HTML pages (example.com)
- JSON endpoints (httpbin.org)
- GitHub repositories
- Raw file content

## Adding a New Fetcher

1. Create `crates/fetchkit/src/fetchers/{name}.rs`
2. Implement `Fetcher` trait
3. Add `mod {name};` and `pub use {name}::*;` to `mod.rs`
4. Register in `FetcherRegistry::with_defaults()` (before DefaultFetcher)
5. Add test cases to `examples/fetch_urls.rs`
