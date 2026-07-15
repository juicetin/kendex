#!/usr/bin/env bash
# Contract: every tracked GitHub shell test is directly executable.
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../.." && pwd)"
FAIL=0
COUNT=0

while IFS= read -r test_script; do
    COUNT=$((COUNT + 1))
    mode="$(git -C "$REPO_ROOT" ls-files -s -- "$test_script" | awk '{print $1}')"
    if [ "$mode" != "100755" ]; then
        printf 'FAIL: %s has tracked mode %s, expected 100755\n' "$test_script" "${mode:-missing}" >&2
        FAIL=$((FAIL + 1))
        continue
    fi
    if [ ! -x "$REPO_ROOT/$test_script" ]; then
        printf 'FAIL: %s is not executable in the worktree\n' "$test_script" >&2
        FAIL=$((FAIL + 1))
        continue
    fi
    printf 'ok - %s is directly executable\n' "$test_script"
done < <(git -C "$REPO_ROOT" ls-files 'skills/github/tests/*.sh')

if [ "$COUNT" -eq 0 ]; then
    printf 'FAIL: no tracked GitHub shell tests found\n' >&2
    exit 1
fi

if [ "$FAIL" -ne 0 ]; then
    printf 'fail: %d of %d GitHub shell tests violate executable mode contract\n' "$FAIL" "$COUNT" >&2
    exit 1
fi

printf 'all pass: %d GitHub shell tests are directly executable\n' "$COUNT"
