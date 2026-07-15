#!/usr/bin/env bash
# Regression tests for `worktree create` reuse rebase-conflict recovery (vstack#567).
#
# When `create` reuses an existing worktree and the rebase onto origin/<default>
# conflicts, the default path aborts the rebase — so the worktree is clean and
# there is no conflict state left to "resolve manually". The error must be
# truthful and actionable: list the conflicting files (captured before the
# abort) and name the two supported recovery paths (`--restack` or
# delete/recreate). `--restack` must redo the rebase and stop IN the conflict
# state with continue/abort guidance. A completed supported rewrite must carry
# an exact, one-worktree push authorization without weakening remote-movement
# or unexpected-local-divergence rejection. Clean-rebase reuse must keep
# working unchanged.
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKTREE_SCRIPT="${WORKTREE_SCRIPT:-$(cd "$TEST_DIR/.." && pwd)/scripts/worktree}"
TMP_ROOT="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$TMP_ROOT"' EXIT

mkdir -p "$TMP_ROOT/bin"
cat >"$TMP_ROOT/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}:${2:-}" in
  pr:list) ;;
esac
STUB
chmod +x "$TMP_ROOT/bin/gh"
export PATH="$TMP_ROOT/bin:$PATH"

PASS=0
FAIL=0

assert_eq() {
  local got="$1" want="$2" name="$3"
  if [[ "$got" == "$want" ]]; then
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        expected: %s\n        got:      %s\n' "$name" "$want" "$got"
  fi
}

assert_ne() {
  local got="$1" unwanted="$2" name="$3"
  if [[ "$got" != "$unwanted" ]]; then
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        expected value to differ from: %s\n' "$name" "$unwanted"
  fi
}

assert_contains() {
  local haystack="$1" needle="$2" name="$3"
  if grep -qF -- "$needle" <<<"$haystack"; then
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        wanted substring: %s\n        in: %s\n' "$name" "$needle" "$haystack"
  fi
}

assert_not_contains() {
  local haystack="$1" needle="$2" name="$3"
  if grep -qF -- "$needle" <<<"$haystack"; then
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        unwanted substring present: %s\n        in: %s\n' "$name" "$needle" "$haystack"
  else
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  fi
}

assert_path_exists() {
  local path="$1" name="$2"
  if [[ -e "$path" ]]; then
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        missing path: %s\n' "$name" "$path"
  fi
}

assert_is_ancestor() {
  local repo="$1" ancestor="$2" descendant="$3" name="$4"
  if git -C "$repo" merge-base --is-ancestor "$ancestor" "$descendant"; then
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        %s is not an ancestor of %s\n' "$name" "$ancestor" "$descendant"
  fi
}

