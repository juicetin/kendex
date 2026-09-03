## Purpose

Expose Claude Fable 5.1 through the Pi Claude bridge without weakening model identity, subscription routing, session continuity, or Pi-owned tool execution.

## ADDED Requirements

### Requirement: Fable 5.1 is a distinct selectable model
The bridge SHALL register `pi-claude/claude-fable-5-1` before other Fable models with a 1,000,000-token context window and 128,000-token output limit. It SHALL keep `pi-claude/claude-fable-5` available and resolve each exact model ID without prefix ambiguity.

#### Scenario: Model catalog lacks Fable 5.1
- **WHEN** Pi's Anthropic catalog has no Fable 5.1 entry
- **THEN** the bridge supplies its own Fable 5.1 metadata and lists it before Fable 5

#### Scenario: Both Fable versions are requested
- **WHEN** a caller selects either exact Pi model ID
- **THEN** the bridge routes that version and does not resolve the other version by partial match

### Requirement: Fable 5.1 uses a compatible Claude Code runtime
The bridge SHALL request Fable 5.1 as `claude-fable-5-1[1m]`. Before changing persisted session state or starting the request, it SHALL select or verify a Claude Code executable at version 2.1.255 or newer. It SHALL fail with the required version and detected candidates when no compatible executable is available.

#### Scenario: Compatible configured executable
- **WHEN** the configured Claude executable reports version 2.1.255 or newer
- **THEN** the bridge uses that executable and sends `claude-fable-5-1[1m]`

#### Scenario: Bundled runtime is too old but PATH is compatible
- **WHEN** no executable is configured, the bundled runtime is older than 2.1.255, and an allowed PATH executable meets the minimum
- **THEN** the bridge uses the compatible PATH executable for Fable 5.1

#### Scenario: No compatible runtime exists
- **WHEN** every eligible Claude executable is missing, unreadable, has an unparseable version, or is older than 2.1.255
- **THEN** the request fails before session rebuild or query start with an actionable compatibility error and no silent smaller-context fallback

### Requirement: Fable 5.1 preserves adaptive thinking constraints
The bridge SHALL let Claude Code manage Fable 5.1 adaptive thinking and SHALL NOT send a forced tool choice that Fable 5.1 does not support.

#### Scenario: Caller selects a Pi thinking level
- **WHEN** Pi sends a supported thinking or effort level with a Fable 5.1 request
- **THEN** the bridge preserves the effort intent without replacing adaptive thinking with a fixed thinking budget

### Requirement: Existing account routing and fallback policy remains intact
Fable 5.1 SHALL use the existing versioned account-router contract, account-scoped sessions, exclusion set, rotation limit, and post-exhaustion model policy. Adding Fable 5.1 SHALL NOT change those behaviors for Fable 5 or other models.

#### Scenario: A managed profile is available
- **WHEN** the account router acquires a profile for Fable 5.1
- **THEN** the bridge sends the exact Fable 5.1 model ID to the router and starts Claude Code in that profile's credential and session scope

#### Scenario: A profile fails before visible output
- **WHEN** the failure is eligible for existing account rotation
- **THEN** the bridge retries under the next eligible profile without leaking duplicate stream events or mixing account-scoped sessions

#### Scenario: Fable allowance is exhausted
- **WHEN** the account router applies its existing post-exhaustion model decision
- **THEN** the bridge uses that routed model and records the actual model in assistant metadata rather than claiming Fable 5.1 served the turn

### Requirement: Fable 5.1 rebuilds omit stale thinking blocks
When rebuilding a Fable 5.1 Claude session from Pi history, the bridge SHALL omit prior signed thinking blocks because their signatures bind to the previous conversation prefix. It SHALL preserve valid text, tool calls, and paired tool results. Reuse without rebuild and rebuilds for other models SHALL retain their current behavior.

#### Scenario: Fable 5.1 session rebuild
- **WHEN** compaction, tree navigation, abort recovery, account rotation, or history divergence forces a Fable 5.1 rebuild
- **THEN** the imported history contains no prior thinking blocks and retains the remaining valid conversation content

#### Scenario: Existing model rebuild
- **WHEN** the bridge rebuilds a session for a model other than Fable 5.1
- **THEN** the existing model-specific import behavior is unchanged

### Requirement: Real validation proves served identity and Pi tool ownership
Acceptance evidence SHALL include a real Fable 5.1 response through Pi, the served model from transport-derived assistant metadata, the selected existing subscription profile in redacted form, and a completed tool request, Pi execution result, and final assistant response. Model self-identification and the requested model label alone SHALL NOT count as served-model evidence.

#### Scenario: Real Fable 5.1 tool loop succeeds
- **WHEN** a permitted existing Team subscription serves a Pi request that requires reading a known fixture
- **THEN** the evidence shows `responseModel` or equivalent transport metadata for Fable 5.1, a Pi tool execution pair, the expected fixture value, and no credential values

#### Scenario: Existing subscription lacks access
- **WHEN** no already-authorized profile can serve Fable 5.1 within its current plan or credits
- **THEN** validation stops and reports unavailable access without enabling credits, purchasing usage, or substituting generated self-identification as proof
