## 1. Test-first model and runtime contracts

- [x] 1.1 Add failing exact-selection and fallback-metadata tests for Fable 5.1 while retaining Fable 5. check:test-first
- [x] 1.2 Add failing executable-resolution tests for configured, bundled, PATH, missing, unreadable, unparseable, and pre-2.1.255 candidates. check:quality-attribute-scenarios
- [x] 1.3 Add failing rebuild tests that remove only Fable 5.1 thinking blocks and preserve valid text and paired tool blocks.
- [x] 1.4 Record the red results and planned evidence bindings. check:catalog-validation

## 2. Bridge implementation

- [x] 2.1 Update the Claude Agent SDK dependency and lockfile to a compatible release after current-library review. check:dependency-review
- [x] 2.2 Add Fable 5.1 model metadata, leading picker order, exact-ID routing, `[1m]` request naming, and existing fallback-policy coverage.
- [x] 2.3 Add model-aware Claude Code 2.1.255 compatibility selection before session synchronization and return one actionable failure when no candidate qualifies.
- [x] 2.4 Filter replayed thinking blocks only on Fable 5.1 rebuilds without changing ordinary resume or other-model history.
- [x] 2.5 Rebuild the tracked bundle through the package build path, preserve dependency template-literal bytes while limiting Git's trailing-space exemption to generated bridge bundles, and confirm the package exports remain unchanged. check:build

## 3. Deterministic validation

- [x] 3.1 Run `npm run typecheck --prefix pi-extensions/pi-claude-bridge` and save the result. check:typecheck
- [x] 3.2 Run `npm run test:ci --prefix pi-extensions/pi-claude-bridge` and save the result. check:baseline-tests
- [x] 3.3 Run the focused model, executable, rebuild, routing, and regression cases; record green evidence. check:quality-attribute-scenarios
- [x] 3.4 Run `fallow-check` on changed source and dependency surfaces. check:fallow
- [x] 3.5 Run the bounded ten-run executable-selection profile and require p95 added time at or below 500 ms. check:bounded-profiling
- [x] 3.6 Review route, compatibility-error, and served-model evidence boundaries. check:product-observability
- [x] 3.7 Review diagnostic and acceptance fields for allowlisted, credential-free output. check:structured-logging
- [x] 3.8 Run `auth-check` for profile selection, credential scope, rotation, and no-credit-purchase boundaries. check:auth-review
- [x] 3.9 Run `security-scan` and `npm audit --omit=dev --prefix pi-extensions/pi-claude-bridge`. check:security-scan
- [x] 3.10 Run `tools/guard` and `git diff --check`; record unrelated host and inherited failures.

## 4. Real subscription acceptance

- [x] 4.1 Run a real Pi JSON conversation through `pi-claude/claude-fable-5-1` using only an existing authorized Team profile and a known non-secret fixture.
- [x] 4.2 Prove the actual served model from assistant transport metadata, not generated self-identification, and record a redacted account-route identity.
- [x] 4.3 Prove one Pi-owned tool request, matching Pi result, and final Fable 5.1 response; stop without enabling or buying credits if access is unavailable.

## 5. Documentation and package checks

- [x] 5.1 Update `pi-extensions/pi-claude-bridge/README.md`, `DEVELOPMENT.md`, affected active architecture text if needed, and a consumer changelog fragment. check:documentation-sync
- [x] 5.2 Run `(cd pi-extensions/pi-claude-bridge && npm pack --dry-run)` and inspect the shipped bundle and metadata. check:package-verification
- [x] 5.3 Run `openspec validate add-claude-fable-5-1-bridge --strict` and `openspec validate --all --strict`. check:openspec-validation

## 6. Review and lifecycle evidence

- [x] 6.1 After focused validation passes, run `code-simplifier`, rerun affected checks, and record any edits. check:code-simplification
- [x] 6.2 Run a fresh read-only maintainability review of model, executable, and rebuild cohesion. check:maintainability-review
- [x] 6.3 Run a final fresh-context code review after deterministic checks and fix blocking findings. check:code-review
- [x] 6.4 Confirm no bridge Agent Skill behavior changed and route the confirmed verifier guidance gap through the skill feedback process. check:skill-evaluation check:skill-governance
- [x] 6.5 Record command duration, retries, review yield, unavailable cost fields, and waiver count. check:workflow-telemetry
- [x] 6.6 Obtain a fresh independent Satisfaction Review over the final diff, specifications, and evidence. check:satisfaction-review
- [x] 6.7 Create and validate the typed retrospective after Satisfaction Review. check:retrospective

## 7. Fork delivery and global startup

- [x] 7.1 Commit validated work, push only to `https://github.com/juicetin/kendex`, and do not open an upstream pull request.
- [x] 7.2 Install the committed package globally, start a disposable Pi process with normal extensions, require its exact marker within 30 seconds, and roll back the global install if the gate fails.

check:selection-review is satisfied by the accepted `selection-review.json` that authorizes this task plan. check:api-contract, check:ui-evidence, and check:mutation-testing are not applicable as recorded in `check-selection.json`.
