## Coding-agent guidance (repo root)

This repo is intended to be runnable locally and easy for coding agents to work in.

Always make sure you are working on top of latest main from remote. Especially in detached worktrees, fetch `origin/main` and rebase or branch from it before editing.

Style
Telegraph. Drop filler/grammar. Min tokens (global AGENTS + replies).

Critical Thinking
Fix root cause (not band-aid). Unsure: read more code; if still stuck, ask w/ short options. Unrecognized changes: assume other agent; keep going; focus your changes. If it causes issues, stop + ask user. Leave breadcrumb notes in thread.

Attribution
NEVER add links to Claude sessions in PR body or commits.
Do not attribute commits or merge commits to coding agents by default; use the
configured git user unless the repo owner asks for a specific attribution.
Contributions from YOLOP agents may be attributed to YOLOP agents.

### Principles

- Keep decisions as comments on top of the file. Only important decisions that could not be inferred from code.
- Code should be easily testable, smoke testable, runnable in local dev env.
- Prefer small, incremental PR-sized changes with a runnable state at each step.
- Avoid adding dependencies with non-permissive licenses. If a dependency is non-permissive or unclear, stop and ask the repo owner.

### Top level requirements

AI-friendly web content fetching tool designed for LLM consumption. Rust library with CLI, MCP server, and Python bindings.

Key capabilities:
- HTTP fetching (GET/HEAD) with streaming support
- HTML-to-Markdown and HTML-to-Text conversion optimized for LLMs
- Binary content detection (returns metadata only)
- Timeout handling with partial content on timeout
- URL filtering via allow/block lists
- MCP server for AI tool integration

### Specs

`specs/` folder contains feature specifications outlining requirements for specific features and components. New code should comply with these specifications or propose changes to them.

Available specs:
- `specs/initial.md` - WebFetch tool specification (types, behavior, conversions, error handling)
- `specs/fetchers.md` - Pluggable fetcher system for URL-specific handling
- `specs/release-process.md` - Agent-driven release and publish workflow
- `specs/maintenance.md` - Periodic maintenance checklist (deps, docs, spec-code alignment)
- `specs/threat-model.md` - Security threat model (SSRF, network, input validation, DoS)
- `specs/bot-auth.md` - Web Bot Authentication (draft-meunier-web-bot-auth-architecture)

Specification format: Abstract and Requirements sections.

### Shipping

Implement → test → `/ship`. The `/ship` command (`.claude/commands/ship.md`) runs a 10-phase workflow: pre-flight, test coverage, code simplification, security review, artifact updates, smoke testing, quality gates, push+PR, CI wait+merge, post-merge report.

Phases 2–6 (tests, simplification, security, artifacts, smoke) are the quality core — never skip.

When asked to "fix and ship": implement fix first, then run `/ship`.

### Skills

