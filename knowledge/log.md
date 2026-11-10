# Fetchkit Knowledge Update Log

## 2026-11-10

* **Release 0.7.2**: patch release of the linear readable-extraction fix (TM-DOS-009) so bashkit can pick it up; no public API change.

## 2026-11-07

* **Linear readable extraction**: a bashkit security audit found `--content-focus readable`/`agent` quadratic on `'<article>' * N + '</article>'` (each nested open rescanned the rest of the body, and each nested candidate re-scored the same text). Closes are matched in one stack pass, searches resume from cached positions, and scored bytes are capped at 8x the document. See [Threat Model](security/threat-model.md), TM-DOS-009.

## 2026-11-03

* **Release 0.7.1**: patch release of the dependency maintenance pass (Markkit 0.3.1, pdf-inspector 1.25); no public API change.

* **Maintenance**: Markkit minimum 0.3.1; `pdf-inspector` moves to the 1.x line (1.25, `lopdf` 0.45) keeping the nested-object bound; quick-xml 0.42 (string-typed names) for the deprecated `office` converter and DOCX table limits; calamine 0.38, ed25519-dalek 3, base64 0.23, pyo3 0.30. CI actions on Node 24 majors.

## 2026-11-01

* **Release 0.7.0**: ships the embeddable CLI library for bashkit's `fetchkit` builtin; Markkit 0.3 so bashkit links one Markkit. Minor bump for the `reqwest-tls` feature split.

* **Embeddable CLI**: The CLI moved into the `fetchkit_cli` library (`run(args, &dyn CliHost)`) so bashkit can ship a `fetchkit` builtin with the identical interface. Hosts own stdio, env, MCP, and egress, and can refuse host-only flags. reqwest TLS/compression moved behind the default `reqwest-tls` feature; clap default features off; Markkit 0.2.

## 2026-10-27

* **Social media**: Add public TikTok/Vimeo/SoundCloud oEmbed metadata fetching with fixed endpoints, policy enforcement, bounded output and explicit media limits. Extend YouTube URL matching to Shorts/live/embeds and validate IDs before API query construction.

## 2026-10-25

* **Publishing authentication**: Bind both Rust crates to GitHub OIDC in `publish.yml`; publishing jobs use short-lived tokens from a pinned official auth action, with no legacy secret fallback. Document recovery from current main with the original immutable release source tag and actual registry consumer verification.

## 2026-10-23

* **Release 0.6.0**: Replace the temporary Markkit git override with registry minimum 0.1.1; require Tokio 1.54.1 for RUSTSEC-2026-0386. Publish both Rust crates with `--locked`. The minor bump reflects public `FetchOptions` changes already accumulated since 0.5.0; see the changelog migration notes.

## 2026-10-22

* **Parser security**: Require PDF inspector 0.1.8 with bounded nested-object parsing and preserve ZIP 9 Office lookup compatibility. The exact patched Markkit source revision is temporary until its registry release; Fetchkit publication must first replace the root-only override with a patched registry minimum.

## 2026-10-13

* **Credential redaction**: `DocumentConversionSettings` now redacts present Cloudflare tokens in compact/pretty `Debug`, including clones and after build, independently of backend features. Public string storage remains unchanged; explicit field logging and downstream diagnostic access remain caller responsibilities (TM-LEAK-007).

## 2026-10-12

* **DOCX table budgets**: Both local backends now admit table dimensions and padded cell work before conversion; the deprecated renderer additionally admits exact Markdown bytes before allocation. Tiny offline fixtures cover limits, escaping and normal output under TM-DOS-008.

* **Office backend**: `markkit` (`MarkkitConverter`, `docs-markkit`, default) is now the Office converter for DOCX/XLSX/XLS/ODS; the in-crate `office` converter is deprecated, off by default, excluded from `docs-all`, and remains the only PPTX/XLSB backend when enabled. TM-DOS-008 now describes markkit's limits.
* **Document conversion**: Replaced the PDF and Office processors with one `DocumentProcessor` running a configurable, per-format `DocumentConverter` chain; added feature-gated converters (`docs-pdf`, `docs-office`, `docs-cloudflare`, `docs-http`), a separate 25 MB document cap, remote-converter opt-in, and threats TM-LEAK-006/007.
* **Office documents**: Added the local Office processor (DOCX, PPTX, XLSX, XLS, ODS) to the fetchers and tool contract, its zip-bomb threat TM-DOS-008, and the Markdown-first `Accept` header.

## 2026-08-08

* **HTML processing**: Routed HTML extraction and Markdown/text conversion through the content processor registry, added a replaceable native converter, expanded structural fidelity for code, tables, footnotes, math, callouts, and related rich content, and bounded nested conversion depth.
* **Migration**: Replaced the unindexed specification directory with an OKF v0.2 bundle organized by foundations, integrations, security, and operations.
* **Process**: Added a maintenance contract and automated conformance checks so durable engineering knowledge changes with the implementation.
