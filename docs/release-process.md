# Release Process

Source of truth: `specs/release-process.md`.

## Overview

FetchKit releases are agent-prepared and CI-published:

1. Human asks an agent to prepare a release
2. Agent updates `CHANGELOG.md` and crate versions, then opens a release PR
3. Human reviews and merges the PR
4. `release.yml` creates a GitHub Release from the merged release commit
5. `publish.yml` publishes crates to crates.io when that GitHub Release is published

## Versioning

FetchKit follows [Semantic Versioning](https://semver.org/).

- MAJOR: breaking API changes
- MINOR: backward-compatible features
- PATCH: backward-compatible fixes and documentation-only release content

Root `Cargo.toml` is the version source of truth.

## Agent Responsibilities

When preparing a release, the agent must:

1. Add a new `CHANGELOG.md` section in the format `## [X.Y.Z] - YYYY-MM-DD`
2. Include `### Highlights` and `### What's Changed`
3. Add `### Breaking Changes` when required
4. Update `[workspace.package].version` in root `Cargo.toml`
5. Update the `fetchkit-cli` dependency on `fetchkit`
6. Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo build --workspace --exclude fetchkit-python --release
```

7. Commit with `chore(release): prepare vX.Y.Z`
8. Open a PR with the same title

## CI Automation

### `release.yml`

- Trigger: push to `main`, or manual `workflow_dispatch`
- Guard: automatic releases only continue when the merged commit starts with
  `chore(release): prepare v`
- Behavior:
  - extract release version
  - verify it matches root `Cargo.toml`
  - extract release notes from `CHANGELOG.md`
  - create GitHub Release `vX.Y.Z`

### `publish.yml`

- Trigger: GitHub Release `published`, or manual `workflow_dispatch`
- Behavior:
  - verify release tag matches root `Cargo.toml` when a tag exists
  - publish `fetchkit`
  - wait for crates.io index propagation
  - publish `fetchkit-cli`

`fetchkit-python` is not part of the crates.io publish flow.
Default release smoke builds exclude it as well.

## Release Artifacts

- GitHub Release with changelog excerpt and source archives
- crates.io package `fetchkit`
- crates.io package `fetchkit-cli`

## Recovery

- Re-run `release.yml` or `publish.yml` with `workflow_dispatch` for manual recovery
- Use `cargo yank` if a bad crate release must be withdrawn
