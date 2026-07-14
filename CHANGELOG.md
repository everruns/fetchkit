# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Optional `render-rakers` feature for explicit, per-request lightweight JS/DOM rendering before markdown/text conversion. The backend is disabled by default, exposed only when a host enables it, and denies rakers-initiated subresource network requests.

### Fixed

- Stack Overflow question fetching now returns question and answer content again, including code snippets.
- Invalid `save_to_file` destinations are rejected before any HTTP request or body download.

## [0.4.1] - 2026-07-04

### Highlights

- Bare host inputs such as `example.com` are now accepted and normalized before fetching.
- URL handling is more consistent across direct and specialized fetcher paths, including docs pages.
- Vulnerable Rust dependencies were updated.

### What's Changed

* fix: normalize direct fetcher URLs ([#141](https://github.com/everruns/fetchkit/pull/141))
* fix(fetcher): respect docs page URLs ([#140](https://github.com/everruns/fetchkit/pull/140))
* fix(deps): update vulnerable Rust dependencies ([#139](https://github.com/everruns/fetchkit/pull/139))
* fix: accept bare host URLs ([#138](https://github.com/everruns/fetchkit/pull/138))

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.4.0...v0.4.1

## [0.4.0] - 2026-06-12

### Highlights

- Pluggable HTTP transport: host applications can route all of fetchkit's outbound HTTP through their own egress boundary via `FetchOptions::transport` or `ToolBuilder::transport`, while fetchkit retains URL validation, DNS policy (resolve-then-check with pinned addresses), manual redirect following, bot-auth signing, and body-size/timeout caps. Default behavior is unchanged (`ReqwestTransport`).
- Policy hardening alongside the transport refactor: YouTube and HackerNews API hosts are now DNS-pinned, Wikipedia/package-registry/arXiv/HackerNews redirects are validated per hop instead of delegated to reqwest, and specialized-fetcher JSON reads are capped at `max_body_size`.
- Local file saver symlink handling hardened.

### Breaking Changes

- `FetchOptions` gained a public `transport` field. Code constructing `FetchOptions` with an exhaustive struct literal must add the field or switch to `..Default::default()`. No behavior change when `transport` is `None`.
- `FetchOptions`, `Tool`, and `ToolBuilder` now have manual `Debug` impls (the transport renders as `"<custom>"` or `"None"`); derived-`Debug` output formats changed accordingly.

### What's Changed

* feat(transport): pluggable HttpTransport abstraction ([#135](https://github.com/everruns/fetchkit/pull/135))
* fix(fetchkit): harden local file saver symlink handling ([f857737](https://github.com/everruns/fetchkit/commit/f857737))

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.3.0...v0.4.0

## [0.3.0] - 2026-05-18

### Highlights

- Hardened outbound fetch policy across every built-in fetcher: SSRF safeguards, body-size caps, redirect re-validation, and symlink-escape protection
- Bot-auth headers are re-signed on every redirect hop so authenticated fetches survive policy-validated hops
- Enhanced fetchers: YouTube transcript extraction, Wikipedia redirect resolution, HackerNews timestamp display, ArXiv PDF binary indication, RSS content-type detection with HTML-to-Markdown
- Tool JSON contract now exposes conditional-fetch and quality fields (word count, redirect chain, paywall) in the output schema
- Dependency refresh including major bumps for the optional `bot-auth` feature (`sha2` 0.11, `rand` 0.10)

### What's Changed

* fix(cli): quote YAML frontmatter scalar values ([#132](https://github.com/everruns/fetchkit/pull/132))
* fix(fetchers): enforce policy for GitHub API subrequests ([#131](https://github.com/everruns/fetchkit/pull/131))
* fix(ci): avoid shell interpolation of release tags ([#130](https://github.com/everruns/fetchkit/pull/130))
* fix(tool): align JSON contract with conditional fetch fields ([#129](https://github.com/everruns/fetchkit/pull/129))
* fix(tool): include quality fields in output schema ([#128](https://github.com/everruns/fetchkit/pull/128))
* fix(ci): pin maturin version in python workflow ([#127](https://github.com/everruns/fetchkit/pull/127))
* fix(docs): avoid printing GITHUB_TOKEN in cloud quickcheck ([#126](https://github.com/everruns/fetchkit/pull/126))
* fix(fetchkit): tighten content-type checks for markdown and text ([#125](https://github.com/everruns/fetchkit/pull/125))
* fix(fetchers): enforce twitter fetch hardening limits ([#124](https://github.com/everruns/fetchkit/pull/124))
* fix(fetchkit): re-sign bot-auth headers on redirect hops ([#123](https://github.com/everruns/fetchkit/pull/123))
* fix(fetchers): enforce policy on GitHub API redirect target ([#122](https://github.com/everruns/fetchkit/pull/122))
* fix(fetchers): enforce max_body_size in GitHub issue fetcher ([#121](https://github.com/everruns/fetchkit/pull/121))
* fix(fetchers): enforce SSRF safeguards in StackOverflow API fetcher ([#120](https://github.com/everruns/fetchkit/pull/120))
* fix(fetchers): enforce body size limits for registry JSON ([#119](https://github.com/everruns/fetchkit/pull/119))
* fix(fetchers): enforce body size limits in wikipedia fetcher ([#118](https://github.com/everruns/fetchkit/pull/118))
* fix(fetchers): enforce fetch options on YouTube secondary requests ([#117](https://github.com/everruns/fetchkit/pull/117))
* fix(fetchers): harden arxiv fetcher input and body limits ([#116](https://github.com/everruns/fetchkit/pull/116))
* fix(fetchers): avoid utf-8 panic in hn html stripping ([#115](https://github.com/everruns/fetchkit/pull/115))
* fix(convert): avoid unicode offset panic in attribute extraction ([#114](https://github.com/everruns/fetchkit/pull/114))
* fix(client): cap batch fetch concurrency ([#112](https://github.com/everruns/fetchkit/pull/112))
* fix(ci): bind publish workflow to release tag ([#111](https://github.com/everruns/fetchkit/pull/111))
* fix(fetchers): harden youtube transcript handling ([#110](https://github.com/everruns/fetchkit/pull/110))
* fix(fetchers): bound HN timestamp formatting ([#109](https://github.com/everruns/fetchkit/pull/109))
* fix(ci): pin publish workflow actions in secret-bearing jobs ([#108](https://github.com/everruns/fetchkit/pull/108))
* fix(fetchers): enforce RSS body size and timeout limits ([#107](https://github.com/everruns/fetchkit/pull/107))
* chore(deps): apply available major bumps (sha2 0.11, rand 0.10) and tighten maintenance spec ([#106](https://github.com/everruns/fetchkit/pull/106))
* chore: periodic maintenance — deps refresh and spec/doc alignment ([#105](https://github.com/everruns/fetchkit/pull/105))
* fix(fetchers): surface malformed body errors ([#104](https://github.com/everruns/fetchkit/pull/104))
* fix(file-saver): block symlink escapes on save ([#103](https://github.com/everruns/fetchkit/pull/103))
* fix(python): preserve hardened redirect policy ([#102](https://github.com/everruns/fetchkit/pull/102))
* fix(fetchers): cap direct llms bodies ([#101](https://github.com/everruns/fetchkit/pull/101))
* fix(fetchers): enforce docs site outbound policy ([#100](https://github.com/everruns/fetchkit/pull/100))
* fix(fetchers): enforce rss feed outbound policy ([#99](https://github.com/everruns/fetchkit/pull/99))
* docs(readme): list built-in fetchers ([#92](https://github.com/everruns/fetchkit/pull/92))
* feat(fetchers): enhance RSSFeedFetcher with content-type detection and html_to_markdown ([#91](https://github.com/everruns/fetchkit/pull/91))
* feat(fetchers): enhance HackerNewsFetcher with timestamp display ([#90](https://github.com/everruns/fetchkit/pull/90))
* feat(fetchers): enhance ArXivFetcher with PDF binary indication ([#89](https://github.com/everruns/fetchkit/pull/89))
* feat(fetchers): enhance YouTubeFetcher with transcript extraction ([#88](https://github.com/everruns/fetchkit/pull/88))
* feat(fetchers): enhance WikipediaFetcher with redirect resolution ([#87](https://github.com/everruns/fetchkit/pull/87))
* fix(ci): trigger publish workflow explicitly from release ([#86](https://github.com/everruns/fetchkit/pull/86))

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.2.0...v0.3.0

## [0.2.0] - 2026-03-27

### Highlights

- Pluggable fetchers for GitHub, Wikipedia, YouTube, ArXiv, StackOverflow, HackerNews, RSS, package registries, docs sites, and Twitter
- Batch fetching for concurrent multi-URL requests
- Content-focused extraction with boilerplate stripping and structured metadata
- Conditional fetching with ETag and If-Modified-Since support
- Improved HTML-to-Markdown conversion quality
- Content quality signals: word count, redirect chain, paywall detection
- Optional Web Bot Authentication support
- Hardened outbound fetch policy with proxy isolation and SSRF mitigations
- Live integration test suite behind feature flag

### Breaking Changes

- Ambient proxy environment variables are now ignored by default; set them explicitly if needed

### What's Changed

* test(fetchers): add live integration tests behind feature flag ([#84](https://github.com/everruns/fetchkit/pull/84))
* chore: periodic maintenance — deps update and spec sync ([#83](https://github.com/everruns/fetchkit/pull/83))
* feat(fetch): add content quality signals (word_count, redirect_chain, is_paywall) ([#82](https://github.com/everruns/fetchkit/pull/82))
* feat(client): add batch_fetch for concurrent multi-URL fetching ([#81](https://github.com/everruns/fetchkit/pull/81))
* feat(fetch): add conditional fetching with ETag and If-Modified-Since ([#80](https://github.com/everruns/fetchkit/pull/80))
* feat(convert): improve HTML-to-Markdown conversion quality ([#79](https://github.com/everruns/fetchkit/pull/79))
* feat(convert): add content-focused extraction with boilerplate stripping ([#78](https://github.com/everruns/fetchkit/pull/78))
* feat(convert): add structured metadata extraction from HTML pages ([#77](https://github.com/everruns/fetchkit/pull/77))
* feat(fetchers): add RSSFeedFetcher for structured feed parsing ([#70](https://github.com/everruns/fetchkit/pull/70))
* feat(fetchers): add HackerNewsFetcher for structured thread extraction ([#69](https://github.com/everruns/fetchkit/pull/69))
* feat(fetchers): add ArXivFetcher for paper metadata and abstract ([#68](https://github.com/everruns/fetchkit/pull/68))
* feat(fetchers): add YouTubeFetcher for video metadata extraction ([#67](https://github.com/everruns/fetchkit/pull/67))
* feat(fetchers): add WikipediaFetcher for article extraction ([#66](https://github.com/everruns/fetchkit/pull/66))
* feat(fetchers): add PackageRegistryFetcher for PyPI, crates.io, npm ([#65](https://github.com/everruns/fetchkit/pull/65))
* feat(fetchers): add StackOverflowFetcher for clean Q&A extraction ([#64](https://github.com/everruns/fetchkit/pull/64))
* feat(fetchers): add DocsSiteFetcher with llms.txt support ([#63](https://github.com/everruns/fetchkit/pull/63))
* feat(fetchers): add GitHubCodeFetcher for source file fetching ([#62](https://github.com/everruns/fetchkit/pull/62))
* feat(fetchers): add GitHubIssueFetcher for structured issue/PR fetching ([#61](https://github.com/everruns/fetchkit/pull/61))
* feat: add process-issues skill for e2e GitHub issue resolution ([#60](https://github.com/everruns/fetchkit/pull/60))
* feat: add optional Web Bot Authentication support ([#49](https://github.com/everruns/fetchkit/pull/49))
* feat(fetchers): add TwitterFetcher for tweet URL handling ([#47](https://github.com/everruns/fetchkit/pull/47))
* feat: skip HTML conversion for non-HTML responses ([#48](https://github.com/everruns/fetchkit/pull/48))
* chore(deps): update workspace dependencies and fix flaky proxy tests ([#46](https://github.com/everruns/fetchkit/pull/46))
* feat(toolkit): align fetchkit with toolkit library contract ([#45](https://github.com/everruns/fetchkit/pull/45))
* fix(security): harden outbound fetch policy ([#43](https://github.com/everruns/fetchkit/pull/43))
* docs: clarify latest-main requirement for worktrees ([#44](https://github.com/everruns/fetchkit/pull/44))
* fix(security): isolate proxy env in shared runtimes ([#42](https://github.com/everruns/fetchkit/pull/42))
* fix(security): block IPv4-compatible and 6to4 IPv6 addresses in SSRF protection ([#41](https://github.com/everruns/fetchkit/pull/41))
* fix(security): sanitize reqwest error messages to prevent hostname leakage ([#40](https://github.com/everruns/fetchkit/pull/40))
* fix: resolve threat model issues ([#37](https://github.com/everruns/fetchkit/pull/37))

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.1.3...v0.2.0

## [0.1.3] - 2026-03-12

### Highlights

- Hardened redirect handling to revalidate every hop against Fetchkit's SSRF policy
- Tightened allow/block prefix matching to use parsed URL components instead of raw string prefixes
- Added FileSaver trait for saving fetched content to files
- Mitigated 6 open threats from threat model
- Added CLI integration tests and doc tests

### What's Changed

* fix(security): harden redirect validation and URL policy matching ([#23](https://github.com/everruns/fetchkit/pull/23))
* fix(security): mitigate 6 open threats from threat model ([#24](https://github.com/everruns/fetchkit/pull/24))
* fix(cli): disable bin rustdoc to avoid doc collision ([#25](https://github.com/everruns/fetchkit/pull/25))
* feat: add FileSaver trait for saving fetched content to files ([#27](https://github.com/everruns/fetchkit/pull/27))
* fix(ci): replace external HTTP calls with wiremock in fetch_urls example ([#29](https://github.com/everruns/fetchkit/pull/29))
* test: add CLI integration tests, doc tests, Python example, and CI improvements ([#31](https://github.com/everruns/fetchkit/pull/31))
* docs: add cargo install from crates.io to README ([#22](https://github.com/everruns/fetchkit/pull/22))
* docs: remove duplicate release-process from public docs ([#30](https://github.com/everruns/fetchkit/pull/30))
* docs: add git user config requirement to attribution section ([#32](https://github.com/everruns/fetchkit/pull/32))
* ci: adopt bashkit release process ([#26](https://github.com/everruns/fetchkit/pull/26))
* feat(skills): add /processing-issues skill ([#28](https://github.com/everruns/fetchkit/pull/28))
* feat: add /ship command and .agents symlinks ([#21](https://github.com/everruns/fetchkit/pull/21))
* chore: add Doppler secrets management and cloud init script ([#20](https://github.com/everruns/fetchkit/pull/20))
* chore: add attribution settings and agent attribution policy ([#19](https://github.com/everruns/fetchkit/pull/19))

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.1.2...v0.1.3

## [0.1.2] - 2026-02-16

### Highlights

- Added SSRF protection with safe-by-default DNS resolution policy
- Private/reserved IP ranges are now blocked by default to prevent server-side request forgery

### What's Changed

* feat(security)!: add SSRF protection with safe-by-default DNS policy ([#17](https://github.com/everruns/fetchkit/pull/17))

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.1.1...v0.1.2

## [0.1.1] - 2026-02-12

### Highlights

- Updated dependencies to latest versions
- Added maintenance spec for periodic upkeep
- Documentation improvements

### What's Changed

* chore: periodic maintenance - update deps, docs, and add maintenance spec ([#15](https://github.com/everruns/fetchkit/pull/15)) by @chaliy

**Full Changelog**: https://github.com/everruns/fetchkit/compare/v0.1.0...v0.1.1

## [0.1.0] - 2026-02-12

### Highlights

- AI-friendly web content fetching with HTML-to-Markdown and HTML-to-Text conversion
- CLI and MCP server for AI tool integration
- Pluggable fetcher system for URL-specific handling
- Python bindings via PyO3

### What's Changed

* feat: add pluggable fetcher system for URL-specific handling ([#9](https://github.com/everruns/fetchkit/pull/9)) by @chaliy
* docs: add LangChain example for MCP integration ([#8](https://github.com/everruns/fetchkit/pull/8)) by @chaliy
* refactor(cli): unified md-first output format ([#7](https://github.com/everruns/fetchkit/pull/7)) by @chaliy
* docs: clarify test classification in AGENTS.md ([#6](https://github.com/everruns/fetchkit/pull/6)) by @chaliy
* docs: add cloud agent env and complete AGENTS.md placeholders ([#5](https://github.com/everruns/fetchkit/pull/5)) by @chaliy
* refactor: rename project from webfetch to fetchkit ([#4](https://github.com/everruns/fetchkit/pull/4)) by @chaliy
* docs: add comprehensive README with installation and usage guide ([#3](https://github.com/everruns/fetchkit/pull/3)) by @chaliy
* feat: implement webfetch library, CLI, MCP server, and Python bindings ([#1](https://github.com/everruns/fetchkit/pull/1)) by @chaliy
* feat: add initial webfetch spec and guidance by @chaliy

**Full Changelog**: https://github.com/everruns/fetchkit/commits/v0.1.0

[Unreleased]: https://github.com/everruns/fetchkit/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/everruns/fetchkit/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/everruns/fetchkit/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/everruns/fetchkit/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/everruns/fetchkit/compare/v0.1.3...v0.2.0
[0.1.3]: https://github.com/everruns/fetchkit/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/everruns/fetchkit/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/everruns/fetchkit/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/everruns/fetchkit/releases/tag/v0.1.0
