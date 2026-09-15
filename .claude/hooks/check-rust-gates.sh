#!/usr/bin/env bash
# check-rust-gates.sh — Stop hook: remind about the Rust quality gates when the
# working tree still holds changed Rust files.
#
# Advisory by design: it never blocks the stop, and always exits 0.
#
# Why this is a real Stop hook rather than a hookify `event: stop` rule: hookify
# resolves its rules relative to the session CWD and returns *all* rules when the
# tool is not Bash/Edit/Write/MultiEdit, so a hookify Stop rule is evaluated on
# every Read/Grep/Task call too. A predicate like "the transcript does not
# mention `cargo clippy`" would therefore fire everywhere — reading the whole
# transcript on each tool call — and with `action: block` it could deny unrelated
# tool calls. Claude Code's own Stop event fires exactly once, on stop.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Drain the hook payload so we never leave an unread pipe behind.
if [[ ! -t 0 ]]; then
  cat >/dev/null || true
fi

command -v git >/dev/null 2>&1 || exit 0
[[ -d "$ROOT/.git" ]] || exit 0

cd "$ROOT"

# Changed Rust files: unstaged, staged, and untracked.
changed="$(
  {
    git diff --name-only || true
    git diff --cached --name-only || true
    git ls-files --others --exclude-standard || true
  } | sort -u | grep -E '\.rs$' || true
)"

[[ -n "$changed" ]] || exit 0

MESSAGE="$(
  {
    printf '%s\n' "Rust files are modified in this working tree:"
    printf '%s\n' "$changed" | awk 'NR<=10 { printf "   - %s\n", $0 }'
    printf '%s\n' ""
    printf '%s\n' "The Rust Engineering Policy requires all of these to pass before done:"
    printf '%s\n' "  cargo fmt --check"
    printf '%s\n' "  cargo clippy --all-targets --all-features --workspace -- -D warnings"
    printf '%s\n' "  cargo test --all-features --workspace"
  }
)"

if command -v jq >/dev/null 2>&1; then
  jq -n --arg m "$MESSAGE" '{systemMessage: $m}'
else
  printf '%s\n' "$MESSAGE" >&2
fi

exit 0
