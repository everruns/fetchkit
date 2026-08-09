---
type: Playbook
title: Knowledge Maintenance Contract
description: Rules for maintaining the Fetchkit knowledge bundle and its OKF conformance.
tags:
  - fetchkit
  - knowledge
  - okf
  - process
---

# Knowledge Maintenance Contract

`knowledge/` is Fetchkit's canonical [Open Knowledge Format (OKF) v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md) bundle and persistent project memory.

## Maintenance rules

- Treat this knowledge as part of the implementation, not as historical documentation.
- Before changing behavior, read the relevant concepts and follow their decisions or update them in the same change.
- When code changes a documented behavior, design decision, invariant, limitation, threat, test strategy, or operational process, update the affected knowledge in the same pull request.
- Record important decisions that are not recoverable from code. Prefer links to source and tests over duplicating volatile implementation details.
- Keep stable identifiers such as `TM-*` and `R-*`; never renumber them.
- Add durable engineering knowledge here. User-facing guides remain in `docs/`.

## OKF conformance rules

The bundle targets OKF v0.2, declared as `okf_version: "0.2"` in the bundle-root [index](index.md).

- Every Markdown file except reserved `index.md` and `log.md` files is a concept and starts with YAML frontmatter containing a non-empty `type`.
- Concepts also carry `title`, a single-line `description`, and useful `tags`.
- Directory indexes contain link lists for concepts and immediate subdirectories only.
- The update log uses `## YYYY-MM-DD` headings, newest first.
- Links between concepts are relative and resolve inside the bundle.
- Every concept links to another concept so agents can traverse the bundle as a graph.
- Reference bundle documents with relative Markdown links, not repository-path text that can silently rot.

OKF provenance, trust, lifecycle, and attestation metadata remain optional. If generated concepts are added, they must identify their `resource` and `generated.by` actor so readers can distinguish generated facts from hand-maintained knowledge.

## Layout

| Directory | Holds |
|---|---|
| [foundations/](foundations/) | Tool behavior and fetcher architecture |
| [integrations/](integrations/) | Agent-facing integration contracts |
| [security/](security/) | Threat model and authentication design |
| [operations/](operations/) | Maintenance and release playbooks |

## Enforcement

Run both checks after changing the bundle:

```console
$ python3 scripts/check_okf.py knowledge
knowledge: OKF v0.2 conformant (8 concepts, 5 index files, 1 log file)
$ okf-lint knowledge --max-line-length 10000
```

The upstream linter enforces OKF v0.2. The local checker adds bundle conventions the format intentionally leaves soft: complete indexes, resolvable graph links, required descriptions, and generated-resource metadata. CI pins `okf-lint` to a reviewed version.

## See also

- [Periodic Maintenance](operations/maintenance.md) — broader repository drift and health checks
- [Fetchkit Tool Contract](foundations/tool-contract.md) — primary behavior contract maintained in this bundle
