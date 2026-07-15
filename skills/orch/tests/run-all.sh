#!/usr/bin/env bash
# Run every orch regression test in tests/*.sh.
#
# Each individual *.sh test is self-contained: builds its own sandbox,
# exercises the target script, prints `pass: N   fail: M`, exits 0 iff
# all assertions passed. This runner just invokes them in lexical order
# and aggregates the overall exit code so CI / pre-commit hooks have a
# single entry point.
#
# Usage:
#   bash skills/orch/tests/run-all.sh
#   bash skills/orch/tests/run-all.sh session_init      # subset by name

set -uo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FILTER="${1:-}"

FAIL_FILES=()
RUN=0
EXTERNAL_CALLER_CWD="$(mktemp -d)"
trap 'rm -rf "$EXTERNAL_CALLER_CWD"' EXIT

for test_file in "$TEST_DIR"/*.sh; do
  [[ -f "$test_file" ]] || continue
  base=$(basename "$test_file" .sh)
  [[ "$base" == "run-all" ]] && continue
  if [[ -n "$FILTER" ]] && [[ "$base" != *"$FILTER"* ]]; then
    continue
  fi
  RUN=$((RUN + 1))
  printf '\n──── %s ────\n' "$base"
  if [[ "$base" == "generated-start-markdownlint" ]]; then
    # Keep this regression independent of the suite's caller. markdownlint's
    # ignore handling rejects absolute fixture paths outside its working tree.
    (cd "$EXTERNAL_CALLER_CWD" && bash "$test_file")
    test_status=$?
  else
    bash "$test_file"
    test_status=$?
  fi
  if [[ "$test_status" -eq 0 ]]; then
    :
  else
    FAIL_FILES+=("$base")
  fi
done

if [[ "$RUN" -eq 0 ]]; then
  if [[ -n "$FILTER" ]]; then
    echo "run-all.sh: no test scripts matched filter '$FILTER' under $TEST_DIR" >&2
  else
    echo "run-all.sh: no test scripts found under $TEST_DIR" >&2
  fi
  exit 1
fi

echo
echo "============================================"
if [[ ${#FAIL_FILES[@]} -eq 0 ]]; then
  printf 'orch tests: all %d file(s) passed\n' "$RUN"
  exit 0
else
  printf 'orch tests: %d/%d file(s) FAILED:\n' "${#FAIL_FILES[@]}" "$RUN"
  for f in "${FAIL_FILES[@]}"; do
    printf '  - %s\n' "$f"
  done
  exit 1
fi
