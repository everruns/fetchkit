# Periodic Maintenance Specification

## Abstract

Define recurring maintenance tasks to keep the fetchkit repository healthy, up-to-date, and well-documented. This spec is intended to be executed periodically (e.g., monthly or before each release) by a human or coding agent.

## Requirements

### 1. Dependency Updates

Update all workspace and crate-level dependencies to their latest compatible versions.

1. **Check outdated deps** - Run `cargo outdated -R` (or equivalent) to list stale dependencies
2. **Update minor/patch** - Apply non-breaking updates via `cargo update`
3. **Evaluate major bumps** - Major version upgrades are allowed; review changelogs for breaking changes and adapt code accordingly
4. **Verify lockfile** - Ensure `Cargo.lock` reflects the updated versions
5. **Build & test** - `cargo build --workspace && cargo test --workspace` must pass after updates
6. **Audit advisories** - Run `cargo audit` (if available) to check for known vulnerabilities

### 2. Documentation Quality (docs.rs / rustdoc)

Ensure all public items have good documentation suitable for docs.rs rendering.

1. **Crate-level docs** - Each crate's `lib.rs` must have a `//!` module doc with:
   - One-line summary
   - Feature overview
   - Quick-start code example (compilable with `cargo test --doc`)
2. **Public items** - Every public struct, enum, trait, function, and method must have a `///` doc comment explaining purpose and usage
3. **Examples in docs** - Key types (`FetchRequest`, `FetchResponse`, `Tool`, `ToolBuilder`, `FetcherRegistry`) should include `# Examples` sections with runnable code blocks
4. **No doc warnings** - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` must pass
5. **README sync** - Root README.md code snippets should be consistent with actual API

### 3. Spec-Code Alignment

Ensure specifications in `specs/` accurately describe the current code, and code conforms to specs.

1. **Type definitions** - Verify struct/enum fields in code match spec definitions (field names, types, optionality)
2. **Error variants** - Verify `FetchError` variants in code match spec
3. **Behavior** - Verify timeouts, binary detection, HTML conversion rules match spec descriptions
4. **Fetcher system** - Verify fetcher trait, registry, and built-in fetchers match `specs/fetchers.md`
5. **CLI flags** - Verify CLI argument names and behavior match spec
6. **MCP protocol** - Verify MCP method names and schemas match spec
7. **Update stale specs** - If code intentionally diverges from spec, update the spec to match
8. **Update stale code** - If spec describes required behavior not in code, flag for implementation

### 4. Example Verification

Ensure all examples compile and run correctly.

1. **Cargo examples** - `cargo run -p fetchkit --example fetch_urls` must complete without error (network-dependent; CI may use timeout)
2. **Doc examples** - `cargo test --doc --workspace` must pass (all doc code blocks must compile)
3. **docs/ and examples/ prose** - Verify shell commands and code snippets in markdown files are accurate
4. **Python examples** - If Python environment available, verify `examples/` Python scripts have correct API usage matching current bindings

### 5. CI & Tooling Health

Verify CI pipeline and development tooling are current.

1. **CI actions** - Check GitHub Actions versions are not deprecated; update to latest stable
2. **Rust toolchain** - Confirm builds on latest Rust stable
3. **Clippy clean** - `cargo clippy --workspace --all-targets -- -D warnings` passes
4. **Format clean** - `cargo fmt --all -- --check` passes
5. **Lockfile committed** - `Cargo.lock` is committed and up-to-date

### 6. Security & License

1. **Dependency licenses** - All dependencies must have permissive licenses (MIT, Apache-2.0, BSD). Flag any non-permissive additions
2. **Advisory scan** - No known vulnerabilities in dependency tree (via `cargo audit` or equivalent)
3. **No secrets** - Ensure no API keys, tokens, or credentials are committed

### 7. Changelog & Versioning

1. **Unreleased section** - `CHANGELOG.md` has an `[Unreleased]` section for pending changes
2. **Version consistency** - Workspace version in root `Cargo.toml` matches latest changelog entry
3. **Inter-crate versions** - Internal dependency versions (e.g., `fetchkit-cli` depending on `fetchkit`) are consistent

## Execution

Run this checklist by working through sections 1-7 in order. Fix issues as encountered. Commit fixes in logical groups following conventional commits. After completion, all CI checks should pass.
