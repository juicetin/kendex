#!/usr/bin/env bash
# ---
# name: pre-commit-check
# event: PreToolUse
# matcher: Bash
# description: On a git commit, defer to the working directory's armed git pre-commit hook (kendex guard install arms one); where none is armed there, run the kendex guard pre-commit chain — format, lint, and commit guards — from the working directory as the fallback gate. Where one is armed, a command that sidesteps it with git's no-verify flag, -n, or a core.hooksPath override is refused: git would skip the commit-msg hook too, and no fallback here can check the message. Gates the working directory only: a commit aimed at another repository is gated by that repository's own armed hook, and by nothing here.
# safety: Prevents committing unchecked code from the working directory when it has no armed git pre-commit hook, and refuses a command that bypasses an armed hook (no-verify, -n) or injects git configuration that could (any -c, --config-env, or GIT_CONFIG_* word — core.hooksPath and include.path among them) rather than half-checking it; a commit aimed at another repository is that repository's armed hook's to gate.
# timeout: 1800
# ---

set -euo pipefail

INPUT=$(cat)

# Word-order detection, no shell parsing: the authoritative check is the
# repository's own git pre-commit hook, which git runs in the right repo
# whatever the command's quoting, substitutions, or directory hops. This
# lane only decides whether to consult the fallback, so a miss here skips
# feedback, never a check — and `git log --grep=commit` merely pays for a
# guard run it did not need.
#
# The payload is JSON, where a string never spans lines: joining the
# payload first reads a key and value that arrived on separate lines.
JOINED=$(printf '%s' "$INPUT" | tr -d '\n\r')
COMMAND=$(printf '%s' "$JOINED" \
  | grep -oE '"command"[[:space:]]*:[[:space:]]*"(\\.|[^"\\])*"' | head -1) || COMMAND=""
# A payload that names a command this lane cannot read is refused, never
# waved through: where no git hook is armed, this lane is the check.
if [ -z "$COMMAND" ]; then
  printf '%s' "$JOINED" | grep -q '"command"[[:space:]]*:' || exit 0
  echo "pre-commit-check: could not read the command out of the hook payload" >&2
  exit 2
fi
# A quoted multi-word argument is data, not shell: an issue description
# that merely mentions the no-verify flag is not a commit that uses it.
# Dropping those regions before the word scan is what separates the two,
# and every ambiguity resolves the other way — toward keeping the text and
# paying a refusal to reword. Kept, therefore: a quoted region with no
# whitespace (a quoted flag, or half a token split across quotes, since
# `--no-"verify"` is still the flag), a double-quoted region holding a
# `$(` or a backtick (the shell runs that, so this lane reads it), and any
# region handed to an interpreter — the argument after `eval` or after a
# short-flag cluster ending in `c`, as `bash -c` and `sh -lc` spell it. An
# unterminated quote is kept whole.
#
# JSON escapes are decoded first: `\"` is a real quote to the shell below,
# and the whitespace escapes separate words — `cargo fmt\ngit commit` is
# two commands, not one word `ngit`.
#
# Without awk the command cannot be read at all — the same case as a
# payload this lane cannot parse, and refused for the same reason.
if ! command -v awk >/dev/null 2>&1; then
  echo "pre-commit-check: awk is not on PATH, so the command cannot be read — nothing here can judge this commit" >&2
  exit 2
