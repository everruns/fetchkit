# Fetcher System Specification

## Abstract

Fetcher system enables specialized content fetching based on URL patterns. Each fetcher handles specific URL types (e.g., GitHub repos, binary files) with custom logic, returning structured responses optimized for LLM consumption.

## Requirements

### Fetcher Trait

Each fetcher must implement:

1. **`name()`** - Unique identifier string for logging/debugging
2. **`matches(url)`** - Returns true if this fetcher handles the URL
3. **`fetch(request, options)`** - Async fetch returning `FetchResponse` or error
4. **`fetch_to_file(request, options, saver)`** - Optional. Path validation runs before fetching. Default implementation then calls `fetch()` and saves string content via `FileSaver`. Fetchers may override for binary-aware saving (e.g., `DefaultFetcher` accepts binary content when saving to file).

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
  - Decompressed body size cap with partial content truncation
- Returns: Standard `FetchResponse` with format `"markdown"`, `"text"`, or `"raw"`

#### GitHubReleaseFetcher

- Matches: `https://github.com/{owner}/{repo}/releases/tag/{tag}`
- Behavior: Fetches tagged release metadata, notes, and assets through the public GitHub API
- Enforces the configured maximum response body size
- Response format field: `"github_release"`

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

#### TwitterFetcher

- Matches: `https://x.com/{user}/status/{id}` and `https://twitter.com/{user}/status/{id}`
- Excludes: Reserved paths (i, settings, explore, search, etc.), non-numeric tweet IDs
- Behavior:
  1. Try syndication API (`cdn.syndication.twimg.com/tweet-result?id={id}`)
  2. Fallback to oEmbed API (`publish.x.com/oembed?url={tweet_url}`)
  3. Format as structured markdown
- Returns: Markdown with tweet text, author info, engagement metrics
- Response format field: `"twitter_tweet"`
- Article tweets: Title as heading, preview text, cover image, link to full article
- Regular tweets: Author heading, tweet text with expanded URLs, media attachments
- Quoted tweets rendered as blockquotes
- Both APIs are unauthenticated; syndication API is undocumented but widely used

#### GitHubCodeFetcher

- Matches: `https://github.com/{owner}/{repo}/blob/{ref}/{path}`
- Excludes: Reserved owner paths (settings, issues, pulls, etc.)
- Behavior: Fetches raw source files via GitHub API, detects language from extension, handles base64 decoding, returns metadata for files >1MB or binary
- Response format field: `"github_file"`

#### GitHubIssueFetcher

- Matches: `https://github.com/{owner}/{repo}/issues/{number}` and `https://github.com/{owner}/{repo}/pull/{number}`
- Excludes: Reserved owner paths, non-numeric IDs
- Behavior: Fetches issue/PR metadata, labels, assignees, milestone, and up to 100 comments; PRs include diff stats and merge status
- Response format field: `"github_issue"` or `"github_pull_request"`

#### StackOverflowFetcher

- Matches: `https://{stackoverflow.com|serverfault.com|superuser.com|askubuntu.com|mathoverflow.net|*.stackexchange.com}/questions/{id}`
- Behavior: Fetches question and top 10 answers sorted by votes via Stack Exchange API
- Response format field: `"stackoverflow_qa"`

#### PackageRegistryFetcher

- Matches: `https://pypi.org/project/{name}`, `https://crates.io/crates/{name}`, `https://www.npmjs.com/package/{name}` (including @scope/name)
- Behavior: Fetches package metadata from respective registry APIs
- Response format field: `"package_registry"`

#### WikipediaFetcher

- Matches: `https://{lang}.wikipedia.org/wiki/{title}`
- Behavior: Fetches article summary via MediaWiki REST API and full HTML, converts to markdown
- Response format field: `"wikipedia"`

#### YouTubeFetcher

- Matches: `https://youtube.com/watch?v={id}`, `https://youtu.be/{id}`
- Behavior: Fetches video metadata via oEmbed API
- Response format field: `"youtube_video"`

#### ArXivFetcher

- Matches: `https://arxiv.org/abs/{id}` and `https://arxiv.org/pdf/{id}`
- Behavior: Fetches paper metadata via arXiv Atom XML API
- Response format field: `"arxiv_paper"`

#### HackerNewsFetcher

- Matches: `https://news.ycombinator.com/item?id={id}`
- Behavior: Fetches item via HN Firebase API with top 20 comments and one level of replies
- Response format field: `"hackernews"`

#### RSSFeedFetcher

- Matches: URLs ending with `/feed`, `/rss`, `/atom`, `.rss`, `.xml` variants
- Behavior: Detects RSS 2.0 or Atom 1.0, parses up to 20 entries
- Response format field: `"rss_feed"`

#### DocsSiteFetcher

- Matches: Direct `/llms.txt` or `/llms-full.txt` URLs, or known docs sites (ReadTheDocs, docs.rs, GitBook, etc.)
- Behavior: Direct `/llms.txt` or `/llms-full.txt` URLs fetch that file. Root docs site URLs probe for `llms-full.txt`/`llms.txt` at origin; if not found, fetch the root page. Specific docs page URLs fetch the requested page and convert HTML to markdown.
- Response format field: `"documentation"` or `"markdown"`

### Response Extensions

