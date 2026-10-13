# fetchkit Hardening Guide

This guide is for operators running `fetchkit` in shared clusters, AI agent data planes, or other environments where untrusted users can influence fetched URLs.

## What fetchkit can enforce

`fetchkit` can harden its own outbound fetch path:

- blocks private and reserved IP ranges by default
- revalidates every redirect hop
- pins DNS to the validated IP to reduce DNS rebinding risk
- ignores `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` by default
- can block internal hostnames before DNS resolution
- can restrict outbound traffic to an allowed port set
- can restrict redirects to the original host only

Use the hardened profile for cluster-facing deployments:

```rust
use fetchkit::{FetchRequest, Tool};

let tool = Tool::builder()
    .hardened()
    .allow_prefix("https://docs.example.com")
    .build();

let response = tool
    .execute(FetchRequest::new("https://docs.example.com").as_markdown())
    .await?;
```

The hardened profile does all of this:

- keeps private IP blocking enabled
- ignores ambient proxy environment variables
- allows only ports `80` and `443`
- blocks `localhost`, `.local`, `.internal`, `.svc`, and `.cluster.local`
- only follows same-host redirects

CLI and MCP equivalents:

```bash
fetchkit fetch https://example.com --hardened
fetchkit mcp --hardened
```

## What fetchkit cannot guarantee

`fetchkit` cannot guarantee that the pod, container, or VM itself has no internal network reachability. If the runtime can open arbitrary sockets to internal services, an infrastructure mistake can still expose that path outside of `fetchkit`.

Treat library checks as defense in depth, not the only boundary.

## Recommended deployment pattern

For cluster deployments, use both application policy and network policy:

1. Run `fetchkit` in a dedicated namespace or workload class.
2. Deny direct egress from `fetchkit` except to DNS and a dedicated egress proxy.
3. Make the egress proxy the only component allowed to reach the public Internet.
4. Block RFC1918 ranges, cluster pod/service CIDRs, link-local ranges, loopback, and metadata endpoints at the proxy or network layer.
5. Keep `fetchkit` hardening enabled inside the application.

This gives you two independent checks:

- `fetchkit` rejects obviously unsafe targets before dialing.
- the network path still cannot reach internal addresses if application policy is bypassed or misconfigured.

## Proxy guidance

By default, `fetchkit` ignores `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY`. This is intentional. In cluster environments, inherited proxy variables can silently route requests around your expected enforcement path.

Only opt in to proxy environment variables if that proxy is part of your intended design:

```rust
let tool = Tool::builder()
    .hardened()
    .respect_proxy_env(true)
    .build();
```

```bash
fetchkit fetch https://example.com --hardened --allow-env-proxy
```

If you need a proxy, prefer a dedicated egress proxy with explicit policy over ambient proxy settings that every process inherits.

## Recommended app policy

For Internet-facing fetching from untrusted input:

- keep `block_private_ips(true)`
- use `.hardened()`
- add `allow_prefix(...)` if you know the domains ahead of time
- keep `same_host_redirects_only(true)` unless cross-host redirects are required
- only opt in to `respect_proxy_env(true)` when the proxy is deliberate and hardened

If you must fetch nonstandard ports or internal-looking public domains, start from `.hardened()` and then add the narrowest exception you need.

## Document conversion

Both built-in DOCX backends reject tables exceeding 1,000 rows, 50 columns or
50,000 padded cells. The default markkit backend preflights archive parts before
conversion, including merged-cell spans and nested tables; its cell-work budget
is cumulative within each XML part. These DOCX limits cannot be raised through
markkit's spreadsheet settings. Archive preflight honors tighter markkit archive
settings and caps entry count and inflated sizes at the defaults.

The deprecated `office` backend also rejects DOCX output over 10 MiB before
allocating escaped or padded Markdown. Smaller `max_body_size` settings still
truncate the returned output. Limits produce a converter failure; another
configured backend may handle the document. Normal table padding and escaping
are preserved. These bounds address table expansion; they are not a process
memory limit for all backend parsing or text rendering.

PDF and Office documents are converted by a chain of document converters. The
default converters (`pdf-inspector`, `markkit`) run in-process and make no network
requests; markkit is built without its HTTP client. Remote converters
(`CloudflareConverter`, `HttpConverter`) upload the downloaded document to
another service:

- They run only when `DocumentConversion::allow_remote(true)` (CLI:
  `--allow-remote-conversion`) is set, and never under `ToolBuilder::hardened()`.
- Their endpoint is operator configuration; fetched content can never choose it.
- Route uploads through your egress boundary by passing your own
  `ConverterClient`, or enable proxy environment variables for them with
  `ReqwestConverterClient::respect_proxy_env(true)`.
- Documents are downloaded up to `max_document_bytes` (25 MB default), separate
  from `max_body_size`. Lower it, or restrict `formats`, for tighter budgets.
- Local parsing of hostile documents is CPU-bound; isolate the process and apply
  outer CPU and memory limits for high-volume untrusted input (TM-DOS-007/008).
