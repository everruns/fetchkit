# Processing Issues

Resolve all qualifying open GitHub issues end-to-end. Each issue becomes exactly one shipped PR.

Delegates to the `process-issues` skill (`.claude/skills/process-issues/SKILL.md`).

## Arguments

- `$ARGUMENTS` - Optional: specific issue numbers (e.g. "42 55") or labels. If omitted, process all open issues.

## Instructions

Run the `process-issues` skill with the provided arguments. The skill handles:

1. Listing and filtering qualifying issues
2. Per-issue: scope, test, implement, `/ship`
3. Post-processing: scan for `#[ignore]` tests that may now pass