`.claude/skills/` contains development skills following the [Agent Skills Specification](https://agentskills.io/specification).

Available skills:
- `/ship` — 10-phase shipping workflow (`.claude/commands/ship.md`)
- `/processing-issues` — Batch-process GitHub issues: triage, implement, ship via individual PRs (`.claude/commands/processing-issues.md`)
- `/process-issues` — Resolve all open GitHub issues e2e; one issue = one shipped PR (`.claude/skills/process-issues/SKILL.md`)

### Agent-portable paths

`.agents/` mirrors `.claude/` via symlinks for agent-agnostic access:
- `.agents/commands/` → `.claude/commands/`
- `.agents/skills/` → `.claude/skills/`


### Public Documentation

`docs/` contains public-facing user documentation. This documentation is intended for end users and operators of the system, not for internal development reference.


When making changes that affect user-facing behavior or operations, update the relevant docs in this folder.

### Local dev expectations

Requirements:
- Rust stable toolchain (rustup recommended)
- cargo for building and testing

Quick start:
```bash
cargo build --workspace --exclude fetchkit-python  # Build default Rust artifacts
cargo test --workspace           # Run all tests
cargo run -p fetchkit-cli -- --help  # Run CLI
```

Note: `fetchkit-python` currently requires a separate Python link environment and is
not part of the default release-build smoke path.


### Code organization

```
crates/
├── fetchkit/           # Core library - types, fetch logic, HTML conversion
├── fetchkit-cli/       # CLI binary and MCP server
└── fetchkit-python/    # Python bindings (PyO3)
specs/                  # Feature specifications
```

### Naming

- Crate names: `fetchkit`, `fetchkit-cli`, `fetchkit-python`
- Types: PascalCase (`WebFetchRequest`, `WebFetchResponse`)
- Functions: snake_case (`fetch`, `html_to_markdown`)
- Constants: SCREAMING_SNAKE_CASE


### CI expectations

- CI is implemented using GitHub Actions.
- Jobs: lint, test, build, doc, examples, check
- `check` is the branch-protection gate and must stay green
- All jobs must pass before merging
- Clippy runs with `-D warnings` (warnings are errors)
- Doc builds must not have warnings

### Releasing

See `specs/release-process.md` for the release contract.

Quick summary:
1. Human asks agent: "Create release v0.2.0"
2. Agent updates CHANGELOG.md (with Highlights + What's Changed), Cargo.toml version, creates PR
3. Human reviews and merges PR to main
4. CI creates GitHub Release via `softprops/action-gh-release` (`release.yml`)
5. GitHub Release publication triggers `publish.yml`
6. CI publishes `fetchkit` then `fetchkit-cli` to crates.io

Workflows:
- `.github/workflows/release.yml` - Creates GitHub Release on merge or manual dispatch
- `.github/workflows/publish.yml` - Publishes to crates.io on GitHub Release or manual dispatch

Requirements:
- `CARGO_REGISTRY_TOKEN` secret must be configured in repo settings

Note: `fetchkit-python` is not published to crates.io (`publish = false`). Python release automation is not configured in this repo yet.

### Cloud Agent (start here)

Use Doppler for all secret-backed commands in cloud agents.

```bash
./scripts/init-cloud-env.sh
```

Disable incremental compilation in cloud (saves ~3 GB, useless for single builds):

```bash
export CARGO_INCREMENTAL=0
```

All cloud secrets are in Doppler (`GITHUB_TOKEN`). Project: `everruns-dev`, config: `dev`.

For GitHub CLI, map token explicitly:

```bash
doppler run -- bash -lc 'GH_TOKEN="$GITHUB_TOKEN" gh auth status'
```

Quickcheck:

```bash
doppler run -- bash -lc 'test -n "${GITHUB_TOKEN:-}" && echo GITHUB_TOKEN present'
doppler run -- bash -lc 'GH_TOKEN="$GITHUB_TOKEN" gh auth status'
```

### Pre-PR checklist

Before creating a pull request, ensure:

1. **Branch rebased**: Rebase on latest main to avoid merge conflicts. In detached worktrees, first create or switch to a topic branch that is based on `origin/main`.
   ```bash
   git fetch origin main && git rebase origin/main
   ```

2. **Formatting**: Run formatter and fix any issues
   ```bash
   cargo fmt --all
   ```

3. **Linting**: Run clippy and fix all warnings
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   ```

4. **Tests**: Run all tests and ensure they pass
   ```bash
   cargo test --workspace
   ```

5. **Documentation**: Ensure docs build without warnings
   ```bash
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   ```

6. **Release build smoke**: Ensure the publishable Rust crates build in release mode
   ```bash
   cargo build --workspace --exclude fetchkit-python --release
   ```

7. **CI green**: All CI checks must pass before merging

8. **PR comments resolved**: No unaddressed review comments in PR

9. **Specs**: If changes affect system behavior, update specs in `specs/`

10. **Docs**: If changes affect usage or configuration, update public docs in `docs/`

CI will fail if formatting, linting, tests, release build smoke, or doc build fail. Always run these locally before pushing.

### Commit message conventions

Follow [Conventional Commits](https://www.conventionalcommits.org) for all commit messages:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style (formatting, semicolons, etc.)
- `refactor`: Code refactoring without feature/fix
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Build process, dependencies, tooling
- `ci`: CI configuration changes

**Examples:**
```
feat(api): add agent versioning endpoint
fix(workflow): handle timeout in run execution
docs: update API documentation
refactor(db): simplify connection pooling
```

**Validation (optional):**
```bash
# Validate a commit message
echo "feat: add new feature" | npx commitlint

# Validate last commit
npx commitlint --from HEAD~1 --to HEAD
```

### PR (Pull Request) conventions

PR titles should follow Conventional Commits format. Use the PR template (`.github/pull_request_template.md`) for descriptions.

Center the description on functional change and impact, not a code-location
walkthrough (the diff shows that). Add a Before / After with proof — CLI/API output,
logs, metrics, or screenshots for UI — whenever behavior changes.

**PR Body Template:**

```markdown
## What changed
Describe the change functionally — what behavior changes and its impact. Lead with
outcomes; don't walk through code locations, the diff shows where and how. Keep any
code-level notes short and specific.

## Why
Problem or motivation.

## Before / After
Show the effect with evidence. Include before and after whenever behavior changes —
CLI/API output, logs, metrics, or screenshots for UI (attach working screenshots
when possible). For changes with no observable behavior (pure refactor, docs), say so.

## Risk
- Low / Medium / High
- What can break

### Checklist
- [ ] Unit tests are passed
- [ ] Smoke tests are passed
- [ ] Documentation is updated
- [ ] Specs are up to date and not in conflict
```

### Testing the system

```bash
# Run all tests
cargo test --workspace

# Run tests with output
cargo test --workspace -- --nocapture

# Run specific test
cargo test --workspace test_name

# Test CLI directly
cargo run -p webfetch-cli -- --url https://example.com --as-markdown

# Test MCP server
cargo run -p webfetch-cli -- mcp
```

Tests use `wiremock` for HTTP mocking (no real external network calls). See `specs/initial.md` for test requirements.
