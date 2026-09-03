## Why

Claude Fable 5.1 is available through current Claude Code subscriptions, but the kendex Pi Claude bridge exposes only Fable 5. Users need the new model without losing the bridge's team-profile routing, Pi-owned tool execution, or session safety.

## What Changes

- Add `pi-claude/claude-fable-5-1` as the leading Fable model while keeping `pi-claude/claude-fable-5` selectable by its exact ID.
- Route Fable 5.1 through Claude Code with its supported one-million-token request form and reject unsupported executable versions with an actionable error.
- Preserve model-scoped team subscription selection, account rotation, and post-rotation fallback behavior for both Fable versions.
- Prevent rebuilt Fable 5.1 sessions from replaying thinking blocks that are bound to a different conversation prefix.
- Verify the requested and served model identity from transport metadata, plus a real Pi-owned tool loop, with existing team subscription profiles and no credential disclosure.
- Document the implementation choice and why extending kendex is a better fit than replacing it with pi-pod, doppelclaude, the chem provider, or direct OAuth impersonation.
- Update user documentation and the consumer changelog, then install the validated fork globally and run a disposable fresh Pi startup check.

## Capabilities

### New Capabilities

- `pi-claude-fable-5-1`: Defines Fable 5.1 selection, runtime compatibility, account routing, session rebuild safety, served-model evidence, and Pi tool-loop behavior.

### Modified Capabilities

- None.

## Impact

- Affects `pi-extensions/pi-claude-bridge` model metadata, Claude query assembly, session import/rebuild behavior, executable compatibility checks, tests, bundle output, README, development guidance, and changelog.
- Preserves the current `kendex.pi.claude-account-router.v1` integration and does not add, copy, or expose subscription credentials.
- Adds no new dependency and does not change the bridge provider ID, persisted account identity format, connector policy, HTTP API, or UI.
- Delivery is limited to the user's `juicetin/kendex` fork. No upstream pull request or merge is authorized.
