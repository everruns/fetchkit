# Threat Model

## Abstract

Threat model for FetchKit, an AI-friendly web content fetching library. FetchKit is designed
to be embedded in AI agent platforms (e.g., Everruns) where untrusted user prompts can
influence which URLs are fetched. This document identifies threats that arise when FetchKit
runs inside a container or cluster with access to internal network resources, and tracks
mitigations implemented in the library.

## Verification Status

Last verified: 2026-03-12

Verified in this review:
- `cargo test --workspace -- --nocapture`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
- `cargo run -p fetchkit-cli -- fetch https://example.com --output json`
- `cargo run -p fetchkit-cli -- fetch http://127.0.0.1 --output json`
- JSON-RPC smoke test against `cargo run -p fetchkit-cli -- mcp`

## Threat ID Scheme

**Format:** `TM-<CATEGORY>-<NNN>`

| Prefix | Category | Description |
|--------|----------|-------------|
| TM-SSRF | Server-Side Request Forgery | Internal resource access, IP bypass, DNS rebinding |
| TM-NET | Network Security | Redirect abuse, protocol smuggling, connection reuse |
| TM-INPUT | Input Validation | URL parsing, prefix bypass, scheme injection |
| TM-DOS | Denial of Service | Resource exhaustion, slowloris, large payloads |
| TM-LEAK | Information Leakage | Error messages, metadata exposure, timing |
| TM-CONV | Content Conversion | HTML parsing abuse, injection via converted content |

### Managing Threat IDs

1. Assign the next sequential number within the category.
2. Never reuse a retired ID.
3. Add code comments at mitigation points: `// THREAT[TM-XXX-NNN]: description`.
4. Add tests that exercise the mitigation.

### Code Comment Format

```rust
// THREAT[TM-XXX-NNN]: Brief description of the threat being mitigated
// Mitigation: What this code does to prevent the attack
```

## Trust Model

```
┌─────────────────────────────────────────────────────┐
│                  Host / Cluster                      │
│                                                      │
│  ┌──────────────────────────────────────────────┐   │
│  │           Container / Sandbox                 │   │
│  │                                               │   │
│  │  ┌─────────────┐     ┌──────────────────┐    │   │
│  │  │  AI Agent    │────▶│    FetchKit      │    │   │
│  │  │  (LLM loop)  │     │  (library/CLI/   │    │   │
│  │  │              │     │   MCP server)    │    │   │
│  │  └─────────────┘     └───────┬──────────┘    │   │
│  │                              │                │   │
│  │  ─ ─ ─ ─ ─ ─ Trust Boundary 1 ─ ─ ─ ─ ─ ─  │   │
│  │                              │                │   │
│  │                   ┌──────────▼──────────┐     │   │
│  │                   │   Network Stack     │     │   │
│  │                   │  (DNS + HTTP/TLS)   │     │   │
│  │                   └──────────┬──────────┘     │   │
│  └──────────────────────────────┼────────────────┘   │
│                                 │                     │
│  ─ ─ ─ ─ ─ ─ ─ Trust Boundary 2 ─ ─ ─ ─ ─ ─ ─ ─   │
│                                 │                     │
│  ┌──────────────────────────────▼────────────────┐   │
│  │            Internal Network                    │   │
│  │  ┌──────────┐  ┌───────────┐  ┌────────────┐ │   │
│  │  │ Metadata │  │ K8s API   │  │ Internal   │ │   │
│  │  │ Service  │  │ Server    │  │ Services   │ │   │
│  │  │169.254.  │  │           │  │            │ │   │
│  │  │169.254   │  │           │  │            │ │   │
│  │  └──────────┘  └───────────┘  └────────────┘ │   │
│  └───────────────────────────────────────────────┘   │
│                                                      │
│  ─ ─ ─ ─ ─ ─ ─ Trust Boundary 3 ─ ─ ─ ─ ─ ─ ─ ─   │
│                                                      │
└──────────────────────────────────────────────────────┘
                          │
               ┌──────────▼──────────┐
               │   Public Internet   │
               └─────────────────────┘
```

