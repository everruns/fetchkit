# Decisions:
# - Spec mirrors Bashkit's agent-driven release flow, adapted to FetchKit's crates.
# - GitHub Release creation is the handoff point to publishing; publish retries use `workflow_dispatch`.
# - `fetchkit-python` is explicitly out of the crates.io publish flow until PyPI packaging exists.

# Release Process Specification

## Abstract

Define how FetchKit releases are prepared by a coding agent, reviewed by a human,
and published by GitHub Actions. The process must keep `CHANGELOG.md`,
workspace versions, and release automation in sync.

## Requirements

### Versioning

- FetchKit follows Semantic Versioning.
- Version source of truth is `[workspace.package].version` in root `Cargo.toml`.
- Internal crate dependency versions must match the workspace version when published
  crates depend on each other.

### Release Initiation

- A human initiates a release by asking an agent to prepare a release, for example:
  - `Create release v0.2.0`
  - `Prepare a patch release`
  - `Release the current changes as v0.2.0`
- The agent may suggest the next patch/minor/major version when the human does not
  specify one.

### Release Preparation

When preparing a release, the agent must:

1. Update `CHANGELOG.md`
2. Update root `Cargo.toml` workspace version
3. Update internal dependency versions for published crates
4. Run release verification commands
5. Commit and push a release PR

#### Changelog Format

- Release header format: `## [X.Y.Z] - YYYY-MM-DD`
- Include `### Highlights` with 2-5 high-signal user-facing bullets
- Include `### Breaking Changes` for breaking minor/major releases, with migration notes
- Include `### What's Changed` using GitHub-style PR links
- Order `### What's Changed` entries newest first
- End each release section with `**Full Changelog**: <compare URL>`
- Maintain comparison links at the bottom of the file

#### Version Updates

- Root `Cargo.toml` must be updated to `version = "X.Y.Z"` under `[workspace.package]`
- `crates/fetchkit-cli/Cargo.toml` must depend on `fetchkit = { version = "X.Y.Z" }`
- `fetchkit-python` does not participate in crates.io publishing and does not require
  a version-pinned internal dependency update

#### Verification

Before the agent opens a release PR, it must run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Release smoke builds should verify the publishable Rust crates with:

```bash
cargo build --workspace --exclude fetchkit-python --release
```

#### Release Commit and PR

- Commit message must be `chore(release): prepare vX.Y.Z`
- PR title must match the commit message
- PR body should include the release summary and changelog excerpt

### CI Automation

#### Release Workflow

`.github/workflows/release.yml` must:

- Trigger on pushes to `main`
- Also support `workflow_dispatch` for manual recovery or re-runs
- Only continue automatically when the pushed commit message starts with
  `chore(release): prepare v`
- Extract the release version from:
  - The first line of the head commit message on push-triggered releases
  - Root `Cargo.toml` on manual dispatch
- Verify the extracted version matches root `Cargo.toml`
- Extract release notes for that version from `CHANGELOG.md`
- Create a GitHub Release tagged `vX.Y.Z`
- Start the publish workflow after creating the release. GitHub does not fire
  the `release.published` event for releases created via the default
  `GITHUB_TOKEN`, so release.yml dispatches publish.yml explicitly via
  `gh workflow run publish.yml` (requires `actions: write` permission). The
  publish workflow's `release.published` trigger still covers releases created
  through the GitHub UI or by a PAT-authenticated workflow.

#### Publish Workflow

`.github/workflows/publish.yml` must:

- Trigger on GitHub Release `published`
- Also support `workflow_dispatch` for manual retries
- Verify the release tag version matches root `Cargo.toml` when a release tag is present
- Publish crates in dependency order:
  1. `fetchkit`
  2. `fetchkit-cli`
- Wait briefly between crate publishes so crates.io index updates propagate
- Not attempt to publish `fetchkit-python` to crates.io
- Keep the default release-build verification scoped to those publishable crates

### Authentication

- GitHub Actions publishing requires `CARGO_REGISTRY_TOKEN`
- Manual release preparation may require GitHub CLI auth for PR creation and merge steps

### Operator Responsibilities

- Human reviews and merges the release PR
- CI creates the GitHub Release after merge
- CI publishes crates after the GitHub Release is published

### Rollback

- If a bad crates.io release ships, rollback uses `cargo yank`
- Yanking must happen in reverse dependency order:
  1. `fetchkit-cli`
  2. `fetchkit`

### Alignment

- `.claude/commands/ship.md` must remain compatible with this release workflow
