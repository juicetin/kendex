# kendex

Desktop app + thin CLI (Rust + Tauri + React) for managing AI coding-harness customizations.

**Orientation.** Read `docs/ARCHITECTURE.md` before structural work; stale docs are bugs — amend them in the same change. Open work lives in Linear (team KEN); scratch goes to `tmp/` (gitignored), never `/tmp`. Review bots follow `review-bots.md` and `.github/instructions/*.instructions.md`. Engineering rules are the code-quality skill; round scope is the dev skill's § Engineering Rules; finding dispositions are orch's `references/finding-disposition.md`.

Repo-specific rules:

- `crates/core` is pure domain logic — no Tauri, no IPC, no UI concerns.
- `ui/` renders state and invokes commands; domain logic and types live in Rust, and TS bindings are generated, never hand-written.
- Every CI job runs on GitHub-hosted runners; no workflow reads `vars.CI_RUNNER_*`.
- In a worktree, sync a skill's `.agents/` render by replaying the source diff, never by copying `SKILL.md`; renders may carry an injected instructions block.
- A new test that needs a host path reads it from `Env` (`host_rooted`, `drift_dir`), never composes the platform path.
- A new test that shells out to git clears `GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`, and `GIT_INDEX_FILE` together. Under `skills/orch/tests/` that clearing lives in `lib/git-env.sh`, which every suite sources on the line under its `set -...o pipefail`.
- The CHANGELOG is for consumers (Keep a Changelog): document app, CLI, and package changes; keep engine-internal and maintainer-only details out. An entry runs at most 200 characters — the outcome, a **Breaking:** migration inline, `— thanks @name` for outside contributors — never an essay.
- An entry is a file, never a `CHANGELOG.md` line: write `changelog.d/<section>/<name>.md` holding the list item it becomes, per `changelog.d/README.md`. `.agents/skills/growth-guards/scripts/changelog-entries --collate` folds them in at release.
- A change to a source with a tracked render (`skills/`, `agents/<n>.md`, `hooks/<n>`) lands the render in the same commit, judged per file; `tools/guard` checks presence, not byte equality, for the injected block named above. A source with no tracked render has nothing to land.
- `ui/` installs with `npm ci --prefix ui`, in the main checkout only.
- Every suite and the aggregator over them run on the pull request as well as in the merge queue, so a green PR proves what the queue re-proves and a red shard blocks the PR itself — fix it there rather than requeuing. Anything in `.github/workflows/skill-tests.yml` that does not run on every event carries an `if:` saying why.

`tools/guard` enforces the rest; read the script, it is the list. It is the last lane of the package's commit chain, named by `GROWTH_GUARDS_PRE_COMMIT_LOCAL`, and `tools/setup` arms that chain in a fresh clone beside the package's own commit-msg gate. A stray `.git/hooks/commit-msg` calling a repo-local lane this repo lacks blocks every commit; delete it and run `tools/setup` again. The commit-msg gate holds the three rules only a commit message can carry: the header is `type(scope)!: subject`, the whole header line caps at 72 characters, and a change under `crates/` or `ui/` ships a changelog fragment or says `[no-changelog]` in the subject.

## Code Review Rules

For automated reviewers (Codex code review, Copilot). Working agents: your
reply contract is in the orch skill, not here.

- Raise only defects in the changed lines or directly broken by them:
  correctness, security, data loss, fail-open in gate/guard/CI code.
- One comment per root cause, naming every affected site. Everything you
  have about the diff goes in one round.
- No style, wording, or naming preferences. No speculative hardening on
  fail-closed paths. No test-coverage asks unless the diff changes behavior
  no test exercises. Formatting and lint belong to CI, not review.
- Do not re-raise a finding class already answered with a documented
  rationale — `Declined: <reason>` on this PR, a settings comment, an engine
  header comment, or a note in `skills/review-gate/references/` — unless the
  relevant code changed since.
- Author replies are `Fixed in <sha>`, `Declined: <reason>`, or
  `Tracked: KEN-<n>` / `#<n>`. A decline takes a reason form
  `skills/orch/references/finding-disposition.md` § Decision flow sets
  out; a label is not a reason. The merge gate rejects tracking claims
  that name no issue, and declines whose reason is nothing but a label
  it knows.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:970c3bf2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

<!-- BEGIN BEADS CODEX SETUP: generated by bd setup codex -->
## Beads Issue Tracker

Use Beads (`bd`) for durable task tracking in repositories that include it. Use the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or `~/.agents/skills/beads/SKILL.md` (global install) for Beads workflow guidance, then use the `bd` CLI for issue operations.

### Quick Reference

```bash
bd ready                # Find available work
bd show <id>            # View issue details
bd update <id> --claim  # Claim work
bd close <id>           # Complete work
bd prime                # Refresh Beads context
```

### Rules

- Use `bd` for all task tracking; do not create markdown TODO lists.
- Run `bd prime` when Beads context is missing or stale. Codex 0.129.0+ can load Beads context automatically through native hooks; use `/hooks` to inspect or toggle them.
- Keep persistent project memory in Beads via `bd remember`; do not create ad hoc memory files.

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.
<!-- END BEADS CODEX SETUP -->
