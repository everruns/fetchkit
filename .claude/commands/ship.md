# /ship — 10-phase shipping workflow

Execute all phases sequentially. Phases 2–6 are the quality core — do NOT skip them.

## Phase 1 · Pre-flight

- Confirm current branch is NOT `main` or `master`
- Verify no uncommitted changes (`git status --porcelain`)
- If dirty: stage + commit with conventional message, or abort

## Phase 2 · Test Coverage

- Identify all modified/added code paths via `git diff origin/main...HEAD`
- Ensure each path has:
  - Positive tests: happy path, valid inputs, expected state transitions
  - Negative tests: error conditions, invalid inputs, boundary cases
- Add missing tests. Tests use `wiremock` for HTTP mocking (no real network calls)
- Run: `cargo test --workspace`

## Phase 3 · Code Simplification

- Review diff for duplication, dead code, over-engineering
- Reduce complexity; extract only when reuse is real
- Run `/simplify` skill on changed files

## Phase 4 · Security Review

- Analyze diff for: injection flaws, auth bypasses, input validation gaps, data exposure, OWASP Top 10
- Cross-reference `specs/threat-model.md` for SSRF, DNS rebinding, private-IP access
- Fix any findings before proceeding

## Phase 5 · Artifact Updates

- Sync specs in `specs/` if behavior changed
- Update `docs/` if user-facing behavior changed
- Update `CHANGELOG.md` if shipping a release-worthy change
- Update `AGENTS.md` if dev workflow changed

## Phase 6 · Smoke Testing

- Build: `cargo build --workspace`
- Spot-check CLI: `cargo run -p webfetch-cli -- --help`
- If MCP changes: `cargo run -p webfetch-cli -- mcp` (verify startup)

## Phase 7 · Quality Gates

Run all gates; fix failures before proceeding:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Phase 8 · Push and PR

- Rebase on main: `git fetch origin main && git rebase origin/main`
- Push branch: `git push -u origin <branch>`
- Create or update PR:
  - Title: conventional commit style
  - Body: use PR template (What / Why / How / Risk / Checklist)
  - NEVER add AI attribution or session links

## Phase 9 · CI Wait & Merge

- Poll CI status: `gh pr checks <pr-number> --watch`
- If CI green: squash-merge via `gh pr merge <pr-number> --squash --auto`
- If CI red: diagnose, fix, re-run from Phase 7

## Phase 10 · Post-merge

- Report: merged PR URL, summary of changes
- Clean up local branch if desired