rebase_state_exists() {
  local wt="$1" state path
  for state in rebase-merge rebase-apply; do
    path="$(git -C "$wt" rev-parse --git-path "$state" 2>/dev/null)" || continue
    [[ "$path" == /* ]] || path="$wt/$path"
    if [[ -d "$path" ]]; then
      return 0
    fi
  done
  return 1
}

assert_rebase_in_progress() {
  local wt="$1" name="$2"
  if rebase_state_exists "$wt"; then
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  else
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        no rebase-merge/rebase-apply state in: %s\n' "$name" "$wt"
  fi
}

assert_no_rebase_in_progress() {
  local wt="$1" name="$2"
  if rebase_state_exists "$wt"; then
    FAIL=$((FAIL + 1))
    printf '  FAIL  %s\n        rebase state still present in: %s\n' "$name" "$wt"
  else
    PASS=$((PASS + 1))
    printf '  ok    %s\n' "$name"
  fi
}

make_repo() {
  local repo="$1"
  mkdir -p "$repo"
  git -C "$repo" init -q -b main
  git -C "$repo" config user.email test@example.com
  git -C "$repo" config user.name Test
  git -C "$repo" config commit.gpgsign false
  printf 'orig\n' > "$repo/file.txt"
  git -C "$repo" add file.txt
  git -C "$repo" commit -q -m base
}

# Build a main+origin pair whose issue worktree diverges from origin/main on
# the same line of file.txt, so a reuse rebase genuinely conflicts.
make_conflict_pair() {
  local root="$1" issue="$2"
  make_repo "$root/main"
  git init -q --bare "$root/origin.git"
  git -C "$root/main" remote add origin "$root/origin.git"
  git -C "$root/main" push -q -u origin main
  # Create through the script so reuse exercises the script's own worktree.
  (cd "$root/main" && "$WORKTREE_SCRIPT" create "$issue" >/dev/null 2>&1)
  local wt="$root/trees/$issue"
  printf 'feature\n' > "$wt/file.txt"
  git -C "$wt" add file.txt
  git -C "$wt" commit -q -m 'feature edit'
  printf 'main-side\n' > "$root/main/file.txt"
  git -C "$root/main" add file.txt
  git -C "$root/main" commit -q -m 'main edit'
  git -C "$root/main" push -q origin main
}

make_published_clean_pair() {
  local root="$1" issue="$2"
  make_repo "$root/main"
  git init -q --bare "$root/origin.git"
  git -C "$root/main" remote add origin "$root/origin.git"
  git -C "$root/main" push -q -u origin main
  (cd "$root/main" && "$WORKTREE_SCRIPT" create "$issue" >/dev/null 2>&1)
  local wt="$root/trees/$issue"
  printf 'feature\n' > "$wt/feature.txt"
  git -C "$wt" add feature.txt
  git -C "$wt" commit -q -m 'feature edit'
  git -C "$wt" push -q origin "HEAD:refs/heads/$issue"
  printf 'advanced\n' > "$root/main/main-advanced.txt"
  git -C "$root/main" add main-advanced.txt
  git -C "$root/main" commit -q -m 'advance main'
  git -C "$root/main" push -q origin main
}

echo "=== worktree create reuse rebase-conflict recovery ==="

# --- Default path: abort, truthful and actionable error ------------------------
DEFAULT_ROOT="$TMP_ROOT/default"
make_conflict_pair "$DEFAULT_ROOT" issue-default
DEFAULT_WT="$DEFAULT_ROOT/trees/issue-default"
default_pre_head="$(git -C "$DEFAULT_WT" rev-parse HEAD)"
set +e
(
  cd "$DEFAULT_ROOT/main" && \
    "$WORKTREE_SCRIPT" create issue-default --reuse >"$DEFAULT_ROOT/create.out" 2>"$DEFAULT_ROOT/create.err"
)
default_code=$?
set -e
default_err="$(cat "$DEFAULT_ROOT/create.err")"
assert_eq "$default_code" "1" "default reuse with conflict exits 1"
assert_contains "$default_err" "Conflicting files:" "default error reports captured conflict list"
assert_contains "$default_err" "file.txt" "default error names the conflicting file"
assert_not_contains "$default_err" "Resolve manually" "default error does not claim a conflict state the abort erased"
assert_contains "$default_err" "aborted" "default error says the rebase was aborted"
assert_contains "$default_err" "--restack" "default error names the --restack recovery path"
assert_contains "$default_err" "remove issue-default" "default error names the delete/recreate recovery path"
assert_no_rebase_in_progress "$DEFAULT_WT" "default reuse leaves no rebase in progress"
assert_eq "$(git -C "$DEFAULT_WT" rev-parse HEAD)" "$default_pre_head" "default reuse restores pre-rebase HEAD"
assert_eq "$(git -C "$DEFAULT_WT" status --porcelain)" "" "default reuse leaves the worktree clean"

# --- --restack: stop in the conflict state with continue/abort guidance --------
RESTACK_ROOT="$TMP_ROOT/restack"
make_conflict_pair "$RESTACK_ROOT" issue-restack
RESTACK_WT="$RESTACK_ROOT/trees/issue-restack"
git -C "$RESTACK_WT" push -q origin HEAD:refs/heads/issue-restack
restack_remote_before="$(git --git-dir="$RESTACK_ROOT/origin.git" rev-parse refs/heads/issue-restack)"
set +e
(
  cd "$RESTACK_ROOT/main" && \
    "$WORKTREE_SCRIPT" create issue-restack --restack >"$RESTACK_ROOT/create.out" 2>"$RESTACK_ROOT/create.err"
)
restack_code=$?
set -e
restack_err="$(cat "$RESTACK_ROOT/create.err")"
assert_eq "$restack_code" "1" "--restack reuse with conflict exits 1"
assert_rebase_in_progress "$RESTACK_WT" "--restack leaves the rebase paused in the conflict state"
assert_eq "$(git -C "$RESTACK_WT" diff --name-only --diff-filter=U)" "file.txt" "--restack leaves file.txt unmerged for resolution"
assert_contains "$restack_err" "file.txt" "--restack error names the conflicting file"
assert_contains "$restack_err" "add <file>" "--restack error documents per-file staging"
assert_contains "$restack_err" "rebase --continue" "--restack error documents the continue command"
assert_contains "$restack_err" "rebase --abort" "--restack error documents the abort escape hatch"

# The documented recovery path must actually work end to end.
printf 'resolved\n' > "$RESTACK_WT/file.txt"
git -C "$RESTACK_WT" add file.txt
GIT_EDITOR=true git -C "$RESTACK_WT" rebase --continue >/dev/null 2>&1
resolved_out=$(cd "$RESTACK_ROOT/main" && "$WORKTREE_SCRIPT" create issue-restack --reuse 2>"$RESTACK_ROOT/resolved.err")
assert_eq "$resolved_out" "$RESTACK_WT" "create after resolved restack finishes setup and prints the path"
assert_is_ancestor "$RESTACK_WT" origin/main HEAD "resolved restack branch contains origin/main"
assert_eq "$(cat "$RESTACK_WT/file.txt")" "resolved" "resolved restack keeps the manual resolution"
assert_eq "$(git -C "$RESTACK_WT" config --worktree --get vstack-restack.expectedRemoteOid)" "$restack_remote_before" "resolved restack preserves the exact pre-rewrite remote lease"
assert_eq "$(git -C "$RESTACK_WT" config --worktree --get vstack-restack.authorizedHead)" "$(git -C "$RESTACK_WT" rev-parse HEAD)" "resolved restack authorizes only its exact rewritten head"

set +e
(
  cd "$RESTACK_ROOT/main" && \
    "$WORKTREE_SCRIPT" push issue-restack >"$RESTACK_ROOT/push.out" 2>"$RESTACK_ROOT/push.err"
)
restack_push_code=$?
set -e
assert_eq "$restack_push_code" "0" "resolved supported restack pushes with its exact force-with-lease"
assert_eq "$(git --git-dir="$RESTACK_ROOT/origin.git" rev-parse refs/heads/issue-restack)" "$(git -C "$RESTACK_WT" rev-parse HEAD)" "resolved restack push publishes the rewritten head"
assert_eq "$(git -C "$RESTACK_WT" config --worktree --get vstack-restack.authorizedHead 2>/dev/null || true)" "" "successful push consumes restack authorization"

# --- Consecutive clean restacks preserve the exact authorization chain -------
CHAINED_ROOT="$TMP_ROOT/chained-restack"
make_published_clean_pair "$CHAINED_ROOT" issue-chained-restack
CHAINED_WT="$CHAINED_ROOT/trees/issue-chained-restack"
chained_remote_before="$(git --git-dir="$CHAINED_ROOT/origin.git" rev-parse refs/heads/issue-chained-restack)"
(cd "$CHAINED_ROOT/main" && "$WORKTREE_SCRIPT" create issue-chained-restack --restack >/dev/null 2>"$CHAINED_ROOT/first-restack.err")
chained_first_head="$(git -C "$CHAINED_WT" rev-parse HEAD)"
assert_eq "$(git -C "$CHAINED_WT" config --worktree --get vstack-restack.authorizedHead)" "$chained_first_head" "first clean restack authorizes its rewritten head"

printf 'advanced twice\n' > "$CHAINED_ROOT/main/main-advanced-twice.txt"
git -C "$CHAINED_ROOT/main" add main-advanced-twice.txt
git -C "$CHAINED_ROOT/main" commit -q -m 'advance main again'
git -C "$CHAINED_ROOT/main" push -q origin main
set +e
(
  cd "$CHAINED_ROOT/main" && \
    "$WORKTREE_SCRIPT" create issue-chained-restack --restack >"$CHAINED_ROOT/second-restack.out" 2>"$CHAINED_ROOT/second-restack.err"
)
chained_second_code=$?
set -e
chained_second_head="$(git -C "$CHAINED_WT" rev-parse HEAD)"
assert_eq "$chained_second_code" "0" "second clean restack accepts the preserved exact-match authorization"
assert_ne "$chained_second_head" "$chained_first_head" "second clean restack rewrites the previously authorized head"
assert_is_ancestor "$CHAINED_WT" origin/main HEAD "second clean restack contains the latest origin/main"
assert_eq "$(git -C "$CHAINED_WT" config --worktree --get vstack-restack.expectedRemoteOid)" "$chained_remote_before" "consecutive restacks retain the original exact remote lease"
assert_eq "$(git -C "$CHAINED_WT" config --worktree --get vstack-restack.authorizedHead)" "$chained_second_head" "second clean restack authorizes only its rewritten head"

set +e
(
  cd "$CHAINED_ROOT/main" && \
    "$WORKTREE_SCRIPT" push issue-chained-restack >"$CHAINED_ROOT/push.out" 2>"$CHAINED_ROOT/push.err"
)
chained_push_code=$?
set -e
assert_eq "$chained_push_code" "0" "consecutive clean restacks push with the original exact lease"
assert_eq "$(git --git-dir="$CHAINED_ROOT/origin.git" rev-parse refs/heads/issue-chained-restack)" "$chained_second_head" "consecutive restack push publishes the final rewritten head"
assert_eq "$(git -C "$CHAINED_WT" config --worktree --get vstack-restack.authorizedHead 2>/dev/null || true)" "" "consecutive restack push consumes authorization"

# --- Remote movement while conflict resolution is pending fails closed -------
PENDING_MOVE_ROOT="$TMP_ROOT/pending-move"
make_conflict_pair "$PENDING_MOVE_ROOT" issue-pending-move
PENDING_MOVE_WT="$PENDING_MOVE_ROOT/trees/issue-pending-move"
git -C "$PENDING_MOVE_WT" push -q origin HEAD:refs/heads/issue-pending-move
set +e
(cd "$PENDING_MOVE_ROOT/main" && "$WORKTREE_SCRIPT" create issue-pending-move --restack >/dev/null 2>"$PENDING_MOVE_ROOT/restack.err")
pending_restack_code=$?
set -e
assert_eq "$pending_restack_code" "1" "pending-movement setup pauses the supported restack"
printf 'resolved\n' > "$PENDING_MOVE_WT/file.txt"
git -C "$PENDING_MOVE_WT" add file.txt
GIT_EDITOR=true git -C "$PENDING_MOVE_WT" rebase --continue >/dev/null 2>&1
pending_move_old_oid="$(git --git-dir="$PENDING_MOVE_ROOT/origin.git" rev-parse refs/heads/issue-pending-move)"
pending_move_old_tree="$(git --git-dir="$PENDING_MOVE_ROOT/origin.git" rev-parse "${pending_move_old_oid}^{tree}")"
pending_move_external="$(GIT_AUTHOR_NAME=External GIT_AUTHOR_EMAIL=external@example.com GIT_COMMITTER_NAME=External GIT_COMMITTER_EMAIL=external@example.com git --git-dir="$PENDING_MOVE_ROOT/origin.git" commit-tree "$pending_move_old_tree" -p "$pending_move_old_oid" -m 'external pending movement')"
git --git-dir="$PENDING_MOVE_ROOT/origin.git" update-ref refs/heads/issue-pending-move "$pending_move_external"
set +e
(cd "$PENDING_MOVE_ROOT/main" && "$WORKTREE_SCRIPT" create issue-pending-move --reuse >/dev/null 2>"$PENDING_MOVE_ROOT/reuse.err")
pending_move_code=$?
set -e
assert_eq "$pending_move_code" "1" "remote movement during conflict resolution refuses restack authorization"
assert_contains "$(cat "$PENDING_MOVE_ROOT/reuse.err")" "changed while the supported restack was in progress" "pending remote movement reports the invalidated authorization"
assert_eq "$(git --git-dir="$PENDING_MOVE_ROOT/origin.git" rev-parse refs/heads/issue-pending-move)" "$pending_move_external" "pending remote movement remains untouched"

# --- Remote movement after authorization still fails the exact lease ---------
REMOTE_MOVE_ROOT="$TMP_ROOT/remote-move"
make_published_clean_pair "$REMOTE_MOVE_ROOT" issue-remote-move
REMOTE_MOVE_WT="$REMOTE_MOVE_ROOT/trees/issue-remote-move"
(cd "$REMOTE_MOVE_ROOT/main" && "$WORKTREE_SCRIPT" create issue-remote-move --reuse >/dev/null 2>"$REMOTE_MOVE_ROOT/create.err")
remote_move_old_oid="$(git --git-dir="$REMOTE_MOVE_ROOT/origin.git" rev-parse refs/heads/issue-remote-move)"
remote_move_old_tree="$(git --git-dir="$REMOTE_MOVE_ROOT/origin.git" rev-parse "${remote_move_old_oid}^{tree}")"
remote_move_external="$(GIT_AUTHOR_NAME=External GIT_AUTHOR_EMAIL=external@example.com GIT_COMMITTER_NAME=External GIT_COMMITTER_EMAIL=external@example.com git --git-dir="$REMOTE_MOVE_ROOT/origin.git" commit-tree "$remote_move_old_tree" -p "$remote_move_old_oid" -m 'external movement')"
git --git-dir="$REMOTE_MOVE_ROOT/origin.git" update-ref refs/heads/issue-remote-move "$remote_move_external"
set +e
(
  cd "$REMOTE_MOVE_ROOT/main" && \
    "$WORKTREE_SCRIPT" push issue-remote-move >"$REMOTE_MOVE_ROOT/push.out" 2>"$REMOTE_MOVE_ROOT/push.err"
)
remote_move_code=$?
set -e
assert_eq "$remote_move_code" "1" "remote movement after restack authorization rejects the push"
assert_eq "$(git --git-dir="$REMOTE_MOVE_ROOT/origin.git" rev-parse refs/heads/issue-remote-move)" "$remote_move_external" "exact lease preserves the externally advanced remote"
assert_contains "$(cat "$REMOTE_MOVE_ROOT/push.err")" "force-with-lease expectation" "remote movement reports exact-lease rejection"

# --- An unrelated local rewrite cannot reuse prior restack authorization ------
LOCAL_MOVE_ROOT="$TMP_ROOT/local-move"
make_published_clean_pair "$LOCAL_MOVE_ROOT" issue-local-move
LOCAL_MOVE_WT="$LOCAL_MOVE_ROOT/trees/issue-local-move"
(cd "$LOCAL_MOVE_ROOT/main" && "$WORKTREE_SCRIPT" create issue-local-move --reuse >/dev/null 2>"$LOCAL_MOVE_ROOT/create.err")
local_move_tree="$(git -C "$LOCAL_MOVE_WT" rev-parse "origin/main^{tree}")"
local_move_unexpected="$(git -C "$LOCAL_MOVE_WT" commit-tree "$local_move_tree" -p origin/main -m 'unexpected local rewrite')"
git -C "$LOCAL_MOVE_WT" reset -q --hard "$local_move_unexpected"
local_move_remote_before="$(git --git-dir="$LOCAL_MOVE_ROOT/origin.git" rev-parse refs/heads/issue-local-move)"
set +e
(
  cd "$LOCAL_MOVE_ROOT/main" && \
    "$WORKTREE_SCRIPT" push issue-local-move >"$LOCAL_MOVE_ROOT/push.out" 2>"$LOCAL_MOVE_ROOT/push.err"
)
local_move_code=$?
set -e
assert_eq "$local_move_code" "1" "unexpected local rewrite is not covered by prior restack authorization"
assert_eq "$(git --git-dir="$LOCAL_MOVE_ROOT/origin.git" rev-parse refs/heads/issue-local-move)" "$local_move_remote_before" "unexpected local rewrite leaves remote unchanged"
assert_contains "$(cat "$LOCAL_MOVE_ROOT/push.err")" "not contained in local branch" "unexpected local rewrite reports divergence"

# --- Clean-rebase reuse unchanged ----------------------------------------------
CLEAN_ROOT="$TMP_ROOT/clean"
make_repo "$CLEAN_ROOT/main"
git init -q --bare "$CLEAN_ROOT/origin.git"
git -C "$CLEAN_ROOT/main" remote add origin "$CLEAN_ROOT/origin.git"
git -C "$CLEAN_ROOT/main" push -q -u origin main
(cd "$CLEAN_ROOT/main" && "$WORKTREE_SCRIPT" create issue-clean >/dev/null 2>&1)
CLEAN_WT="$CLEAN_ROOT/trees/issue-clean"
printf 'fix\n' > "$CLEAN_WT/fix.txt"
git -C "$CLEAN_WT" add fix.txt
git -C "$CLEAN_WT" commit -q -m 'review fix'
printf 'advanced\n' > "$CLEAN_ROOT/main/main-advanced.txt"
git -C "$CLEAN_ROOT/main" add main-advanced.txt
git -C "$CLEAN_ROOT/main" commit -q -m 'advance main'
git -C "$CLEAN_ROOT/main" push -q origin main
clean_pre_head="$(git -C "$CLEAN_WT" rev-parse HEAD)"
clean_out=$(cd "$CLEAN_ROOT/main" && "$WORKTREE_SCRIPT" create issue-clean --reuse 2>"$CLEAN_ROOT/create.err")
assert_eq "$clean_out" "$CLEAN_WT" "clean reuse still prints the worktree path"
assert_ne "$(git -C "$CLEAN_WT" rev-parse HEAD)" "$clean_pre_head" "clean reuse rebased HEAD onto advanced origin/main"
assert_path_exists "$CLEAN_WT/main-advanced.txt" "clean reuse pulled in the advanced main content"
assert_is_ancestor "$CLEAN_WT" origin/main HEAD "clean reuse contains origin/main after rebase"
restack_noop_out=$(cd "$CLEAN_ROOT/main" && "$WORKTREE_SCRIPT" create issue-clean --restack 2>"$CLEAN_ROOT/restack-noop.err")
assert_eq "$restack_noop_out" "$CLEAN_WT" "--restack is a no-op when no rebase conflict occurs"

echo
printf 'pass: %d   fail: %d\n' "$PASS" "$FAIL"
[[ "$FAIL" -eq 0 ]]
