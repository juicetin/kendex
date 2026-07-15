#!/usr/bin/env bash
# GitHub label add wrapper.
# Runs `gh pr edit --add-label` or `gh issue edit --add-label`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/label-activity.sh
source "$SCRIPT_DIR/../lib/label-activity.sh"
# shellcheck source=../lib/gh-auth.sh
source "$SCRIPT_DIR/../lib/gh-auth.sh"

show_help() {
    cat <<'EOF'
Add a label to a PR or issue.

Usage: label-add.sh <pr-or-issue-ref> <label> [--reason TEXT] [--issue] [--required|--optional]

Arguments:
  pr-or-issue-ref   PR number, branch ref, or issue number. Empty
                    string defers to gh's current-branch resolution.
  label             Label name to add (single label per call).

Options:
  --reason TEXT     Human-readable reason recorded in activity details.
  --issue           Treat the ref as an issue (default: PR).
  --pr              Treat the ref as a PR (default).
  --required        Treat a missing label or write capability as an error
                    (default; exit 78 for missing label, 77 for permission).
  --optional        Skip with a structured optional_unsupported result when
                    the repository lacks the label or label-write capability.
  --help, -h        Show this help.

Examples:
  label-add.sh 44 needs-qa --reason "ready for QA review"
  label-add.sh 123 needs-triage --issue
  label-add.sh 44 informational --optional
EOF
}

emit_preflight_result() {
    local status="$1" policy="$2" reason="$3" repository="$4" label="$5"
    local permission="${6:-}" detail="${7:-}"
    jq -cn \
        --arg status "$status" \
        --arg policy "$policy" \
        --arg reason "$reason" \
        --arg repository "$repository" \
        --arg label "$label" \
        --arg viewer_permission "$permission" \
        --arg detail "$detail" \
        '{
           status:$status,
           policy:$policy,
           reason:$reason,
           repository:$repository,
           label:$label,
           message:(
             if $status == "configuration_error" then
               "Required label \"\($label)\" is not configured in \($repository)"
             elif $status == "capability_error" then
               "Required label \"\($label)\" cannot be applied with \($viewer_permission) repository permission"
             elif $reason == "label_missing" then
               "Optional label \"\($label)\" is not configured; mutation skipped"
             elif $reason == "insufficient_permission" then
               "Optional label \"\($label)\" cannot be applied with \($viewer_permission) repository permission; mutation skipped"
             else
               "Label capability preflight failed before mutation"
             end
           )
         }
         + (if $viewer_permission == "" then {} else {viewer_permission:$viewer_permission} end)
         + (if $detail == "" then {} else {detail:$detail} end)'
}

preflight_label_add() {
    local label="$1" policy="$2"
    local repo_json="" repo_rc=0 repository="" permission=""
    repo_json="$(gh repo view --json nameWithOwner,viewerPermission 2>&1)" || repo_rc=$?
    if [ "$repo_rc" -ne 0 ]; then
        emit_preflight_result \
            "preflight_failed" "$policy" "repository_lookup_failed" "" "$label" "" "$repo_json" >&2
        return "$repo_rc"
    fi

    repository="$(printf '%s' "$repo_json" | jq -r '.nameWithOwner // empty')"
    permission="$(printf '%s' "$repo_json" | jq -r '.viewerPermission // empty')"
    if [ -z "$repository" ] || [ -z "$permission" ]; then
        emit_preflight_result \
            "preflight_failed" "$policy" "invalid_repository_metadata" "$repository" "$label" "$permission" "$repo_json" >&2
        return 1
    fi

    case "$permission" in
        ADMIN|MAINTAIN|WRITE|TRIAGE) ;;
        *)
            if [ "$policy" = "optional" ]; then
                emit_preflight_result \
                    "optional_unsupported" "$policy" "insufficient_permission" "$repository" "$label" "$permission"
                return 10
            fi
            emit_preflight_result \
                "capability_error" "$policy" "insufficient_permission" "$repository" "$label" "$permission" >&2
            return 77
            ;;
    esac

    local encoded_label="" lookup_output="" lookup_rc=0
    encoded_label="$(jq -nr --arg label "$label" '$label | @uri')"
    lookup_output="$(gh api "repos/$repository/labels/$encoded_label" 2>&1)" || lookup_rc=$?
    if [ "$lookup_rc" -eq 0 ]; then
        return 0
    fi

    if printf '%s' "$lookup_output" | grep -Eq 'HTTP 404|"status"[[:space:]]*:[[:space:]]*"?404"?|Not Found'; then
        if [ "$policy" = "optional" ]; then
            emit_preflight_result \
                "optional_unsupported" "$policy" "label_missing" "$repository" "$label" "$permission"
            return 10
        fi
        emit_preflight_result \
            "configuration_error" "$policy" "label_missing" "$repository" "$label" "$permission" >&2
        return 78
    fi

    emit_preflight_result \
        "preflight_failed" "$policy" "label_lookup_failed" "$repository" "$label" "$permission" "$lookup_output" >&2
    return "$lookup_rc"
}

main() {
    local ref="" label="" reason="" kind="pr" policy="required" policy_set=""
    local positional=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --help|-h) show_help; exit 0 ;;
            --reason) reason="${2:-}"; shift 2 ;;
            --reason=*) reason="${1#--reason=}"; shift ;;
            --issue) kind="issue"; shift ;;
            --pr) kind="pr"; shift ;;
            --required|--optional)
                local requested_policy="${1#--}"
                if [ -n "$policy_set" ] && [ "$policy_set" != "$requested_policy" ]; then
                    echo "label-add: --required and --optional are mutually exclusive" >&2
                    exit 2
                fi
                policy="$requested_policy"
                policy_set="$requested_policy"
                shift
                ;;
            --) shift; break ;;
            -*)
                echo "label-add: unknown flag: $1" >&2
                exit 2
                ;;
            *)
                case "$positional" in
                    0) ref="$1"; positional=1 ;;
                    1) label="$1"; positional=2 ;;
                    *) echo "label-add: unexpected positional: $1" >&2; exit 2 ;;
                esac
                shift
                ;;
        esac
    done

    if [ -z "$label" ]; then
        echo "label-add: <label> is required" >&2
        show_help >&2
        exit 2
    fi

    local project_root
    project_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
    vstack_github_load_project_env_preserving_caller "$project_root"
    vstack_github_apply_selected_auth_token router || true
    vstack_github_sanitize_gh_env

    local preflight_rc=0
    preflight_label_add "$label" "$policy" || preflight_rc=$?
    if [ "$preflight_rc" -eq 10 ]; then
        exit 0
    fi
    if [ "$preflight_rc" -ne 0 ]; then
        exit "$preflight_rc"
    fi

    local rc=0
    if [ "$kind" = "issue" ]; then
        gh issue edit "$ref" --add-label "$label" || rc=$?
    else
        gh pr edit "$ref" --add-label "$label" || rc=$?
    fi
    if [ "$rc" -ne 0 ]; then
        exit "$rc"
    fi

    emit_label_activity add "$kind" "$ref" "$label" "$reason" || true
    exit 0
}

main "$@"
