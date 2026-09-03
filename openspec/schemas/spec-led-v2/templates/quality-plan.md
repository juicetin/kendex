# Quality plan

## Baseline commands

<!-- List each exact repository command. Missing commands fail; do not substitute another check. -->

- `<command>`

A final fresh-context code review is required after deterministic checks pass. Fix blocking findings and rerun affected commands.

## Impact-based checks

| Check | Applies | Reason | Command or skill | Evidence |
| --- | --- | --- | --- | --- |
| Dependency review | no | `<specific reason>` | `<command or skill, or n/a>` | `<expected reference>` |
| Test-first implementation | no | `<specific reason>` | `<command or waiver reference, or n/a>` | `<expected reference>` |
| Auth review | no | `<specific reason>` | `<skill or n/a>` | `<expected reference>` |
| Security scan | no | `<specific reason>` | `<command or skill, or n/a>` | `<expected reference>` |
| HTTP API contract | no | `<specific reason>` | `<command or skill, or n/a>` | `<expected reference>` |
| UI visual and accessibility evidence | no | `<specific reason>` | `<command or skill, or n/a>` | `<expected reference>` |
| Code simplification | no | `<specific reason>` | `<skill or n/a>` | `<expected reference>` |
| Fallow | no | `<specific reason>` | `<command or n/a>` | `<expected reference>` |

## Mutation testing

- Human approved: no
- Approval reference: n/a
- Not applicable reason: `<specific reason>`
- Scope: n/a
- Runtime budget: n/a
- Fast test command: n/a
- Evidence threshold: n/a

<!-- If a human opts in, replace every n/a value with the approved bounded value and record the approval reference. -->

## Implementation tasks

<!-- Add every selected command and review to tasks.md as a verifiable checkbox. Record evidence with the matching task; do not create gate state. -->
