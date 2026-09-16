#!/usr/bin/env bash
# check-rust-gates.sh — Stop hook: remind about the Rust quality gates when
# the working tree still holds changed Rust files.
#
# Advisory by design: it never blocks the stop, and always exits 0.
#
# This is a real Stop hook (not a hookify `event: stop` rule), because
# hookify resolves rules relative to the session CWD and returns ALL rules
# for non-Bash/Edit/Write tools — a hookify Stop rule would fire on every
# Read/Grep/Task call. The Droid/Claude Stop event fires exactly once.
#
# SINGLE SOURCE OF TRUTH shared by .claude/hooks/check-rust-gates.sh and
# .factory/hooks/ (require-clippy-before-stop.sh is the heavy, blocking
# variant: clippy+fmt+test before stopping).
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