`FetchResponse.format` values:
- `"markdown"` - HTML converted to markdown
- `"text"` - HTML converted to plain text
- `"raw"` - Original content unchanged
- `"github_repo"` - GitHub repository metadata + README
- `"github_file"` - GitHub source file content
- `"github_issue"` - GitHub issue content
- `"github_pull_request"` - GitHub pull request content
- `"twitter_tweet"` - Twitter/X tweet content with metadata
- `"stackoverflow_qa"` - Stack Overflow Q&A
- `"package_registry"` - Package registry metadata
- `"wikipedia"` - Wikipedia article
- `"youtube_video"` - YouTube video metadata
- `"arxiv_paper"` - arXiv paper metadata
- `"hackernews"` - Hacker News item with comments
- `"rss_feed"` - RSS/Atom feed entries
- `"documentation"` - Documentation site content

### HTTP Transport

All fetchers perform their outbound HTTP exclusively through a pluggable
`HttpTransport` (see `transport.rs`). The transport is a single-hop socket adapter:
it never follows redirects and never performs DNS policy resolution. fetchkit owns
URL validation, DNS policy (resolve-then-check, producing `TransportRequest.pinned_addrs`),
manual per-hop redirect following, bot-auth signing, and body-size/timeout caps;
only the socket-level send is delegated.

`FetchOptions.transport` selects the implementation (`None` => default
`ReqwestTransport`). A host application can supply its own transport to route
fetchkit through a dedicated egress boundary without weakening any security policy.
When `pinned_addrs` is non-empty a transport MUST connect only to those addresses
(TM-SSRF-001, TM-SSRF-005).

Hosts that consume fetchkit through the `Tool` surface inject the transport with
`ToolBuilder::transport(Arc<dyn HttpTransport>)`; every Tool execution path
(`execute`, `execute_with_status`, `execute_with_saver`, JSON `execution`/service)
honors it, so the host keeps Tool's description/schema/llmtxt and FetchOptions
assembly while owning egress.

### Browser-Rendered Fetching

Browser-rendered fetching is optional and MUST NOT be enabled by default.
It is a fetcher/render-backend concern, not an `HttpTransport` concern:
rendering needs page lifecycle, JavaScript execution, subresource policy,
DOM snapshotting, and wait strategy, while `HttpTransport` remains a
single-hop socket adapter.

The first lightweight rendered mode MUST be exposed explicitly behind a
Cargo feature named `render-rakers`. It may use the rakers-style approach:
parse HTML, execute JavaScript in a lightweight runtime with a partial DOM,
serialize the post-execution DOM, then pass that HTML through the existing
markdown/text conversion path.

`render-rakers` requirements:
- Disabled unless the `render-rakers` Cargo feature is enabled.
- Not part of default features.
- Documented as partial browser rendering, not a full browser engine.
- Best-effort for SPAs and client-rendered docs; no guarantee for pages that
  require real layout, WebGL, service workers, browser fingerprinting, or a
  complete DOM/CSS engine.
- Must honor fetchkit URL validation, allow/block lists, DNS policy, proxy
  policy, timeout policy, and body-size limits for the initial page.
- Must re-apply the configured body-size limit to rendered HTML before
  metadata extraction, boilerplate stripping, or markdown/text conversion.
- Must not let the rakers runtime bypass fetchkit egress policy. Until
  subresource fetches can be routed through fetchkit policy, rakers-initiated
  external script, fetch, and XHR requests must be denied.
- Must expose an explicit request/config switch; enabling the Cargo feature
  only makes the backend available and does not change default fetch behavior.

Future real-browser rendering MUST be a separate backend and feature flag, for
example `render-servo`. Servo support must not reuse the `render-rakers` feature
because it has different dependency, fidelity, security, and platform tradeoffs.

### Configuration

Fetchers receive `FetchOptions` for:
- `user_agent` - Custom User-Agent string
- `allow_prefixes` - URL prefix allow list
- `block_prefixes` - URL prefix block list
- `enable_markdown` - Enable markdown conversion
- `enable_text` - Enable text conversion
- `enable_save_to_file` - Enable file saving support
- `dns_policy` - DNS resolution policy for SSRF prevention (default: block private IPs)
- `max_body_size` - Maximum response body size after decompression
  (default: 10 MB)
- `respect_proxy_env` - Whether to honor `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`
  from the process environment (default: disabled)

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
- Ignore ambient proxy env by default so shared runtimes do not silently route
  traffic through operator-provided proxies unless explicitly enabled
- See `specs/threat-model.md` for threat IDs: TM-SSRF-001 through TM-SSRF-010

## Module Structure

```
crates/fetchkit/src/
├── dns.rs               # DnsPolicy - SSRF prevention via resolve-then-check
├── file_saver.rs        # FileSaver trait, LocalFileSaver, SaveResult, FileSaveError
├── fetchers/
│   ├── mod.rs           # Fetcher trait, FetcherRegistry
│   ├── arxiv.rs         # ArXivFetcher
│   ├── default.rs       # DefaultFetcher (with binary-aware fetch_to_file override)
│   ├── docs_site.rs     # DocsSiteFetcher
│   ├── github_code.rs   # GitHubCodeFetcher
│   ├── github_issue.rs  # GitHubIssueFetcher
│   ├── github_repo.rs   # GitHubRepoFetcher
│   ├── hackernews.rs    # HackerNewsFetcher
│   ├── package_registry.rs # PackageRegistryFetcher
│   ├── rss_feed.rs      # RSSFeedFetcher
│   ├── stackoverflow.rs # StackOverflowFetcher
│   ├── twitter.rs       # TwitterFetcher
│   ├── wikipedia.rs     # WikipediaFetcher
│   └── youtube.rs       # YouTubeFetcher
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

Built-in fetchers normalize `FetchRequest::url` before parsing, so direct calls to
`Fetcher::fetch` accept the same URL forms as the registry and tool surfaces:
explicit `http://`, explicit `https://`, or bare domain URLs normalized to
`https://`.

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
