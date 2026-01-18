# Release Process

## Abstract

This document describes the release process for fetchkit. Releases are initiated by asking a coding agent to prepare the release, with CI automation handling the rest.

## Versioning

fetchkit follows [Semantic Versioning](https://semver.org/):

- **MAJOR** (X.0.0): Breaking API changes
- **MINOR** (0.X.0): New features, backward compatible
- **PATCH** (0.0.X): Bug fixes, backward compatible

## Release Workflow

### Overview

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Human asks     │     │  Agent creates  │     │  GitHub         │     │  crates.io      │
│  "release v0.2" │────>│  release PR     │────>│  Release        │────>│  Publish        │
│                 │     │                 │     │  (automatic)    │     │  (automatic)    │
└─────────────────┘     └─────────────────┘     └─────────────────┘     └─────────────────┘
```

### Human Steps

1. **Ask the agent** to create a release:
   - "Create release v0.2.0"
   - "Prepare a patch release"
   - "Release the current changes as v0.2.0"

2. **Review the PR** created by the agent

3. **Merge to main** - CI handles GitHub Release and crates.io publish

### Agent Steps (automated)

When asked to create a release, the agent:

1. **Determine version**
   - Use version specified by human, OR
   - Suggest next version based on changes (patch/minor/major)

2. **Update CHANGELOG.md**
   - Move items from `[Unreleased]` to new version section
   - Add release date: `## [X.Y.Z] - YYYY-MM-DD`
   - Add breaking changes section if applicable (see format below)
   - List commits in GitHub-style format with PR links and contributors
   - Update comparison links at bottom of file

3. **Update Cargo.toml**
   - Set `version = "X.Y.Z"` in workspace

4. **Run verification**
   - `cargo fmt --check`
   - `cargo clippy`
   - `cargo test`

5. **Commit and push**
   - Commit message: `chore(release): prepare vX.Y.Z`
   - Push to feature branch

6. **Create PR**
   - Title: `chore(release): prepare vX.Y.Z`
   - Include changelog excerpt in description

### CI Automation

**On merge to main** (release.yml):
- Detects commit message `chore(release): prepare vX.Y.Z`
- Extracts release notes from CHANGELOG.md
- Creates GitHub Release with tag `vX.Y.Z`

**On GitHub Release created** (publish.yml):
- Runs verification (fmt, clippy, tests)
- Publishes `fetchkit` and `fetchkit-cli` to crates.io

## Pre-Release Checklist

The agent verifies before creating a release PR:

- [ ] All CI checks pass on main
- [ ] `cargo fmt` - code is formatted
- [ ] `cargo clippy` - no warnings
- [ ] `cargo test` - all tests pass
- [ ] CHANGELOG.md has entries for changes since last release

## Changelog Format

The changelog follows [Keep a Changelog](https://keepachangelog.com/) with GitHub-style commit listings.

### Structure

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Breaking Changes

- **Short description**: Detailed explanation of what changed and migration steps.
  - Before: `/old/path`
  - After: `/new/path`

### What's Changed

- Commit message ([#PR](https://github.com/everruns/fetchkit/pull/PR)) by @contributor
- Another commit ([#PR](https://github.com/everruns/fetchkit/pull/PR)) by @contributor
```

### Generating Commit List

Get commits since last release, excluding chore/ci/bench commits:

```bash
git log --oneline | grep -v -E "^.{7} (chore|ci|bench)"
```

Format each commit as:
```
- <commit message> ([#<PR>](https://github.com/everruns/fetchkit/pull/<PR>)) by @<author>
```

### Breaking Changes Section

Include when the release has breaking changes (typically MINOR or MAJOR versions):

1. **Bold summary** of the breaking change
2. **Migration guide** showing before/after
3. **Code examples** if helpful

Example:
```markdown
### Breaking Changes

- **Renamed crate from webfetch to fetchkit**: All imports need to be updated.
  - `webfetch::fetch` → `fetchkit::fetch`
  - CLI binary: `webfetch` → `fetchkit`
```

## Workflows

### release.yml

- **Trigger**: Push to `main` with commit message starting with `chore(release): prepare v`
- **Actions**: Creates GitHub Release with tag and release notes
- **File**: `.github/workflows/release.yml`

### publish.yml

- **Trigger**: GitHub Release published
- **Actions**: Verifies and publishes to crates.io
- **File**: `.github/workflows/publish.yml`
- **Secret required**: `CARGO_REGISTRY_TOKEN`

## Example Conversation

```
Human: Create release v0.2.0

Agent: I'll prepare the v0.2.0 release. Let me:
1. Update CHANGELOG.md with the v0.2.0 section
2. Update Cargo.toml version to 0.2.0
3. Run verification checks
4. Create the release PR

[Agent performs steps...]

Done. PR created: https://github.com/everruns/fetchkit/pull/XX
Please review and merge to trigger the release.
```

## Hotfix Releases

For urgent fixes:

1. Ask agent: "Create patch release v0.1.1 for the auth fix"
2. Agent prepares release with patch version
3. Review and merge

## Release Artifacts

Each release includes:

- **GitHub Release**: Tag, release notes, source archives
- **crates.io**: Published crates for `cargo install fetchkit-cli`

Note: `fetchkit-python` is not published to crates.io (uses PyPI distribution instead).

Future considerations:
- Pre-built binaries (Linux, macOS, Windows)
- Docker images
- Homebrew formula