fi
BODY=$(printf '%s' "$COMMAND" | sed 's/^"command"[[:space:]]*:[[:space:]]*"//; s/"$//')
SANITIZED=$(printf '%s\n' "$BODY" | awk '
function keep(body, quote, sofar,   prev) {
  if (body !~ /[ \t]/) return 1
  if (quote == "\"" && (index(body, "$(") > 0 || index(body, "`") > 0)) return 1
  sub(/[ \t]+$/, "", sofar)
  prev = sofar
  sub(/^.*[ \t]/, "", prev)
  return (prev == "eval" || prev ~ /^-[a-zA-Z]*c$/)
}
{
  # 1. JSON unescape. A \uXXXX escape becomes a space: this lane reads
  #    shell syntax, and no harness spells its syntax that way.
  cmd = ""
  n = length($0)
  for (i = 1; i <= n; i++) {
    c = substr($0, i, 1)
    if (c != "\\" || i == n) { cmd = cmd c; continue }
    i++
    e = substr($0, i, 1)
    if (e ~ /^[ntrbf]$/) cmd = cmd " "
    else if (e == "u") { i += 4; cmd = cmd " " }
    else cmd = cmd e
  }
  # 2. Drop the quoted data regions, keeping the shell around them. A
  #    backslash outside quotes escapes one character, which stays: the
  #    shell reads `--no\-verify` as the flag, and so must this lane.
  out = ""
  n = length(cmd)
  i = 1
  while (i <= n) {
    c = substr(cmd, i, 1)
    if (c == "\\" && i < n) { out = out substr(cmd, i + 1, 1); i += 2; continue }
    if (c != "\"" && c != "\047") { out = out c; i++; continue }
    body = ""
    j = i + 1
    while (j <= n) {
      d = substr(cmd, j, 1)
      if (c == "\"" && d == "\\" && j < n) { body = body substr(cmd, j + 1, 1); j += 2; continue }
      if (d == c) break
      body = body d
      j++
    }
    if (j > n) { out = out substr(cmd, i); break }
    out = out (keep(body, c, out) ? body : " ")
    i = j + 1
  }
  print out
}')
WORDS=" $(printf '%s' "$SANITIZED" | tr -c 'a-zA-Z0-9_=-' ' ') "
printf '%s' "$WORDS" | grep -qE ' git( .*)? commit ' || exit 0

# Repository-moving words (-C, --git-dir, --work-tree, cd, a GIT_DIR or
# GIT_WORK_TREE assignment) mean the commit may land elsewhere. This lane never follows them — git does, where the
# target has an armed hook — so where it cannot defer it says which
# directory it judged, and that the target's own hook is the target's gate.
MOVES=""
printf '%s' "$WORDS" | grep -qE ' (cd|-C|--git-dir[^ ]*|--work-tree[^ ]*|GIT_DIR[^ ]*|GIT_WORK_TREE[^ ]*) ' && MOVES=1
elsewhere_notice() {
  [ -z "$MOVES" ] && return 0
  echo "pre-commit-check: the command moves repositories (-C, --git-dir, --work-tree, cd, GIT_DIR, or GIT_WORK_TREE); this hook judged $PWD only — the target repository is gated by its own armed git pre-commit hook, if any (kendex guard install there)" >&2
}

# An armed hook means git itself will gate the commit; running the chain
# here too would validate everything twice. A command that sidesteps it
# is refused, not covered: git's no-verify flag — spelled out or cut to
# any unique prefix, as git allows, or `-n` alone or inside a short-flag
# cluster — tells git to skip the commit-msg hook as well, and the
# message is not knowable here, so no fallback could stand in; and any
# configuration injected on the command line — a `-c` word, a
# `--config-env` word, a `GIT_CONFIG_*` assignment — can point git at
# hooks this lane did not inspect, by `core.hooksPath` directly or by an
# `include.path` that loads it, so the whole class is refused rather
# than one spelling of it — and so is a `git config` write of
# core.hooksPath in the same command, which disarms the hook before the
# commit reaches it (the key is matched in any case, whatever options
# stand between `config` and it). One of
# those words from some other command on the line costs a refusal to
# reword, never an unchecked commit.
HOOKS_DIR=$(git rev-parse --git-path hooks 2>/dev/null) || {
  elsewhere_notice
  exit 0
}
if [ -x "$HOOKS_DIR/pre-commit" ]; then
  BYPASS=$(printf '%s' "$WORDS" | grep -oE ' (--no-veri[a-z]*|-[a-zA-Z]*n[a-zA-Z]*|-c|--config-env[^ ]*|GIT_CONFIG_[^ ]*) ' | head -1) \
    || BYPASS=$(printf '%s' "$WORDS" | grep -oiE ' config .* hookspath ' | head -1) \
    || exit 0
  echo "pre-commit-check: '$(printf '%s' "$BYPASS" | sed 's/^ *//; s/ *$//')' bypasses this repository's armed git hooks or injects configuration that could, and the commit-msg gate cannot be checked from here — commit without bypassing hooks or passing git configuration; git runs the installed pre-commit and commit-msg hooks itself" >&2
  exit 2
fi
elsewhere_notice

if ! command -v kendex >/dev/null 2>&1; then
  echo "pre-commit-check: no git pre-commit hook will run for this commit and the kendex binary is not on PATH, so nothing can check it — install kendex, or remove this hook" >&2
  exit 2
fi
# The frontmatter timeout budgets a cold clippy build on top of the other
# lanes; the harness cancelling this hook at that budget is the one way
# left past the gate, so the budget stays above what the chain can take.
CHAIN=$(kendex guard run pre-commit 2>&1) || {
  printf '%s\n' "$CHAIN" >&2
  echo "pre-commit-check: commit blocked by the failures above (no git pre-commit hook runs for this commit; kendex guard install arms one, which git runs unless the command bypasses it)" >&2
  exit 2
}
# A lane that skipped itself said so for a reason; a clean verdict must
# not swallow it. grep's 1 is "nothing skipped"; anything else is grep
# itself failing, which no clean verdict may paper over.
status=0
printf '%s\n' "$CHAIN" | grep -F 'skipped' >&2 || status=$?
[ "$status" -le 1 ] || {
  echo "pre-commit-check: could not scan the chain report for skipped lanes (grep exited $status)" >&2
  exit 2
}
exit 0
