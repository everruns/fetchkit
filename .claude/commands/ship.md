# Ship

Run the full ship flow: verify quality, ensure test coverage, update artifacts,
smoke test, then push, create PR, and merge when CI is green.

This command implements the Shipping section in `AGENTS.md`. When the user says
"ship" or "fix and ship", execute all phases below.

## Arguments

- `$ARGUMENTS` - Optional: description of what is being shipped

## Instructions

### Phase 1: Pre-flight

1. Confirm we're NOT on `main` or `master`
2. Confirm there are no uncommitted changes (`git diff --quiet && git diff --cached --quiet`)
3. If uncommitted changes exist, stop and tell the user

### Phase 2: Test Coverage

Review the changes on this branch (`git diff origin/main...HEAD` and
`git log origin/main..HEAD`) and ensure coverage for all changed code paths.

1. Identify all changed code paths
2. Verify existing tests cover the changes
3. Add missing tests:
   - Positive tests: happy path, valid inputs, expected state transitions
   - Negative tests: invalid inputs, error conditions, boundary cases
   - Security tests: if network, URL parsing, filtering, or HTML conversion changed,
     add or extend tests tied to `specs/threat-model.md`
4. Run all tests: `cargo test --workspace`
5. If any test fails, fix the code or test until green

### Phase 3: Artifact Updates

Review the change and update affected artifacts. Skip items that are not touched.

1. Specs in `specs/`
2. Threat model in `specs/threat-model.md` for new attack surfaces or mitigations
3. Release process docs/spec if shipping or release behavior changed
4. `AGENTS.md` if workflow, commands, or repo guidance changed
5. Public docs in `docs/` if user-facing behavior changed
6. `CHANGELOG.md` if preparing a release or shipping notable user-facing behavior

### Phase 3b: Code Simplification

Review all changed code for simplification opportunities.

1. Identify duplication
2. Reduce complexity
3. Remove dead code
4. Check naming clarity
5. Remove unnecessary abstraction

If simplification changes are made, loop back to Phase 2.

### Phase 3c: Security Review

Analyze all changed code for security vulnerabilities.

1. Input validation
2. Injection or path traversal risks
3. SSRF and redirect handling
4. Resource exhaustion
5. Error-message leakage
6. Unsafe code usage

If security issues are found, fix them, add regression tests, and update
`specs/threat-model.md` if a new threat must be tracked.

### Phase 4: Smoke Testing

Smoke test impacted functionality end to end.

1. Build: `cargo build --workspace`
   - Use `cargo build --workspace --exclude fetchkit-python` for the default release smoke path
2. CLI changes: `cargo run -p fetchkit-cli -- --help`
3. Library or fetcher changes: `cargo run -p fetchkit --example fetch_urls`
4. MCP changes: `cargo run -p fetchkit-cli -- mcp`

If smoke testing reveals issues, fix them and loop back to Phase 2.

### Phase 5: Quality Gates

```bash
git fetch origin main && git rebase origin/main
```

- If rebase fails with conflicts, stop and tell the user

Run all gates; fix failures before proceeding:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

### Phase 6: Push and PR

```bash
git push -u origin <current-branch>
```

Check for an existing PR:

```bash
gh pr view --json url 2>/dev/null
```

If no PR exists, create one:

- Title: conventional commit style from the branch changes
- Body: use `.github/pull_request_template.md` (What changed / Why / Before / After / Risk / Checklist); center it on functional change and impact, not a code-location walkthrough
- Evidence: in the Before / After, attach proof — CLI/API output or logs (fetch results, MCP responses). Say so when there is no observable behavior change
- Never add AI attribution or session links

If a PR exists, update it if needed and report its URL.

### Phase 7: Wait for CI and Merge

- Check CI with `gh pr checks` (poll up to 15 minutes)
- If CI is green, merge with `gh pr merge --squash --auto`
- If CI fails, report failing checks and stop
- Never merge when CI is red

### Phase 8: Post-merge

After merge:

- Report the merged PR URL
- Summarize what shipped

## Notes

- Phases 2-4 are the quality core. Do not skip them.
- For "fix and ship" requests: implement the fix first, then run `/ship`.
