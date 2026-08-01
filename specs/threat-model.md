# Threat Model

## Abstract

Threat model for Fetchkit, an AI-friendly web content fetching library. Fetchkit is designed
to be embedded in AI agent platforms (e.g., Everruns) where untrusted user prompts can
influence which URLs are fetched. This document identifies threats that arise when Fetchkit
runs inside a container or cluster with access to internal network resources, and tracks
mitigations implemented in the library.

## Verification Status

Last verified: 2026-05-17

Verified in this review:
- `cargo test --workspace -- --nocapture`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
- `cargo run -p fetchkit-cli -- fetch https://example.com --output json`
- `cargo run -p fetchkit-cli -- fetch http://127.0.0.1 --output json`
- `HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 cargo run -p fetchkit-cli -- fetch https://example.com --output json`
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
| TM-AUTH | Bot Authentication | Signing key exposure, replay, signature scope |

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
│  │  │  AI Agent    │────▶│    Fetchkit      │    │   │
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

**Trust Boundary 1 — Agent to Fetchkit:**
The AI agent passes user-influenced URLs to Fetchkit. Fetchkit must treat all
URLs as untrusted input. The agent cannot be relied upon to validate URLs since
adversarial prompts can manipulate it.

**Trust Boundary 2 — Container to Internal Network:**
The container typically has network access to internal services (metadata endpoints,
Kubernetes API, databases). Fetchkit must prevent requests that cross this boundary
unless explicitly allowed.

