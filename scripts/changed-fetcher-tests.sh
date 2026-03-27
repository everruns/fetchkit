#!/usr/bin/env bash
# Determine which live_* test modules to run based on changed fetcher files.
#
# Usage: scripts/changed-fetcher-tests.sh <base-ref>
# Output: space-separated test name filters, or empty string if none changed.
#
# Maps: crates/fetchkit/src/fetchers/<name>.rs → live_<name>
# Also maps: tests/fetcher_live.rs → (all)

set -euo pipefail

BASE_REF="${1:-origin/main}"

# Get changed files relative to base
CHANGED=$(git diff --name-only "$BASE_REF"...HEAD 2>/dev/null || git diff --name-only "$BASE_REF" HEAD)

FILTERS=()
RUN_ALL=false

while IFS= read -r file; do
  # If the live test file itself changed, run everything
  if [[ "$file" == "crates/fetchkit/tests/fetcher_live.rs" ]]; then
    RUN_ALL=true
    break
  fi

  # Match fetcher source files
  if [[ "$file" =~ ^crates/fetchkit/src/fetchers/([a-z_]+)\.rs$ ]]; then
    name="${BASH_REMATCH[1]}"
    # Skip mod.rs — user said no need to run all for shared changes
    [[ "$name" == "mod" ]] && continue
    FILTERS+=("live_${name}")
  fi
done <<< "$CHANGED"

if $RUN_ALL; then
  echo "live_"
elif [[ ${#FILTERS[@]} -gt 0 ]]; then
  # Deduplicate
  printf '%s\n' "${FILTERS[@]}" | sort -u | tr '\n' ' '
else
  echo ""
fi
