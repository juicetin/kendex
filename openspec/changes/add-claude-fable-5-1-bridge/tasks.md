## 1. Test-first model and runtime contracts

- [ ] 1.1 Add failing exact-selection and fallback-metadata tests for Fable 5.1 while retaining Fable 5. check:test-first
- [ ] 1.2 Add failing executable-resolution tests for configured, bundled, PATH, missing, unreadable, unparseable, and pre-2.1.255 candidates. check:quality-attribute-scenarios
- [ ] 1.3 Add failing rebuild tests that remove only Fable 5.1 thinking blocks and preserve valid text and paired tool blocks.
- [ ] 1.4 Record the red results and planned evidence bindings. check:catalog-validation

## 2. Bridge implementation

- [ ] 2.1 Update the Claude Agent SDK dependency and lockfile to a compatible release after current-library review. check:dependency-review
- [ ] 2.2 Add Fable 5.1 model metadata, leading picker order, exact-ID routing, `[1m]` request naming, and existing fallback-policy coverage.
- [ ] 2.3 Add model-aware Claude Code 2.1.255 compatibility selection before session synchronization and return one actionable failure when no candidate qualifies.
- [ ] 2.4 Filter replayed thinking blocks only on Fable 5.1 rebuilds without changing ordinary resume or other-model history.
- [ ] 2.5 Rebuild the tracked bundle and confirm the package exports remain unchanged. check:build

## 3. Deterministic validation

- [ ] 3.1 Run `npm run typecheck --prefix pi-extensions/pi-claude-bridge` and save the result. check:typecheck
- [ ] 3.2 Run `npm run test:ci --prefix pi-extensions/pi-claude-bridge` and save the result. check:baseline-tests
- [ ] 3.3 Run the focused model, executable, rebuild, routing, and regression cases; record green evidence. check:quality-attribute-scenarios
- [ ] 3.4 Run `fallow-check` on changed source and dependency surfaces. check:fallow
- [ ] 3.5 Run the bounded ten-run executable-selection profile and require p95 added time at or below 500 ms. check:bounded-profiling
- [ ] 3.6 Review route, compatibility-error, and served-model evidence boundaries. check:product-observability
- [ ] 3.7 Review diagnostic and acceptance fields for allowlisted, credential-free output. check:structured-logging
- [ ] 3.8 Run `auth-check` for profile selection, credential scope, rotation, and no-credit-purchase boundaries. check:auth-review
- [ ] 3.9 Run `security-scan` and `npm audit --omit=dev --prefix pi-extensions/pi-claude-bridge`. check:security-scan
- [ ] 3.10 Run `tools/guard` and `git diff --check`.

## 4. Real subscription acceptance

- [ ] 4.1 Run a real Pi JSON conversation through `pi-claude/claude-fable-5-1` using only an existing authorized Team profile and a known non-secret fixture.
- [ ] 4.2 Prove the actual served model from assistant transport metadata, not generated self-identification, and record a redacted account-route identity.
- [ ] 4.3 Prove one Pi-owned tool request, matching Pi result, and final Fable 5.1 response; stop without enabling or buying credits if access is unavailable.

## 5. Documentation and package checks

- [ ] 5.1 Update `pi-extensions/pi-claude-bridge/README.md`, `DEVELOPMENT.md`, affected active architecture text if needed, and a consumer changelog fragment. check:documentation-sync
- [ ] 5.2 Run `(cd pi-extensions/pi-claude-bridge && npm pack --dry-run)` and inspect the shipped bundle and metadata. check:package-verification
- [ ] 5.3 Run `openspec validate add-claude-fable-5-1-bridge --strict` and `openspec validate --all --strict`. check:openspec-validation

## 6. Review and lifecycle evidence

- [ ] 6.1 After focused validation passes, run `code-simplifier`, rerun affected checks, and record any edits. check:code-simplification
- [ ] 6.2 Run a fresh read-only maintainability review of model, executable, and rebuild cohesion. check:maintainability-review
- [ ] 6.3 Run a final fresh-context code review after deterministic checks and fix blocking findings. check:code-review
- [ ] 6.4 Confirm no Agent Skill behavior changed and route the confirmed verifier guidance gap through the skill feedback process. check:skill-evaluation check:skill-governance
- [ ] 6.5 Record command duration, retries, review yield, unavailable cost fields, and waiver count. check:workflow-telemetry
- [ ] 6.6 Obtain a fresh independent Satisfaction Review over the final diff, specifications, and evidence. check:satisfaction-review
- [ ] 6.7 Create and validate the typed retrospective after Satisfaction Review. check:retrospective

## 7. Fork delivery and global startup

- [ ] 7.1 Commit validated work, push only to `https://github.com/juicetin/kendex`, and do not open an upstream pull request.
- [ ] 7.2 Install the committed package globally, start a disposable Pi process with normal extensions, require its exact marker within 30 seconds, and roll back the global install if the gate fails.

check:selection-review is satisfied by the accepted `selection-review.json` that authorizes this task plan. check:api-contract, check:ui-evidence, and check:mutation-testing are not applicable as recorded in `check-selection.json`.
