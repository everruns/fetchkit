# Fetchkit Knowledge Update Log

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
