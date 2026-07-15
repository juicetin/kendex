#!/usr/bin/env bash
# Regression tests for label-add capability and policy preflight.
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../.." && pwd)"
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

assert_eq() {
    local got="$1" want="$2" name="$3"
    if [ "$got" != "$want" ]; then
        fail "$name: expected '$want', got '$got'"
    fi
    printf 'ok - %s\n' "$name"
}

assert_no_mutation() {
    local name="$1"
    if grep -Eq '^(pr|issue) edit' "$TMP_ROOT/gh.calls"; then
        fail "$name: target mutation was attempted"
    fi
    printf 'ok - %s\n' "$name"
}

mkdir -p "$TMP_ROOT/repo" "$TMP_ROOT/bin"
git -C "$TMP_ROOT/repo" init -q

cat > "$TMP_ROOT/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$STUB_CALLS"

case "${1:-} ${2:-}" in
    "repo view")
        if [ "${STUB_REPO_FAILURE:-0}" = "1" ]; then
            printf 'repository unavailable\n' >&2
            exit 1
        fi
        printf '{"nameWithOwner":"owner/repo","viewerPermission":"%s"}\n' "${STUB_PERMISSION:-WRITE}"
        ;;
    "api repos/owner/repo/labels/"*)
        if [ "${STUB_LABEL_FAILURE:-0}" = "1" ]; then
            printf 'gh: server error (HTTP 500)\n' >&2
            exit 1
        fi
        if [ "${STUB_LABEL_EXISTS:-1}" = "1" ]; then
            printf '{"name":"label"}\n'
            exit 0
        fi
        printf '{"message":"Not Found","status":"404"}\n' >&2
        printf 'gh: Not Found (HTTP 404)\n' >&2
        exit 1
        ;;
    "pr edit"|"issue edit")
        printf 'updated\n'
        ;;
    *)
        printf 'unexpected gh call: %s\n' "$*" >&2
        exit 1
        ;;
esac
EOF
chmod +x "$TMP_ROOT/bin/gh"

run_label_add() {
    env \
        PATH="$TMP_ROOT/bin:$PATH" \
        STUB_CALLS="$TMP_ROOT/gh.calls" \
        STUB_PERMISSION="${STUB_PERMISSION:-WRITE}" \
        STUB_LABEL_EXISTS="${STUB_LABEL_EXISTS:-1}" \
        STUB_REPO_FAILURE="${STUB_REPO_FAILURE:-0}" \
        STUB_LABEL_FAILURE="${STUB_LABEL_FAILURE:-0}" \
        "$REPO_ROOT/skills/github/scripts/commands/label-add.sh" "$@"
}

cd "$TMP_ROOT/repo"

: >"$TMP_ROOT/gh.calls"
output="$(run_label_add 42 needs-review)"
assert_eq "$output" "updated" "required mode defaults and mutates after preflight"
assert_eq "$(sed -n '1p' "$TMP_ROOT/gh.calls")" "repo view --json nameWithOwner,viewerPermission" "repository capability is checked first"
assert_eq "$(sed -n '2p' "$TMP_ROOT/gh.calls")" "api repos/owner/repo/labels/needs-review" "live label inventory is checked second"
assert_eq "$(sed -n '3p' "$TMP_ROOT/gh.calls")" "pr edit 42 --add-label needs-review" "mutation runs only after preflight"

: >"$TMP_ROOT/gh.calls"
set +e
required_missing="$(STUB_LABEL_EXISTS=0 run_label_add 42 needs-review --required 2>&1)"
required_missing_rc=$?
set -e
assert_eq "$required_missing_rc" "78" "required missing label is a configuration error"
assert_eq "$(jq -r .status <<<"$required_missing")" "configuration_error" "required missing label has structured status"
assert_eq "$(jq -r .reason <<<"$required_missing")" "label_missing" "required missing label has structured reason"
assert_eq "$(jq -r .message <<<"$required_missing")" 'Required label "needs-review" is not configured in owner/repo' "required missing label explains repository configuration"
assert_no_mutation "required missing label stops before mutation"

: >"$TMP_ROOT/gh.calls"
optional_missing="$(STUB_LABEL_EXISTS=0 run_label_add 42 informational --optional)"
assert_eq "$(jq -r .status <<<"$optional_missing")" "optional_unsupported" "optional missing label is a supported skip"
assert_eq "$(jq -r .reason <<<"$optional_missing")" "label_missing" "optional missing label explains why it skipped"
assert_eq "$(jq -r .message <<<"$optional_missing")" 'Optional label "informational" is not configured; mutation skipped' "optional missing label reports skipped mutation"
assert_no_mutation "optional missing label skips mutation"

: >"$TMP_ROOT/gh.calls"
set +e
required_read="$(STUB_PERMISSION=READ run_label_add 42 needs-review --required 2>&1)"
required_read_rc=$?
set -e
assert_eq "$required_read_rc" "77" "required read-only identity is a capability error"
assert_eq "$(jq -r .status <<<"$required_read")" "capability_error" "required permission failure has structured status"
assert_no_mutation "required permission failure stops before inventory and mutation"

: >"$TMP_ROOT/gh.calls"
optional_read="$(STUB_PERMISSION=READ run_label_add 42 informational --optional)"
assert_eq "$(jq -r .status <<<"$optional_read")" "optional_unsupported" "optional read-only identity is a supported skip"
assert_eq "$(jq -r .reason <<<"$optional_read")" "insufficient_permission" "optional permission skip explains why"
assert_no_mutation "optional permission skip stops mutation"

: >"$TMP_ROOT/gh.calls"
set +e
optional_lookup_failure="$(STUB_LABEL_FAILURE=1 run_label_add 42 informational --optional 2>&1)"
optional_lookup_failure_rc=$?
set -e
assert_eq "$optional_lookup_failure_rc" "1" "optional mode does not hide operational label lookup failures"
assert_eq "$(jq -r .status <<<"$optional_lookup_failure")" "preflight_failed" "operational lookup failure has structured status"
assert_no_mutation "operational lookup failure stops mutation"

: >"$TMP_ROOT/gh.calls"
encoded_output="$(run_label_add 42 needs/review --required)"
assert_eq "$encoded_output" "updated" "encoded label succeeds"
assert_eq "$(sed -n '2p' "$TMP_ROOT/gh.calls")" "api repos/owner/repo/labels/needs%2Freview" "label lookup URL-encodes the label"

: >"$TMP_ROOT/gh.calls"
set +e
conflicting_policy="$(run_label_add 42 needs-review --required --optional 2>&1)"
conflicting_policy_rc=$?
set -e
assert_eq "$conflicting_policy_rc" "2" "conflicting policy flags are rejected"
assert_no_mutation "conflicting policy flags stop before mutation"

printf 'all pass\n'
