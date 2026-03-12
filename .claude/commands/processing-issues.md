# Processing Issues

Batch-process GitHub issues or open threat-model items. Triage, implement, and
ship via individual PRs.

## Arguments

- `$ARGUMENTS` - Optional: filter, label, or issue numbers (e.g. "bug", "#12 #15")

## Requirements

- **One issue = one PR.** Never bundle unrelated issues into a single PR.
- **Ship each PR via `/ship`.** Every PR follows the full shipping process.
- **Max 5 issues in parallel.** Work in batches if more than 5.
- **Rebase often.** Rebase in-progress branches on `main` periodically,
  especially after something is merged, to avoid conflict pileup.
- **Reference the issue** in the PR body (e.g. "Closes #42").
- **Report results.** After processing, summarize each issue with PR link and status.
