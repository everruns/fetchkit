# Processing Issues

Batch-process GitHub issues: triage, implement, and ship fixes/features via
individual PRs. Use when tackling a backlog of open issues or threat-model items.

## Arguments

- `$ARGUMENTS` - Optional: filter or label selector (e.g. "bug", "threat-model",
  issue numbers like "#12 #15 #20")

## Instructions

### Phase 1: Gather Issues

1. List open issues matching the filter (or all open issues if no filter):
   ```bash
   gh issue list --state open --limit 50
   ```
2. If `$ARGUMENTS` specifies issue numbers, use only those.
3. Present the list to the user and confirm which issues to process.

### Phase 2: Plan and Prioritize

1. Read each selected issue to understand scope.
2. Order by priority: security/threat-model issues first, bugs second, features last.
3. Present the ordered list with one-line summaries for user approval.

### Phase 3: Process Issues

**Constraint: max 5 issues in parallel.** Never start more than 5 issue branches
at the same time. If processing more than 5, work in batches of 5, completing one
batch before starting the next.

For **each** issue, follow this workflow:

#### 3a: Branch

1. Create a dedicated branch from latest `main`:
   ```bash
   git fetch origin main
   git checkout -b fix/<issue-number>-<short-slug> origin/main
   ```
2. One branch per issue — never combine multiple issues into a single PR.

#### 3b: Implement

1. Implement the fix or feature for the issue.
2. Write or update tests covering the change.
3. Keep changes minimal and focused on the issue.

#### 3c: Ship

1. Run `/ship` to execute the full shipping workflow (quality gates, PR, CI, merge).
2. Reference the issue in the PR body (e.g. "Closes #42").
3. Do **not** merge PRs that have failing CI — report and move on.

#### 3d: Rebase Check

After each PR is merged (or after every 2–3 PRs), rebase all remaining
in-progress branches on latest `main`:

```bash
git fetch origin main
git rebase origin/main
```

If conflicts arise, resolve them or flag to user before continuing.

### Phase 4: Post-Processing

1. Report a summary table of all processed issues:
   - Issue number and title
   - PR number and URL
   - Status: merged / open / blocked
2. List any issues that could not be completed and why.

## Rules

- **One issue = one PR.** Never bundle unrelated issues.
- **Max 5 parallel.** Do not exceed 5 concurrent issue branches.
- **Rebase often.** Rebase on `main` periodically, especially after merges,
  to avoid conflicts piling up.
- **Ship each PR.** Every PR must go through `/ship` (quality gates, tests,
  CI, merge). No shortcuts.
- **No AI attribution.** Never add session links or agent attribution to
  commits or PR bodies.
