# Quality plan

## Baseline commands

Run from the repository root and store each command result under `evidence/`:

- `npm run typecheck --prefix pi-extensions/pi-claude-bridge`
- `npm run test:ci --prefix pi-extensions/pi-claude-bridge`
- `tools/guard`
- `npm run build --prefix pi-extensions/pi-claude-bridge`
- `openspec validate add-claude-fable-5-1-bridge --strict`
- `openspec validate --all --strict`
- `git diff --check`
- `(cd pi-extensions/pi-claude-bridge && npm pack --dry-run)`

A final fresh-context code review is required after deterministic checks pass. Fix blocking findings and rerun affected commands.

## Impact-based checks

| Check | Applies | Reason | Command or skill | Evidence |
| --- | --- | --- | --- | --- |
| Dependency review | yes | The existing Claude Agent SDK dependency and lockfile must move to a release that bundles a Claude Code runtime compatible with Fable 5.1. | `dependency-survey` and package diff inspection | `evidence/dependency-review.json` |
| Test-first implementation | yes | The bridge gains a selectable model, runtime rejection behavior, model-specific history filtering, and real routing acceptance criteria. | `tdd-enforcement` with focused red and green unit or integration cases | `evidence/test-first.json` |
| Auth review | yes | The new model passes through existing subscription-profile selection, credential scoping, usage limits, and account rotation, which are authentication and authorization boundaries. | `auth-check` | `evidence/auth-review.json` |
| Security scan | yes | The new model passes through existing subscription-profile selection, credential scoping, usage limits, and account rotation, which are authentication and authorization boundaries. | `security-scan` plus `npm audit --omit=dev --prefix pi-extensions/pi-claude-bridge` | `evidence/security-scan.json` |
| HTTP API contract | no | The change adds no HTTP endpoint, wire protocol, or OpenAPI contract. | n/a | `impact.json` and changed-path inspection |
| UI visual and accessibility evidence | no | The Pi model picker consumes provider metadata; no kendex React or Tauri screen changes. | n/a | `impact.json` and changed-path inspection |
| Code simplification | yes | Model routing, executable selection, and session filtering change control flow in several TypeScript modules. | `code-simplifier` after passing focused tests | `evidence/code-simplification.json` |
| Fallow | yes | The change modifies TypeScript module boundaries and a package dependency, so changed-code dependency and structure findings require review. | `fallow-check` | `evidence/fallow.json` |

## Complete catalog disposition rendering

Machine markers for all 28 dispositions:

- check:catalog-validation
- check:baseline-tests
- check:typecheck
- check:build
- check:openspec-validation
- check:dependency-review
- check:test-first
- check:selection-review
- check:satisfaction-review
- check:code-review
- check:auth-review
- check:security-scan
- check:api-contract
- check:ui-evidence
- check:quality-attribute-scenarios
- check:product-observability
- check:structured-logging
- check:bounded-profiling
- check:workflow-telemetry
- check:skill-evaluation
- check:code-simplification
- check:maintainability-review
- check:fallow
- check:mutation-testing
- check:documentation-sync
- check:package-verification
- check:retrospective
- check:skill-governance

