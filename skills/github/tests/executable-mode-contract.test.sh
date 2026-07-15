#!/usr/bin/env bash
# Contract: every tracked GitHub shell test is directly executable in the
# current working tree, including unstaged mode changes produced by refresh.
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../.." && pwd)"
FAIL=0
COUNT=0

effective_tracked_mode() {
    local repo_root="$1"
    local test_script="$2"
    local worktree_mode

    # diff-files reports the effective worktree mode in its second raw field.
    # Fall back to the index only when the path has no unstaged change.
    worktree_mode="$(git -C "$repo_root" diff-files --raw -- "$test_script" | awk 'NR == 1 { print $2 }')"
    if [ -n "$worktree_mode" ]; then
        printf '%s\n' "$worktree_mode"
    else
        git -C "$repo_root" ls-files -s -- "$test_script" | awk 'NR == 1 { print $1 }'
    fi
}

contract_accepts_script() {
    local repo_root="$1"
    local test_script="$2"
    local mode="${3:-}"

    if [ -z "$mode" ]; then
        mode="$(effective_tracked_mode "$repo_root" "$test_script")"
    fi
    [ "$mode" = "100755" ] && [ -x "$repo_root/$test_script" ]
}

assert_fixture_contract() {
    local expected="$1"
    local label="$2"
    local actual="reject"

    if contract_accepts_script "$FIXTURE_REPO" "$FIXTURE_RELATIVE"; then
        actual="accept"
    fi
    if [ "$actual" = "$expected" ]; then
        printf 'ok - %s\n' "$label"
    else
        printf 'FAIL: %s (expected %s, got %s)\n' "$label" "$expected" "$actual" >&2
        FAIL=$((FAIL + 1))
    fi
}

while IFS= read -r test_script; do
    COUNT=$((COUNT + 1))
    mode="$(effective_tracked_mode "$REPO_ROOT" "$test_script")"
    if ! contract_accepts_script "$REPO_ROOT" "$test_script" "$mode"; then
        if [ "$mode" != "100755" ]; then
            printf 'FAIL: %s has effective mode %s, expected 100755\n' "$test_script" "${mode:-missing}" >&2
        else
            printf 'FAIL: %s is not executable in the worktree\n' "$test_script" >&2
        fi
        FAIL=$((FAIL + 1))
        continue
    fi
    printf 'ok - %s is directly executable\n' "$test_script"
done < <(git -C "$REPO_ROOT" ls-files 'skills/github/tests/*.sh')

FIXTURE_ROOT="$(mktemp -d)"
trap 'rm -rf "$FIXTURE_ROOT"' EXIT
FIXTURE_REPO="$FIXTURE_ROOT/repo"
FIXTURE_RELATIVE="skills/github/tests/fixture.sh"
FIXTURE_SCRIPT="$FIXTURE_REPO/$FIXTURE_RELATIVE"

git init -q "$FIXTURE_REPO"
git -C "$FIXTURE_REPO" config core.fileMode true
mkdir -p "$(dirname "$FIXTURE_SCRIPT")"
printf '#!/usr/bin/env bash\nexit 0\n' >"$FIXTURE_SCRIPT"
chmod 0644 "$FIXTURE_SCRIPT"
git -C "$FIXTURE_REPO" add -- "$FIXTURE_RELATIVE"

assert_fixture_contract "reject" "tracked non-executable mode remains a contract failure"

chmod 0755 "$FIXTURE_SCRIPT"
assert_fixture_contract "accept" "unstaged 100644 to 100755 mode change is accepted"

git -C "$FIXTURE_REPO" add -- "$FIXTURE_RELATIVE"
chmod 0644 "$FIXTURE_SCRIPT"
assert_fixture_contract "reject" "unstaged 100755 to 100644 mode change is rejected"

if [ "$COUNT" -eq 0 ]; then
    printf 'FAIL: no tracked GitHub shell tests found\n' >&2
    exit 1
fi

if [ "$FAIL" -ne 0 ]; then
    printf 'fail: %d of %d GitHub shell tests violate executable mode contract\n' "$FAIL" "$COUNT" >&2
    exit 1
fi

printf 'all pass: %d GitHub shell tests are directly executable\n' "$COUNT"
