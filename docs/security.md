# Security Notes

FetchKit is intended to run in agent, server, and cluster environments where URL input may be
user-controlled.

## Safe Defaults

- Private and reserved IP ranges are blocked by default via resolve-then-check DNS validation.
- Redirects are followed manually so every hop is revalidated.
- Textual response bodies are capped at 10 MB after decompression by default. Larger responses are truncated
  and marked with `truncated: true`.
- `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` are ignored by default.

## Multi-Tenant Deployment

For shared VMs, containers, or clusters:

- Keep private-IP blocking enabled.
- Keep proxy inheritance disabled unless outbound traffic must traverse a trusted proxy.
- Use allow-lists where possible instead of relying only on block-lists.
- Apply caller-side rate limits and concurrency limits around FetchKit.

If you need different limits, configure them through `ToolBuilder`:

```rust
use fetchkit::ToolBuilder;

let tool = ToolBuilder::new()
    .max_body_size(1024 * 1024)
    .respect_proxy_env(false)
    .build();
```

See [`specs/threat-model.md`](../specs/threat-model.md) for the full threat inventory.
