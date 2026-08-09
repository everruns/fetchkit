---
type: Interface Contract
title: Agent Resource Discovery
description: Bounded discovery and reporting of same-origin resources intended for AI agents.
tags:
  - fetchkit
  - agents
  - discovery
---

# Agent Resource Discovery

## Abstract

FetchKit enriches regular fetches with bounded discovery of resources intended
for AI agents. It reads explicit HTTP and HTML advertisements, probes a fixed
set of conventional same-origin paths, exposes typed metadata, and adds compact
navigation links to Markdown output.

## Requirements

1. Regular `GET` fetches MUST inspect all final-response `Link` header fields.
2. HTML fetches MUST inspect agent-relevant `<link>` and `<meta>` declarations.
3. Markdown `GET` fetches MUST probe `/llms.txt`, `/llms-full.txt`, `/auth.md`, and the
   documented fixed set of relevant `/.well-known/` paths on the final origin.
4. FetchKit MUST NOT enumerate arbitrary `/.well-known/` paths or recursively
   fetch discovered resources.
5. Probes MUST use existing SSRF, DNS, redirect, proxy, timeout, and request
   signing controls.
6. Probe concurrency, timeout, and result count MUST be bounded.
7. Failed discovery MUST NOT fail the requested page fetch.
8. Resources MUST be normalized, classified, deduplicated, and returned as
   structured page metadata with source and verification state.
9. Markdown responses MUST end with a compact `Agent resources` section when
   at least one resource is found. Raw HTML MUST NOT be modified.
10. Advertised resources MUST NOT be described as verified unless FetchKit
    requested and validated that exact resource.
11. Discovery MUST NOT invoke APIs, authorization flows, payment protocols, or
    agent capabilities.

## See also

- [Fetcher System](../foundations/fetchers.md) — transport and URL policy used by discovery probes
- [Threat Model](../security/threat-model.md) — discovery amplification and network-policy threats