| Check ID | Applies | Exact method | Planned evidence |
| --- | --- | --- | --- |
| `catalog-validation` | yes | Check-lifecycle validator plus strict OpenSpec validation | `evidence/catalog-validation.json` |
| `baseline-tests` | yes | `npm run test:ci --prefix pi-extensions/pi-claude-bridge` | `evidence/baseline-tests.json` |
| `typecheck` | yes | `npm run typecheck --prefix pi-extensions/pi-claude-bridge` | `evidence/typecheck.json` |
| `build` | yes | `npm run build --prefix pi-extensions/pi-claude-bridge` | `evidence/build.json` |
| `openspec-validation` | yes | `openspec validate add-claude-fable-5-1-bridge --strict && openspec validate --all --strict` | `evidence/openspec-validation.json` |
| `dependency-review` | yes | `dependency-survey` | `evidence/dependency-review.json` |
| `test-first` | yes | `tdd-enforcement` | `evidence/test-first.json` |
| `selection-review` | yes | Fresh-context read-only Selection Review | `selection-review.md`, `selection-review.json`, `evidence/selection-review.json` |
| `satisfaction-review` | yes | Fresh-context read-only Satisfaction Review | `satisfaction-review.json`, its source report, `evidence/satisfaction-review.json` |
| `code-review` | yes | `code-review` on the final task-owned diff | `evidence/code-review.json` |
| `auth-review` | yes | `auth-check` | `evidence/auth-review.json` |
| `security-scan` | yes | `security-scan` and production-dependency audit | `evidence/security-scan.json` |
| `api-contract` | no | n/a; no HTTP contract changes | `check-selection.json` reason |
| `ui-evidence` | no | n/a; no rendered UI changes | `check-selection.json` reason |
| `quality-attribute-scenarios` | yes | Focused model, executable, rebuild, routing, and tool-loop scenarios | `evidence/quality-attribute-scenarios.json` |
| `product-observability` | yes | Review error, route, and served-model evidence boundaries | `evidence/product-observability.json` |
| `structured-logging` | yes | Review allowlisted fields and secret-free diagnostics | `evidence/structured-logging.json` |
| `bounded-profiling` | yes | Ten-run local executable-selection profile | `evidence/bounded-profiling.json` |
| `workflow-telemetry` | yes | Record duration, retries, review yield, and unavailable cost fields | `evidence/workflow-telemetry.json` |
| `skill-evaluation` | yes | Confirm no Agent Skill behavior changed; record the verifier skill-gap disposition | `evidence/skill-evaluation.json` |
| `code-simplification` | yes | `code-simplifier` after focused tests pass | `evidence/code-simplification.json` |
| `maintainability-review` | yes | `strict-maintainability-review` | `evidence/maintainability-review.json` |
| `fallow` | yes | `fallow-check` | `evidence/fallow.json` |
| `mutation-testing` | no | n/a; no approved scope or budget | `check-selection.json` reason |
| `documentation-sync` | yes | README, development notes, architecture, changelog, and diff inspection | `evidence/documentation-sync.json` |
| `package-verification` | yes | `(cd pi-extensions/pi-claude-bridge && npm pack --dry-run)` | `evidence/package-verification.json` |
| `retrospective` | yes | Validate typed `retrospective.json` after Satisfaction Review | `retrospective.json`, `evidence/retrospective.json` |
| `skill-governance` | yes | Search the Skill Index, route the confirmed verifier guidance gap, and require approval for any guidance mutation | `evidence/skill-governance.json` |

## Quality attribute scenarios

1. Exact model selection: 100% of exact Fable 5 and Fable 5.1 cases route the requested version; Fable 5.1 is first in the picker.
2. Runtime compatibility: 100% of configured, bundled, PATH, missing, unreadable, unparseable, and too-old executable cases choose a valid 2.1.255+ runtime or fail before session synchronization.
3. Rebuild integrity: every Fable 5.1 rebuild fixture contains zero replayed thinking blocks and preserves all valid text and paired tool-call/result blocks; other model fixtures remain unchanged.
4. Real route: one existing authorized Team profile returns a real response whose assistant transport metadata identifies Fable 5.1.
5. Pi tool ownership: the same real run records at least one Pi tool request, its matching result, and a final response containing the known fixture value.
6. Performance: across ten local candidate-resolution runs, the compatibility check adds no more than 500 ms at p95 before query start.
7. Secret handling: committed evidence contains zero access tokens, authorization headers, credential file contents, or raw subscription secrets.

## Mutation testing

- Human approved: no
- Approval reference: n/a
- Not applicable reason: The user did not approve a bounded mutation-testing scope, runtime budget, fast test command, or evidence threshold.
- Scope: n/a
- Runtime budget: n/a
- Fast test command: n/a
- Evidence threshold: n/a

## Real subscription acceptance

Use only existing Team subscription authorization. Run Pi in JSON mode with `pi-claude/claude-fable-5-1` and a prompt that requires reading a known non-secret fixture. Preserve the raw local output outside Git, then commit only a redacted assertion record under `evidence/`. The record must bind the requested model, transport-derived served model, redacted route identity, tool-call ID, matching tool-result ID, expected fixture value, exit status, and Git commit. Stop if access requires enabling or buying credits.

## Post-release obligations

None. The real subscription route, global package installation, and disposable fresh Pi startup check are pre-completion checks.
