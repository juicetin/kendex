## Context

See `proposal.md` for motivation and `specs/pi-claude-fable-5-1/spec.md` for behavior. The bridge already owns model projection, Claude query options, executable preflight, account-router integration, transport model metadata, and session rebuilds. It supports Fable 5 but has no Fable 5.1 catalog entry, 2.1.255 compatibility check, or model-specific rebuild filtering.

## Goals / Non-Goals

**Goals:**

- Add Fable 5.1 by extending the current bridge seams instead of copying or replacing the provider.
- Fail before destructive session synchronization when the selected Claude Code runtime cannot support Fable 5.1.
- Keep account rotation, connector isolation, session ownership, and Pi tool dispatch unchanged.
- Produce real, redacted evidence for served model identity, account routing, and one Pi-owned tool loop.

**Non-Goals:**

- Rework the account-router protocol, connector policy, provider ID, or stored account identity.
- Add direct Anthropic API or OAuth impersonation paths.
- Enable or purchase usage credits.
- Open or merge an upstream pull request.

## Decisions

### Keep catalog identity separate from the Claude Code request name

Add `claude-fable-5-1` as the Pi model ID and leading picker entry. Prefer current Pi catalog metadata when present; otherwise use bridge metadata for 1M context, 128K output, and direct xhigh/max mapping. Convert only the selected Fable 5.1 query to `claude-fable-5-1[1m]` at the Claude Code boundary.

Alternative: replace the Fable 5 entry. Rejected because existing callers need its exact ID and behavior.

### Gate Fable 5.1 on Claude Code 2.1.255

Extend executable resolution to describe configured, Agent SDK bundled, and allowed PATH candidates with parsed versions. A configured executable remains authoritative but must meet the Fable 5.1 minimum. Without one, use the bundled executable when compatible; otherwise use a compatible PATH executable outside isolated mode. If none qualifies, return one error with the minimum and detected candidate versions. Run this check before `syncSharedSession`.

Update `@anthropic-ai/claude-agent-sdk` to a release that bundles a compatible Claude Code runtime and update its lockfile. No new package is introduced.

Alternative: always request `[1m]` through the current SDK. Rejected because older Claude Code builds can silently use the wrong catalog or context behavior.

### Apply model policy in focused helpers

Keep model ordering, fallback classification, and request-name conversion in the model/query layer. Keep executable version discovery in `claude-executable.ts`. Pass the selected model ID to these helpers rather than adding Fable checks across the stream loop.

Alternative: special-case the whole request in `index.ts`. Rejected because it would duplicate routing and session logic.

### Filter replayed thinking only during a Fable 5.1 rebuild

Use the model ID already passed to `syncSharedSession` to omit imported thinking blocks on its rebuild path. Do not change ordinary resume, text blocks, tool calls, paired results, or rebuilds for other models.

Alternative: remove thinking blocks from every rebuilt session. Rejected because the compatibility issue is specific to Fable 5.1 prefix-bound thinking signatures.

### Treat transport metadata as model identity evidence

The real acceptance run records Pi JSON events and asserts the assistant message's transport-derived model field. It also asserts at least one Pi tool call/result pair and a final answer based on a known fixture. Account evidence records only a stable profile label or digest, subscription class, route decision, and model; it excludes tokens, credential paths, and raw authorization data.

Alternative: ask the model which model it is. Rejected because generated text does not prove routing.

## Risks / Trade-offs

- [Claude Code version output changes] -> Parse only a tested semantic-version token and fail with the raw candidate path plus a bounded, credential-free reason.
- [Bundled executable layout changes across SDK releases] -> Resolve it through the installed SDK package and cover missing, unreadable, old, and compatible candidates in unit tests.
- [Fable 5.1 is unavailable on an existing Team seat] -> Stop the real check without enabling credits or claiming success from unit evidence.
- [Thinking filtering drops valid history] -> Limit filtering to rebuild import for the exact Fable 5.1 model and test preserved text and tool pairs.
- [New model logic changes Fable 5 fallback behavior] -> Add regression tests for unmanaged fallback, managed rotation before fallback, and routed output metadata for both Fable versions.

## Migration Plan

1. Update the SDK dependency and lockfile, model projection, query naming, executable compatibility checks, and rebuild filtering.
2. Build the tracked bundle and run unit, integration, package, guard, review, and documentation checks.
3. Install the validated fork package globally, then start a disposable Pi process with normal extensions and require its marker within 30 seconds.
4. If the global startup gate fails, restore the prior global package and rerun the same startup check before reporting the change complete.
