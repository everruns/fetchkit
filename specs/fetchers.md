# Fetcher System Specification

## Abstract

Fetcher system enables specialized content fetching based on URL patterns. Each fetcher handles specific URL types (e.g., GitHub repos, binary files) with custom logic, returning structured responses optimized for LLM consumption.

## Requirements

### Fetcher Trait

Each fetcher must implement:

1. **`name()`** - Unique identifier string for logging/debugging
2. **`matches(url)`** - Returns true if this fetcher handles the URL
3. **`fetch(request, options)`** - Async fetch returning `FetchResponse` or error

### Fetcher Registry

Central dispatcher that:

1. Maintains ordered list of fetchers (most specific first)
2. Iterates fetchers, uses first matching one
3. Falls back to default fetcher if none match
4. Provides `register()` for adding custom fetchers

### Built-in Fetchers

#### DefaultFetcher (lowest priority)

- Matches: All HTTP/HTTPS URLs
- Behavior: Current `client.rs` fetch logic
- Returns: Standard `FetchResponse` with HTML conversion

#### GitHubRepoFetcher

- Matches: `https://github.com/{owner}/{repo}` (exactly 2 path segments, no file paths)
- Behavior:
  1. Fetch repo metadata via GitHub API (`/repos/{owner}/{repo}`)
  2. Fetch README content if exists (`/repos/{owner}/{repo}/readme`)
  3. Combine into structured response
- Returns: Markdown with repo metadata header + README content
- Response format field: `"github_repo"`

### Response Extensions

`FetchResponse.format` gains new values:
- `"github_repo"` - GitHub repository metadata + README

### Configuration

Fetchers receive `FetchOptions` for:
- User-Agent configuration
- Allow/block URL lists (applied before fetcher matching)

### Extensibility

Design supports hundreds of fetchers by:
- Each fetcher in separate file under `fetchers/` module
- Simple registration pattern
- No compile-time limit on fetcher count

### Error Handling

- Fetcher errors bubble up as `FetchError`
- If specialized fetcher fails, does NOT fall back to default (explicit failure)
- Add `FetchError::FetcherError(String)` for fetcher-specific errors

## Module Structure

```
crates/fetchkit/src/
├── fetchers/
│   ├── mod.rs           # Fetcher trait, FetcherRegistry
│   ├── default.rs       # DefaultFetcher
│   └── github_repo.rs   # GitHubRepoFetcher
```

## API Changes

```rust
// New trait
#[async_trait]
pub trait Fetcher: Send + Sync {
    fn name(&self) -> &'static str;
    fn matches(&self, url: &Url) -> bool;
    async fn fetch(&self, request: &FetchRequest, options: &FetchOptions)
        -> Result<FetchResponse, FetchError>;
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
}
```

## Testing

- Unit tests per fetcher with mocked HTTP
- Integration tests for registry dispatch
- GitHub fetcher tests with mocked GitHub API responses