**Trust Boundary 1 — Agent to FetchKit:**
The AI agent passes user-influenced URLs to FetchKit. FetchKit must treat all
URLs as untrusted input. The agent cannot be relied upon to validate URLs since
adversarial prompts can manipulate it.

**Trust Boundary 2 — Container to Internal Network:**
The container typically has network access to internal services (metadata endpoints,
Kubernetes API, databases). FetchKit must prevent requests that cross this boundary
unless explicitly allowed.

**Trust Boundary 3 — Cluster to Public Internet:**
Outbound requests to the public internet are the intended use case. FetchKit should
only allow connections to publicly-routable IP addresses by default.

## 1. Server-Side Request Forgery (TM-SSRF)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-SSRF-001 | Private IP access via URL | Critical | Resolve-then-check: resolve hostname, validate IP against blocked ranges before connecting | MITIGATED |
| TM-SSRF-002 | Loopback access (127.0.0.1, ::1) | Critical | Blocked in private IP ranges; also blocks `localhost` after resolution | MITIGATED |
| TM-SSRF-003 | Cloud metadata endpoint (169.254.169.254) | Critical | Link-local range blocked; specific metadata IPs covered by range check | MITIGATED |
| TM-SSRF-004 | Numeric IP variants (octal 0177.0.0.1, hex 0x7f000001, decimal 2130706433) | High | URL parsed by `url` crate which normalizes IP representations; resolved IP validated | MITIGATED |
| TM-SSRF-005 | DNS rebinding (hostname resolves to public IP, then re-resolves to private) | High | Pin DNS resolution via `reqwest::ClientBuilder::resolve()`; validated IP used for connection | MITIGATED |
| TM-SSRF-006 | IPv6-mapped IPv4 (::ffff:127.0.0.1) | High | `to_canonical()` extracts IPv4 from mapped addresses before range check | MITIGATED |
| TM-SSRF-007 | DNS names resolving to private IPs | Critical | Post-resolution IP check catches all DNS-to-private-IP scenarios | MITIGATED |
| TM-SSRF-008 | Kubernetes service DNS (*.svc.cluster.local) | High | Resolves to cluster IPs which are private ranges; blocked by IP check | MITIGATED |
| TM-SSRF-009 | URL with credentials (http://user:pass@internal) | Medium | Credentials in URL passed through to reqwest; no credential stripping | **ACCEPTED** |
| TM-SSRF-010 | Redirect to internal resource | High | Default fetcher follows redirects manually; each hop is re-parsed and re-validated against scheme and DNS policy | MITIGATED |

### Mitigation Details

**TM-SSRF-001 — Resolve-then-check (MITIGATED):**
FetchKit resolves the hostname to IP addresses using the system resolver, validates
each resolved IP against blocked ranges, and pins the validated IP via
`reqwest::ClientBuilder::resolve()` to prevent re-resolution.

Blocked ranges:
- Loopback: `127.0.0.0/8`, `::1`
- Private: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- Link-local: `169.254.0.0/16`, `fe80::/10`
- Unspecified: `0.0.0.0/32`, `::/128`
- Documentation: `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`
- Benchmarking: `198.18.0.0/15`
- Carrier-grade NAT: `100.64.0.0/10`
- Unique local (IPv6): `fc00::/7`
- Multicast: `224.0.0.0/4`, `ff00::/8`
- Broadcast: `255.255.255.255/32`

**TM-SSRF-004 — Numeric IP variants (MITIGATED):**
The `url` crate normalizes IP representations during parsing. FetchKit validates
the resolved `IpAddr` (not the string), so octal/hex/decimal-encoded IPs are
caught after normalization.

**TM-SSRF-005 — DNS rebinding (MITIGATED):**
After validating the resolved IP, FetchKit uses `reqwest::ClientBuilder::resolve(host, addr)`
to pin the connection to the validated IP. This prevents reqwest from re-resolving
the hostname during connection establishment.

**TM-SSRF-009 — URL credentials (ACCEPTED):**
FetchKit passes URLs to reqwest as-is. If credentials are embedded in the URL,
they are sent with the request. This is acceptable because:
- FetchKit only supports GET/HEAD (read-only operations)
- The URL comes from the caller who controls what credentials to include
- Stripping credentials would break legitimate use cases
- **Risk:** Low. Mitigated at the caller level.

**TM-SSRF-010 — Redirect to internal resource (MITIGATED):**
Automatic redirects are disabled. The default fetcher follows redirects manually
(max 10 hops), reparses the `Location` target, rejects non-HTTP(S) schemes, and
rebuilds the client for each hop so DNS resolution and private-IP validation run
again before every outbound connection. The GitHub fetcher disables redirects
entirely because it only talks to `api.github.com`.

## 2. Network Security (TM-NET)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-NET-001 | HTTP downgrade (HTTPS URL redirects to HTTP) | Medium | Redirects are validated manually but HTTP is still allowed as a destination scheme | **ACCEPTED** |
| TM-NET-002 | TLS certificate validation bypass | Low | Uses reqwest defaults (system certificate store via rustls-platform-verifier) | MITIGATED |
| TM-NET-003 | Connection reuse leaking context | Low | New reqwest client per request; no connection pooling across requests | MITIGATED |
| TM-NET-004 | Proxy environment variables (HTTP_PROXY) | Medium | Reqwest respects system proxy env vars; attacker could set these in container | **CALLER RISK** |
| TM-NET-005 | Man-in-the-middle on HTTP (non-TLS) | Medium | HTTP scheme is allowed; content can be intercepted/modified on the wire | **ACCEPTED** |

### Mitigation Details

**TM-NET-001 — HTTP downgrade (ACCEPTED):**
FetchKit allows both HTTP and HTTPS schemes. If an HTTPS URL redirects to HTTP,
FetchKit will still follow the redirect after validating the new target. This is
accepted because:
- FetchKit is designed for content fetching, not security-sensitive operations
- The caller controls which URLs to fetch
- Enforcing HTTPS-only would break many legitimate use cases

**TM-NET-003 — Connection reuse (MITIGATED):**
The `DefaultFetcher` creates a new `reqwest::Client` per request, which prevents
connection pool state from leaking between requests. This is a defense-in-depth
measure.

**TM-NET-004 — Proxy environment variables (CALLER RISK):**
Reqwest automatically reads `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` environment
variables. In a container environment, these should be controlled by the operator.
This is the caller's responsibility to configure or clear.

## 3. Input Validation (TM-INPUT)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-INPUT-001 | Non-HTTP scheme (file://, ftp://, data:) | High | Explicit scheme check: only `http://` and `https://` prefixes allowed | MITIGATED |
| TM-INPUT-002 | URL prefix normalization edge cases | Medium | Prefix matching parses URLs, normalizes scheme/host/trailing dot, and checks path boundaries; encoded path canonicalization is still limited | **PARTIALLY MITIGATED** |
| TM-INPUT-003 | Empty or malformed URL | Low | Empty URL check and `url::Url::parse()` validation | MITIGATED |
| TM-INPUT-004 | Extremely long URL | Low | No explicit length limit; reqwest/OS handles | **ACCEPTED** |
| TM-INPUT-005 | URL with fragment/query manipulation | Low | Fragments and queries are part of the URL; no special handling needed | **BY DESIGN** |
| TM-INPUT-006 | Prefix bypass via URL authority (http://evil.com@127.0.0.1) | Medium | `url` crate parses authority correctly; resolve-then-check validates the actual host | MITIGATED |
| TM-INPUT-007 | Prefix matching is string-based, not URL-aware | Medium | Prefixes are parsed and compared by scheme, host, optional port, path boundary, and optional query | MITIGATED |

### Mitigation Details

**TM-INPUT-001 — Scheme validation (MITIGATED):**
```rust
// THREAT[TM-INPUT-001]: Block non-HTTP schemes (file://, ftp://, data:, etc.)
// Mitigation: Early return with InvalidUrlScheme error
if !request.url.starts_with("http://") && !request.url.starts_with("https://") {
    return Err(FetchError::InvalidUrlScheme);
}
```

**TM-INPUT-002 — URL prefix normalization edge cases (PARTIALLY MITIGATED):**
Prefix matching no longer relies on raw string prefixes. FetchKit parses both the
policy prefix and candidate URL, lowercases and de-dots the host, respects path
segment boundaries, and treats an omitted prefix port as "any port on this host".
Residual risk remains for percent-encoded path variants because FetchKit does not
fully canonicalize encoded path segments before comparison.

**TM-INPUT-006 — URL authority bypass (MITIGATED):**
URLs like `http://evil.com@127.0.0.1/path` have `127.0.0.1` as the host (with
`evil.com` as the username). The `url` crate correctly parses this, and
resolve-then-check validates the actual host's IP.

**TM-INPUT-007 — URL-aware prefix matching (MITIGATED):**
Allow/block prefixes are parsed as URLs and compared structurally:
- Scheme must match
- Host is lowercased and trailing dots are ignored
- Explicit prefix ports must match; omitted prefix ports match any port
- Path prefixes respect segment boundaries (`/api` matches `/api/v1`, not `/apiv1`)
- If the prefix includes a query string, it must match exactly

This closes the allow-list overmatch case where a raw prefix like
`https://allowed.example.com` previously also matched
`https://allowed.example.com.evil.test`.

## 4. Denial of Service (TM-DOS)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-DOS-001 | Unbounded response body | Medium | 30-second body timeout; no max body size limit | **OPEN** |
| TM-DOS-002 | Slowloris / slow body | Low | 1-second first-byte timeout; 30-second body timeout | MITIGATED |
| TM-DOS-003 | Compressed content bomb (gzip bomb) | Medium | Reqwest decompresses gzip/brotli/deflate; no decompressed size limit | **OPEN** |
| TM-DOS-004 | Rapid request flooding via tool | Low | No rate limiting in FetchKit; caller responsibility | **CALLER RISK** |
| TM-DOS-005 | DNS resolution delay | Low | DNS resolution uses system resolver; no explicit timeout on DNS lookup | **ACCEPTED** |
| TM-DOS-006 | Memory exhaustion from large HTML conversion | Medium | HTML converter processes in-memory; no streaming conversion | **OPEN** |

### Mitigation Details

**TM-DOS-001 — Unbounded response body (OPEN):**
FetchKit reads the entire response body into memory (up to 30-second timeout).
A malicious server could stream data at just above the timeout threshold,
consuming significant memory.
- **Recommendation:** Add a configurable `max_body_size` (e.g., 10 MB default)
  that truncates the response and sets `truncated: true`.
- **Priority:** Medium

**TM-DOS-002 — Slowloris (MITIGATED):**
The 1-second first-byte timeout prevents connections from being held open
indefinitely during the initial handshake. The 30-second body timeout provides
a hard ceiling on total request duration.

**TM-DOS-003 — Compressed content bomb (OPEN):**
Reqwest is configured with `gzip`, `brotli`, and `deflate` features, which
enable transparent decompression. A small compressed payload could decompress
to a very large body.
- **Recommendation:** Monitor decompressed body size against `max_body_size`.
- **Priority:** Medium

## 5. Information Leakage (TM-LEAK)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-LEAK-001 | Error messages reveal internal network topology | Medium | Error messages include connect/timeout details but not resolved IPs | MITIGATED |
| TM-LEAK-002 | DNS resolution errors reveal internal DNS | Low | DNS errors surfaced as connect errors; hostname visible in error | **ACCEPTED** |
| TM-LEAK-003 | Response content leaks internal data | Low | FetchKit returns content as-is; caller must filter sensitive data | **CALLER RISK** |
| TM-LEAK-004 | User-Agent reveals software version | Info | Default UA `Everruns FetchKit/1.0` reveals stack; configurable | **BY DESIGN** |
| TM-LEAK-005 | Timing side-channels (connect time reveals network proximity) | Low | 1-second timeout masks some timing; not fully mitigated | **ACCEPTED** |

### Mitigation Details

**TM-LEAK-001 — Error message detail (MITIGATED):**
FetchKit's error types (`FetchError`) use generic messages that don't include
resolved IP addresses. Connect errors say "Failed to connect to server" without
revealing the specific IP or port that was attempted.

## 6. Content Conversion (TM-CONV)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-CONV-001 | Script injection in converted markdown | Low | `<script>` tags stripped during HTML-to-markdown conversion | MITIGATED |
| TM-CONV-002 | Excessive memory from deeply nested HTML | Medium | No recursion depth limit in HTML parser | **ACCEPTED** |
| TM-CONV-003 | Markdown injection (crafted HTML producing executable markdown) | Low | FetchKit produces markdown text; execution depends on downstream consumer | **BY DESIGN** |
| TM-CONV-004 | Entity decoding producing unexpected characters | Low | Limited entity set decoded; no arbitrary numeric entity expansion | MITIGATED |

### Mitigation Details

**TM-CONV-001 — Script stripping (MITIGATED):**
The HTML converter skips content inside `script`, `style`, `noscript`, `iframe`,
and `svg` tags, preventing script injection into the converted output.

**TM-CONV-002 — Deeply nested HTML (ACCEPTED):**
The HTML parser is character-based and iterative (not recursive), so stack
overflow from deep nesting is unlikely. However, deeply nested structures could
produce large output. This remains accepted only because the parser is iterative;
there is currently no upstream body size cap, so TM-DOS-001/TM-DOS-006 remain open.

## Vulnerability Summary

### Open Threats (Require Action)

| ID | Threat | Severity | Recommendation |
|----|--------|----------|----------------|
| TM-INPUT-002 | URL prefix normalization edge cases | Medium | Canonicalize percent-encoded path variants before comparison |
| TM-DOS-001 | Unbounded response body | Medium | Add configurable max_body_size |
| TM-DOS-003 | Compressed content bomb | Medium | Monitor decompressed size |
| TM-DOS-006 | Memory exhaustion from HTML conversion | Medium | Add conversion size limit |

### Accepted Risks

| ID | Threat | Rationale |
|----|--------|-----------|
| TM-SSRF-009 | URL credentials | Read-only ops; caller controls credentials |
| TM-NET-001 | HTTP downgrade | Content fetching; not security-sensitive |
| TM-NET-005 | HTTP MITM | HTTP scheme intentionally allowed |
| TM-INPUT-004 | Long URLs | OS/library limits sufficient |
| TM-DOS-005 | DNS delay | System resolver; typical behavior |
| TM-LEAK-002 | DNS error detail | Hostname visible but not internal IPs |
| TM-LEAK-005 | Timing channels | Low risk; timeout masks some signal |
| TM-CONV-002 | Deep HTML nesting | Iterative parser avoids stack overflow, but large-output DoS remains tracked separately |

### Caller Responsibilities

| Responsibility | Related Threats | Description |
|---------------|----------------|-------------|
| Rate limiting | TM-DOS-004 | Caller must implement request rate limits |
| Proxy config | TM-NET-004 | Clear or set HTTP_PROXY env vars appropriately |
| Content filtering | TM-LEAK-003 | Filter sensitive data from responses |
| URL allow-listing | TM-INPUT-002 | Use allow_prefixes for positive security model and prefer exact path prefixes |

## Security Controls Matrix

| Control | Category | Implementation |
|---------|----------|---------------|
| Scheme validation | TM-INPUT | `starts_with("http://")` check |
| URL prefix allow/block | TM-INPUT | Parsed URL comparison in `FetcherRegistry` |
| Private IP blocking | TM-SSRF | `DnsPolicy::block_private_ips()` with resolve-then-check |
| DNS pinning | TM-SSRF | `reqwest::ClientBuilder::resolve()` |
| Redirect hop validation | TM-SSRF | Manual redirect loop in `DefaultFetcher`; redirects disabled in `GitHubRepoFetcher` |
| IPv6-mapped-IPv4 canonicalization | TM-SSRF | `IpAddr::to_canonical()` before range check |
| First-byte timeout | TM-DOS | 1-second connect+response timeout |
| Body timeout | TM-DOS | 30-second streaming body timeout |
| Script tag stripping | TM-CONV | Skip `script`/`style`/`noscript`/`iframe`/`svg` |
| Binary detection | TM-CONV | Content-Type prefix matching |
| New client per request | TM-NET | No connection pool state leakage |

## References

- `specs/initial.md` — FetchKit tool specification
- `specs/fetchers.md` — Pluggable fetcher system
- [OWASP SSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html)
- [CWE-918: Server-Side Request Forgery](https://cwe.mitre.org/data/definitions/918.html)
