## Coding-agent guidance (repo root)

This repo is intended to be runnable locally and easy for coding agents to work in.

Style
Telegraph. Drop filler/grammar. Min tokens (global AGENTS + replies).

Critical Thinking
Fix root cause (not band-aid). Unsure: read more code; if still stuck, ask w/ short options. Unrecognized changes: assume other agent; keep going; focus your changes. If it causes issues, stop + ask user. Leave breadcrumb notes in thread.

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

Specification format: Abstract and Requirements sections.

### Skills

`.claude/skills/` contains development skills following the [Agent Skills Specification](https://agentskills.io/specification).

Available skills:
- None configured yet


### Public Documentation

`docs/` contains public-facing user documentation. This documentation is intended for end users and operators of the system, not for internal development reference.


When making changes that affect user-facing behavior or operations, update the relevant docs in this folder.

### Local dev expectations

Requirements:
- Rust stable toolchain (rustup recommended)
- cargo for building and testing

Quick start:
```bash
cargo build --workspace          # Build all crates
cargo test --workspace           # Run all tests
cargo run -p webfetch-cli -- --help  # Run CLI
```


### Code organization

```
crates/
├── webfetch/           # Core library - types, fetch logic, HTML conversion
├── webfetch-cli/       # CLI binary and MCP server
└── webfetch-python/    # Python bindings (PyO3)
specs/                  # Feature specifications
```

### Naming

- Crate names: `webfetch`, `webfetch-cli`, `webfetch-python`
- Types: PascalCase (`WebFetchRequest`, `WebFetchResponse`)
- Functions: snake_case (`fetch`, `html_to_markdown`)
- Constants: SCREAMING_SNAKE_CASE


### CI expectations

- CI is implemented using GitHub Actions.
- Jobs: check, fmt, clippy, test, build, doc
- All jobs must pass before merging
- Clippy runs with `-D warnings` (warnings are errors)
- Doc builds must not have warnings

### Releasing

See `docs/release-process.md` for full release process documentation.

Quick summary:
1. Human asks agent: "Create release v0.2.0"
2. Agent updates CHANGELOG.md (with Highlights + What's Changed), Cargo.toml version, creates PR
3. Human reviews and merges PR to main
4. CI creates GitHub Release via `softprops/action-gh-release` (release.yml)
5. release.yml triggers publish.yml
6. CI publishes `fetchkit` then `fetchkit-cli` to crates.io (publish.yml)

Workflows:
- `.github/workflows/release.yml` - Creates GitHub Release on merge or manual dispatch
- `.github/workflows/publish.yml` - Publishes to crates.io on GitHub Release or manual dispatch

Requirements:
- `CARGO_REGISTRY_TOKEN` secret must be configured in repo settings

Note: `fetchkit-python` is not published to crates.io (`publish = false`). Uses PyPI distribution instead.

### Cloud Agent environments

When running in cloud-hosted agent environments (e.g., Claude Code on the web), the following secrets are available:

- `GITHUB_TOKEN`: Available for GitHub API operations (PRs, issues, repository access)

These secrets are pre-configured in the environment and do not require manual setup.

If `gh` CLI is not available, use GitHub API directly with `GITHUB_TOKEN`:
```bash
curl -H "Authorization: token $GITHUB_TOKEN" https://api.github.com/repos/owner/repo/...
```

### Pre-PR checklist

Before creating a pull request, ensure:

1. **Branch rebased**: Rebase on latest main to avoid merge conflicts
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

6. **CI green**: All CI checks must pass before merging

7. **PR comments resolved**: No unaddressed review comments in PR

8. **Specs**: If changes affect system behavior, update specs in `specs/`

9. **Docs**: If changes affect usage or configuration, update public docs in `docs/`

CI will fail if formatting, linting, tests, or doc build fail. Always run these locally before pushing.

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

**PR Body Template:**

```markdown
## What
Clear description of the change.

## Why
Problem or motivation.

## How
High-level approach.

## Risk
- Low / Medium / High
- What can break

### Checklist
- [ ] Unit tests are passed
- [ ] Smoke tests are passed
- [ ] Documenation is updated
- [ ] Specs are up to date and not in conflict
- [ ] ... other check list items
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
