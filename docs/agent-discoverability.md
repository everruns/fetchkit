# Agent resource discovery

FetchKit enriches regular `GET` responses with advertised resources that help
an agent navigate a site. Markdown `GET` requests additionally probe a bounded
set of conventional resources. Discovery is shallow and descriptive: FetchKit
reports resources but never invokes an advertised API, authentication flow, or
agent endpoint.

## Discovery sources

FetchKit combines these sources:

- HTTP `Link` response headers with agent-relevant relations and Markdown, JSON,
  or conventional agent-resource targets.
- HTML `<link>` declarations using `alternate`, `service-desc`, `describedby`,
  `authorization_endpoint`, `mcp`, `a2a`, `agent-card`, or `skill` relations.
- HTML `<meta name="..." content="URL">` declarations named `llms`,
  `llms-full`, `auth`, `service-desc`, `api-catalog`, `mcp`, `a2a`,
  `agent-card`, or `agent-skills`.
- A fixed set of conventional same-origin probes:

  ```text
  /llms.txt
  /llms-full.txt
  /auth.md
  /.well-known/oauth-authorization-server
  /.well-known/openid-configuration
  /.well-known/oauth-protected-resource
  /.well-known/api-catalog
  /.well-known/mcp/server-card.json
  /.well-known/agent-card.json
  /.well-known/agent-skills/index.json
  ```

The list is intentionally explicit. FetchKit does not enumerate the unbounded
`/.well-known/` namespace. Probe requests use `HEAD`, run with bounded
concurrency and a short timeout, and do not recurse into discovered resources.
A probed resource is accepted only after a direct `2xx` response; redirects are
not accepted as verification.

## Output

Resources are returned as `PageMetadata.agent_resources`. Each resource has:

- `url`: normalized absolute URL
- `kind`: stable category such as `llms-txt`, `auth`, `mcp`, or `oauth`
- `source`: `http-link`, `html-link`, `metadata`, or `probe`
- optional `relation`, `media_type`, and `title`
- `verified`: whether a conventional probe confirmed the URL

Resources advertised by headers or HTML are marked unverified because FetchKit
does not issue an additional request merely to validate each arbitrary target.
Duplicates are removed and output is capped at 20 resources.

For Markdown requests, FetchKit also appends an `Agent resources` section to the
returned document. Raw HTML is not modified; consumers can use the structured
metadata instead.

## Security and operational behavior

All probes use the same URL validation, DNS/IP policy, redirect validation,
proxy policy, and Web Bot Authentication transport rules as the original
request. Probes are restricted to fixed paths on the final response origin and
do not forward request-specific authorization headers.

Discovery adds up to ten lightweight requests to a Markdown `GET`. Servers that
support `HEAD` can make these inexpensive. Failed, blocked, redirected, or timed
out probes are omitted without failing the requested page fetch.

## Publishing resources for agents

Sites get the strongest result by explicitly advertising resources:

```http
Link: </llms.txt>; rel="alternate"; type="text/markdown"; title="LLM index"
Link: </openapi.json>; rel="service-desc"; type="application/openapi+json"
```

or in HTML:

```html
<link rel="alternate" type="text/markdown" href="/llms.txt" title="LLM index">
<link rel="service-desc" type="application/openapi+json" href="/openapi.json">
```

Conventional resources should return an accurate status and content type for
`HEAD`. Avoid catch-all `200 OK` responses for nonexistent paths, because they
make protocol-level discovery ambiguous.
