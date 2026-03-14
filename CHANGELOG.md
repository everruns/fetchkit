# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Highlights

- Added a hardened outbound profile for cluster and data-plane deployments
- Ambient proxy environment variables are now ignored by default
- Added hostname, port, and redirect restrictions for tighter egress policy

### What's Changed

- `fix(security): harden outbound fetch policy and add deployment guidance`

## [0.1.3] - 2026-03-12

### Highlights

- Hardened redirect handling to revalidate every hop against FetchKit's SSRF policy
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

[Unreleased]: https://github.com/everruns/fetchkit/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/everruns/fetchkit/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/everruns/fetchkit/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/everruns/fetchkit/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/everruns/fetchkit/releases/tag/v0.1.0
