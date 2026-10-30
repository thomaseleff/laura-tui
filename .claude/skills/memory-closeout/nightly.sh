#!/bin/sh
# Nightly Brain distill: main with a budget, and each branch whose PR is merged.
# A branch with an open PR, or no PR, waits until it merges. DRY_RUN=1 prints the plan only.
set -u

echo "== $(date -Is) nightly distill"

branches=$(entire brain distill --dry-run --json | python3 -I -c '
import json, sys
for b in json.load(sys.stdin)["branches"]:
    if b["sessions_to_distill"] > 0:
        print(b["branch"])')

for b in $branches; do
  if [ "$b" = main ]; then
    cmd="entire brain distill --branch main --newest-first --max-sessions 100"
  elif n=$(gh pr list --head "$b" --state merged --json number -q length) && [ "$n" != 0 ]; then
    cmd="entire brain distill --branch $b"
  else
    echo "skip $b: no merged PR, or gh failed"
    continue
  fi
  echo "run: $cmd"
  [ "${DRY_RUN:-0}" = 1 ] || $cmd || echo "failed: $b"
done

if [ "${DRY_RUN:-0}" = 1 ]; then
  echo "run: entire brain patterns verify --deep"
else
  entire brain patterns verify --deep || echo "failed: patterns verify"
fi