**Trust Boundary 3 — Cluster to Public Internet:**
Outbound requests to the public internet are the intended use case. Fetchkit should
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
| TM-SSRF-011 | IPv4-compatible IPv6 (::127.0.0.1) and 6to4 (2002:7f00:1::) | Medium | Extract embedded IPv4 from deprecated IPv4-compatible and 6to4 addresses, validate against blocked ranges | MITIGATED |
| TM-SSRF-007 | DNS names resolving to private IPs | Critical | Post-resolution IP check catches all DNS-to-private-IP scenarios | MITIGATED |
| TM-SSRF-008 | Kubernetes service DNS (*.svc.cluster.local) | High | Resolves to cluster IPs which are private ranges; blocked by IP check | MITIGATED |
| TM-SSRF-009 | URL with credentials (http://user:pass@internal) | Medium | Credentials in URL passed through to reqwest; no credential stripping | **ACCEPTED** |
| TM-SSRF-010 | Redirect to internal resource | High | Manual redirect following with IP validation at each hop | MITIGATED |

### Mitigation Details

**TM-SSRF-001 — Resolve-then-check (MITIGATED):**
Fetchkit resolves the hostname to IP addresses using the system resolver, validates
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
The `url` crate normalizes IP representations during parsing. Fetchkit validates
the resolved `IpAddr` (not the string), so octal/hex/decimal-encoded IPs are
caught after normalization.

**TM-SSRF-005 — DNS rebinding (MITIGATED):**
After validating the resolved IP, Fetchkit uses `reqwest::ClientBuilder::resolve(host, addr)`
to pin the connection to the validated IP. This prevents reqwest from re-resolving
the hostname during connection establishment.

**TM-SSRF-011 — IPv4-compatible and 6to4 IPv6 addresses (MITIGATED):**
IPv4-compatible IPv6 addresses (`::<ipv4>`, deprecated RFC 4291) and 6to4
addresses (`2002::/16`, RFC 3056) embed IPv4 addresses that `to_canonical()`
does not extract. `is_blocked_ipv6()` now detects both formats, extracts
the embedded IPv4, and validates it against the blocked ranges.

**TM-SSRF-009 — URL credentials (ACCEPTED):**
Fetchkit passes URLs to reqwest as-is. If credentials are embedded in the URL,
they are sent with the request. This is acceptable because:
- Fetchkit only supports GET/HEAD (read-only operations)
- The URL comes from the caller who controls what credentials to include
- Stripping credentials would break legitimate use cases
- **Risk:** Low. Mitigated at the caller level.

**TM-SSRF-010 — Redirect to internal resource (MITIGATED):**
Automatic redirects are disabled via `reqwest::redirect::Policy::none()`. Fetchkit
manually follows redirects (up to 10 hops) and performs full IP validation
(resolve-then-check with DNS pinning) at each hop. Scheme validation is also
enforced at each hop, preventing redirects to non-HTTP schemes (e.g., `file://`).
A new `reqwest::Client` is built per hop to ensure DNS pinning applies to the
redirect target, not the original host.

## 2. Network Security (TM-NET)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-NET-001 | HTTP downgrade (HTTPS URL redirects to HTTP) | Medium | Scheme validated per hop (non-HTTP blocked); HTTPS→HTTP downgrade allowed | **ACCEPTED** |
| TM-NET-002 | TLS certificate validation bypass | Low | Uses reqwest defaults (system certificate store via rustls-platform-verifier) | MITIGATED |
| TM-NET-003 | Connection reuse leaking context | Low | New reqwest client per request; no connection pooling across requests | MITIGATED |
| TM-NET-004 | Proxy environment variables (HTTP_PROXY) | Medium | Clients ignore ambient proxy env by default; callers can opt in explicitly | MITIGATED |
| TM-NET-004 | Proxy environment variables (HTTP_PROXY) | Medium | Ambient proxy env is ignored by default; opt-in required via builder/CLI | MITIGATED |
| TM-NET-005 | Man-in-the-middle on HTTP (non-TLS) | Medium | HTTP scheme is allowed; content can be intercepted/modified on the wire | **ACCEPTED** |

### Mitigation Details

**TM-NET-001 — HTTP downgrade (ACCEPTED):**
Fetchkit validates the scheme at each redirect hop — non-HTTP(S) schemes are
rejected (see TM-INPUT-001). However, HTTPS→HTTP downgrade is still allowed.
This is accepted because:
- Fetchkit is designed for content fetching, not security-sensitive operations
- The caller controls which URLs to fetch
- Enforcing HTTPS-only would break many legitimate use cases

**TM-NET-003 — Connection reuse (MITIGATED):**
The `DefaultFetcher` creates a new `reqwest::Client` per request, which prevents
connection pool state from leaking between requests. This is a defense-in-depth
measure.

**TM-NET-004 — Proxy environment variables (MITIGATED):**
Fetchkit disables ambient `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` handling by
default via `reqwest::ClientBuilder::no_proxy()`. Callers must opt in explicitly
via `ToolBuilder::respect_proxy_env(true)` or the CLI `--allow-env-proxy` flag.
This prevents inherited container proxy settings from silently bypassing the
expected outbound path.

## 3. Input Validation (TM-INPUT)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-INPUT-001 | Non-HTTP scheme (file://, ftp://, data:) | High | Explicit scheme check: only `http://` and `https://` prefixes allowed | MITIGATED |
| TM-INPUT-002 | URL prefix bypass via encoding | Medium | URL-aware prefix matching using parsed/normalized URL components | MITIGATED |
| TM-INPUT-003 | Empty or malformed URL | Low | Empty URL check and `url::Url::parse()` validation | MITIGATED |
| TM-INPUT-004 | Extremely long URL | Low | No explicit length limit; reqwest/OS handles | **ACCEPTED** |
| TM-INPUT-005 | URL with fragment/query manipulation | Low | Fragments and queries are part of the URL; no special handling needed | **BY DESIGN** |
| TM-INPUT-006 | Prefix bypass via URL authority (http://evil.com@127.0.0.1) | Medium | `url` crate parses authority correctly; resolve-then-check validates the actual host | MITIGATED |
| TM-INPUT-007 | Block prefix matching is string-based, not URL-aware | Medium | URL-aware prefix matching compares parsed components (scheme, host, path) | MITIGATED |
| TM-INPUT-008 | Symlink-based path traversal in LocalFileSaver | Medium | Save-time parent-directory walk rejects symlinks and re-checks canonical path under base_dir | MITIGATED |
| TM-INPUT-009 | LocalFileSaver without base_dir allows arbitrary writes | Medium | Documented limitation; callers should always set base_dir in untrusted contexts | **ACCEPTED** |
| TM-INPUT-010 | Invalid save destination triggers unnecessary network/body download | Low | Schema rejects blank strings; runtime calls `FileSaver::validate_path` before transport | MITIGATED |

### Mitigation Details

**TM-INPUT-001 — Scheme validation (MITIGATED):**
```rust
// THREAT[TM-INPUT-001]: Block non-HTTP schemes (file://, ftp://, data:, etc.)
// Mitigation: normalize domain-like bare hosts to https://, then reject any
// remaining URL without an explicit http:// or https:// scheme.
request.normalize_url_for_fetch()?;
```
Bare URL normalization is deliberately narrow: domain-like hosts such as
`example.com/docs` default to `https://example.com/docs`; non-web schemes,
protocol-relative URLs, bare IP literals, and single-label local names are rejected.
SSRF, DNS, port, allow-prefix, and block-prefix policies run after normalization
against the canonical URL that will be fetched.

**TM-INPUT-002 — URL prefix bypass via encoding (MITIGATED):**
Prefix matching now uses the `url` crate to parse both the URL and the prefix,
then compares normalized components (scheme, host, port, path). The `url` crate
normalizes scheme/host to lowercase, resolves punycode, and handles encoding.
This prevents bypasses via case variations, URL encoding, or trailing dots.

**TM-INPUT-006 — URL authority bypass (MITIGATED):**
URLs like `http://evil.com@127.0.0.1/path` have `127.0.0.1` as the host (with
`evil.com` as the username). The `url` crate correctly parses this, and
resolve-then-check validates the actual host's IP.

**TM-INPUT-007 — String-based prefix matching (MITIGATED):**
Prefix matching now parses both the URL and the prefix with the `url` crate,
then compares scheme, host (exact match), port, and path (segment-boundary
matching). `http://internal.example.com` correctly does NOT match
`http://internal.example.com.evil.com` since hosts differ after parsing.

**TM-INPUT-008 — Symlink-based path traversal (MITIGATED):**
`LocalFileSaver` still performs lexical normalization to block `..` traversal, but
save-time enforcement now walks each parent directory component under `base_dir`
using directory handles, rejects symlinks with no-follow opens, and opens the
final file relative to the verified parent with no-follow semantics.
`execute_with_saver()` also performs `validate_path()` preflight before transport;
the descriptor-anchored save-time checks remain authoritative against path changes.

**TM-INPUT-009 — No base_dir allows arbitrary writes (ACCEPTED):**
`LocalFileSaver::new(None)` only requires absolute paths, with no directory restriction.
Accepted because:
- This mode is for CLI/trusted contexts only
- The `FileSaver` trait allows custom implementations with stricter controls
- `enable_save_to_file` is disabled by default

**TM-INPUT-010 — Save destination preflight (MITIGATED):**
The input schema requires `save_to_file` to contain at least one non-whitespace
character. Runtime execution independently rejects blank destinations and calls
the configured `FileSaver::validate_path` before any HTTP request or body download.
`LocalFileSaver` preflight also rejects its configured base directory, root-like
destinations, and existing directories. Save-time no-follow checks remain in place
to protect against changes between validation and write.

## 4. Denial of Service (TM-DOS)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-DOS-001 | Unbounded response body | Medium | Configurable `max_body_size` (default 10 MB); truncates with `truncated: true` | MITIGATED |
| TM-DOS-002 | Slowloris / slow body | Low | 1-second first-byte timeout; 30-second body timeout | MITIGATED |
| TM-DOS-003 | Compressed content bomb (gzip bomb) | Medium | `max_body_size` enforced on decompressed stream; truncates large payloads | MITIGATED |
| TM-DOS-004 | Rapid request flooding via tool | Low | No rate limiting in Fetchkit; caller responsibility | **CALLER RISK** |
| TM-DOS-005 | DNS resolution delay | Low | DNS resolution uses system resolver; no explicit timeout on DNS lookup | **ACCEPTED** |
| TM-DOS-006 | Memory exhaustion from large HTML conversion | Medium | Conversion input bounded by `max_body_size` (10 MB default) | MITIGATED |

### Mitigation Details

**TM-DOS-001 — Unbounded response body (MITIGATED):**
Fetchkit enforces a configurable `max_body_size` (default 10 MB) during streaming
body reads. When the limit is reached, the response is truncated and
`truncated: true` is set in the response. The 30-second body timeout provides
additional protection. Configurable via `ToolBuilder::max_body_size()`.

**TM-DOS-002 — Slowloris (MITIGATED):**
The 1-second first-byte timeout prevents connections from being held open
indefinitely during the initial handshake. The 30-second body timeout provides
a hard ceiling on total request duration.

**TM-DOS-003 — Compressed content bomb (MITIGATED):**
The `max_body_size` limit is enforced on the decompressed stream (reqwest
decompresses transparently before returning chunks). A gzip bomb that
decompresses to a large size is caught by the same size limit that protects
against unbounded responses (TM-DOS-001).

## 5. Information Leakage (TM-LEAK)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-LEAK-001 | Error messages reveal internal network topology | Medium | Error messages include connect/timeout details but not resolved IPs | MITIGATED |
| TM-LEAK-002 | DNS resolution errors reveal internal DNS | Low | DNS errors surfaced as connect errors; hostname visible in error | **ACCEPTED** |
| TM-LEAK-003 | Response content leaks internal data | Low | Fetchkit returns content as-is; caller must filter sensitive data | **CALLER RISK** |
| TM-LEAK-004 | User-Agent reveals software version | Info | Default UA `Everruns Fetchkit/1.0` reveals stack; configurable | **BY DESIGN** |
| TM-LEAK-005 | Timing side-channels (connect time reveals network proximity) | Low | 1-second timeout masks some timing; not fully mitigated | **ACCEPTED** |

### Mitigation Details

**TM-LEAK-001 — Error message detail (MITIGATED):**
Fetchkit's error types (`FetchError`) use generic messages that don't include
resolved IP addresses or internal hostnames. Connect errors say "Failed to connect
to server" and the `from_reqwest()` fallback path classifies errors by type
(redirect, body, decode) instead of passing through raw reqwest error strings
which could contain hostnames or URL details.

## 6. Content Conversion (TM-CONV)

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-CONV-001 | Script injection in converted markdown | Low | `<script>` tags stripped during HTML-to-markdown conversion | MITIGATED |
| TM-CONV-002 | Excessive memory from deeply nested HTML | Medium | No recursion depth limit in HTML parser | **ACCEPTED** |
| TM-CONV-003 | Markdown injection (crafted HTML producing executable markdown) | Low | Fetchkit produces markdown text; execution depends on downstream consumer | **BY DESIGN** |
| TM-CONV-004 | Entity decoding producing unexpected characters | Low | Limited entity set decoded; no arbitrary numeric entity expansion | MITIGATED |

### Mitigation Details

**TM-CONV-001 — Script stripping (MITIGATED):**
The HTML converter skips content inside `script`, `style`, `noscript`, `iframe`,
and `svg` tags, preventing script injection into the converted output.

**TM-CONV-002 — Deeply nested HTML (ACCEPTED):**
The HTML parser is character-based and iterative (not recursive), so stack
overflow from deep nesting is unlikely. However, deeply nested structures could
produce large output. This is accepted as the body size limit (TM-DOS-001)
provides upstream protection.

## 7. Bot Authentication (TM-AUTH)

Feature-gated behind `bot-auth`. Only relevant when the feature is enabled.

| ID | Threat | Severity | Mitigation | Status |
|----|--------|----------|------------|--------|
| TM-AUTH-001 | Signing key held in process memory | Medium | Key is in-memory only; never serialized to disk or logs; lifetime scoped to process | **ACCEPTED** |
| TM-AUTH-002 | Signature replay by attacker | Low | Nonce (32 random bytes) + short validity window (default 300s) + `created`/`expires` timestamps | MITIGATED |
| TM-AUTH-003 | Signature scope too broad (covers wrong requests) | Low | Signature covers `@authority` (hostname); different hosts get different signatures | MITIGATED |
| TM-AUTH-004 | Weak key material from caller | Low | Ed25519 key derived from caller-provided seed; no validation of entropy; caller responsibility | **CALLER RISK** |
| TM-AUTH-005 | Signing failure blocks requests | Low | Signing errors are logged and the request proceeds unsigned; never causes fetch failure | MITIGATED |

### Mitigation Details

**TM-AUTH-001 — Key in process memory (ACCEPTED):**
The Ed25519 `SigningKey` lives in the `BotAuthConfig` struct for the process
lifetime. It is never written to disk, serialized, or included in error messages.
Accepted because any in-process secret has this property; operators should use
OS-level memory protections (e.g., encrypted swap, no core dumps) for sensitive
workloads.

**TM-AUTH-002 — Signature replay (MITIGATED):**
Each signature includes a cryptographically random 32-byte nonce, a `created`
timestamp, and an `expires` timestamp (default 5 minutes). Origins can reject
replayed signatures by checking nonce uniqueness and timestamp validity.

**TM-AUTH-003 — Signature scope (MITIGATED):**
The signature base includes `@authority` (the target hostname per RFC 9421).
Signatures for `example.com` are not valid for `other.com`. Optionally,
`signature-agent` is also covered when configured.

## Vulnerability Summary

### Open Threats (Require Action)

None — all previously open threats have been mitigated.

### Recently Mitigated (previously open)

| ID | Threat | Severity | Mitigation |
|----|--------|----------|------------|
| TM-SSRF-010 | Redirect to internal resource | High | Manual redirect following with IP validation per hop |
| TM-INPUT-002 | URL prefix bypass via encoding | Medium | URL-aware prefix matching via parsed components |
| TM-INPUT-007 | String-based prefix matching | Medium | URL-aware prefix matching with host/path comparison |
| TM-DOS-001 | Unbounded response body | Medium | Configurable `max_body_size` (default 10 MB) |
| TM-DOS-003 | Compressed content bomb | Medium | Size limit enforced on decompressed stream |
| TM-DOS-006 | Memory exhaustion from HTML conversion | Medium | Conversion input bounded by `max_body_size` |

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
| TM-CONV-002 | Deep HTML nesting | Iterative parser; upstream size limits |
| TM-AUTH-001 | Signing key in memory | Same as any in-process secret; OS protections apply |

### Caller Responsibilities

| Responsibility | Related Threats | Description |
|---------------|----------------|-------------|
| Rate limiting | TM-DOS-004 | Caller must implement request rate limits |
| Proxy config | TM-NET-004 | Opt in with `respect_proxy_env(true)` only when an explicit proxy is required |
| Content filtering | TM-LEAK-003 | Filter sensitive data from responses |
| URL allow-listing | TM-INPUT-002, TM-INPUT-007 | Use allow_prefixes for positive security model (now URL-aware) |
| Network isolation | TM-SSRF, TM-NET | Route Fetchkit through dedicated egress controls; library checks are defense in depth |
| Key entropy | TM-AUTH-004 | Provide high-entropy Ed25519 seeds; library does not validate seed randomness |

## Security Controls Matrix

| Control | Category | Implementation |
|---------|----------|---------------|
| Scheme validation | TM-INPUT | `starts_with("http://")` check; also enforced at each redirect hop |
| URL prefix allow/block | TM-INPUT | URL-aware prefix matching via parsed URL components |
| Hostname block rules | TM-INPUT | Exact host and suffix checks before DNS resolution |
| Port allow-listing | TM-INPUT | Optional port restrictions validated before connect and on redirects |
| Private IP blocking | TM-SSRF | `DnsPolicy::block_private_ips()` with resolve-then-check |
| DNS pinning | TM-SSRF | `reqwest::ClientBuilder::resolve()` per redirect hop |
| IPv6-mapped-IPv4 canonicalization | TM-SSRF | `IpAddr::to_canonical()` before range check |
| IPv4-compatible/6to4 extraction | TM-SSRF | Extract embedded IPv4 from `::` and `2002::` prefixes, validate |
| Manual redirect following | TM-SSRF | `Policy::none()` with IP validation at each hop |
| Ambient proxy suppression | TM-NET | `reqwest::ClientBuilder::no_proxy()` unless caller opts in |
| Same-host redirect hardening | TM-NET | Optional `same_host_redirects_only(true)` for hardened deployments |
| First-byte timeout | TM-DOS | 1-second connect+response timeout |
| Body timeout | TM-DOS | 30-second streaming body timeout |
| Body size limit | TM-DOS | Configurable `max_body_size` (default 10 MB) |
| Script tag stripping | TM-CONV | Skip `script`/`style`/`noscript`/`iframe`/`svg` |
| Binary detection | TM-CONV | Content-Type prefix matching |
| New client per request | TM-NET | No connection pool state leakage |
| Pluggable transport keeps policy in fetchkit | TM-SSRF, TM-NET | `HttpTransport` only performs the socket send; DNS policy (resolve-then-check), redirect following, and proxy suppression stay in fetchkit. `TransportRequest.pinned_addrs` (from `DnsPolicy`) constrains a custom transport to fetchkit-validated addresses; the default `ReqwestTransport` enforces this via `resolve_to_addrs()` |
| Fetcher API URL hardcoding | TM-SSRF | Specialized fetchers (GitHub, Twitter) connect to hardcoded API hosts, not user-controlled URLs; DNS validation applied on initial connect |
| Proxy env isolation | TM-NET | `reqwest::ClientBuilder::no_proxy()` by default |
| Path traversal prevention | TM-INPUT | Preflight destination validation, lexical normalization, and save-time parent-directory symlink rejection in `LocalFileSaver` |
| Save feature gating | TM-INPUT | `enable_save_to_file` disabled by default; schema gated |
| Bot-auth feature gating | TM-AUTH | `bot-auth` Cargo feature disabled by default; no crypto deps unless opted in |
| Rendered fetch feature gating | TM-SSRF, TM-NET, TM-DOS | Browser-rendered fetching disabled by default; lightweight rakers-style rendering must be gated by `render-rakers`, require an explicit request/config switch, apply fetchkit URL, DNS, proxy, timeout, and body-size policy to the initial page, re-apply body-size policy to rendered HTML before conversion, and deny rakers-initiated subresource network requests unless they can be routed through fetchkit policy |
| Signature nonce + timestamps | TM-AUTH | 32-byte random nonce + created/expires per signature prevents replay |
| Authority-scoped signatures | TM-AUTH | Signature covers `@authority`; per-host binding |
| Graceful signing failure | TM-AUTH | Signing errors logged, request proceeds unsigned |

## References

- `specs/initial.md` — Fetchkit tool specification
- `specs/fetchers.md` — Pluggable fetcher system
- [OWASP SSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html)
- [CWE-918: Server-Side Request Forgery](https://cwe.mitre.org/data/definitions/918.html)

### TM-DISC-001: Agent discovery request amplification

**Threat**: A single fetch triggers unbounded secondary requests or recursive
resource traversal.

**Mitigations**:
- Probe only a fixed, documented path set on the final response origin.
- Never recursively follow discovered resources.
- Bound concurrency, per-probe timeout, and emitted resource count.
- Treat discovery failure as non-fatal.

**Verification**: Tests assert fixed-path probing, deduplication, and bounded
resource output.

### TM-DISC-002: Discovery bypasses network policy

**Threat**: Advertised or conventional resources reach private networks, abuse
redirects, or receive credentials intended for another origin.

**Mitigations**:
- Route probes through the normal DNS, IP, redirect, proxy, and signing policy.
- Probe fixed same-origin URLs only and reject redirected probes as verified.
- Do not forward request-specific authorization headers.
- Report arbitrary advertised links without fetching them.

**Verification**: Discovery uses the shared request transport and tests use a
private-address policy override explicitly.
