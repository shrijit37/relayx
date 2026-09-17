#!/usr/bin/env bash
# require-clippy-before-stop.sh — Stop hook (Factory Droid).
# Require cargo clippy, cargo fmt, and cargo test before stopping on Rust files.
#
# Hardening vs the legacy version: uses `git status --porcelain` instead of
# `git diff` (git diff misses untracked new .rs files — a brand-new file could
# otherwise slip through with zero checks), runs under `timeout` so a hung
# cargo never wedges the stop, and scopes clippy to `--all-features` to match
# the repo's canonical command.
#
# Timeout choice: a full workspace suite legitimately exceeds 10 minutes on a
# cold build (the first `cargo test` after many crates change). 1800s is
# session-scale: it only fires on a genuinely hung cargo, not a slow one.
# `timeout` is GNU coreutils and absent on stock macOS — fall back to a plain
# run there (no bound is better than a hard false-positive block).
#
# Output discipline: gate output is streamed to stderr and fed back to the
# agent, which can flood its context (clippy/test produce thousands of lines).
# Only print the tail of the log on failure.
#
# Exit 2 blocks stopping and feeds stderr back to Droid.
set -euo pipefail

INPUT=$(cat)

# Any Rust files modified in this session? (unstaged, staged, or untracked)
changed="$({
  git diff --name-only 2>/dev/null || true
  git diff --cached --name-only 2>/dev/null || true
  git ls-files --others --exclude-standard 2>/dev/null || true
} | sort -u | grep -E '\.rs$' || true)"
[[ -n "$changed" ]] || exit 0

echo "Rust files were modified. Running quality gates before stopping..." >&2
echo "" >&2

root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$root"

FAILED=0

run_gate() {
  local label="$1"; shift
  echo "Running: $*" >&2
  local log
  log="$(mktemp)"
  if command -v timeout >/dev/null 2>&1; then
    if ! timeout 1800 "$@" >"$log" 2>&1; then
      echo "$label failed (tail):" >&2
      tail -n 40 "$log" >&2
      FAILED=1
    fi
  else
    if ! "$@" >"$log" 2>&1; then
      echo "$label failed (tail):" >&2
      tail -n 40 "$log" >&2
      FAILED=1
    fi
  fi
  rm -f "$log"
}

run_gate "cargo clippy" cargo clippy --all-features --all-targets --workspace -- -D warnings
echo "" >&2
run_gate "cargo fmt" cargo fmt --check
echo "" >&2
run_gate "cargo test" cargo test --all-features --workspace

if [[ "$FAILED" -ne 0 ]]; then
  echo "" >&2
  echo "Rust quality gates failed. Fix the issues before reporting done." >&2
  exit 2
fi

echo "" >&2
echo "All Rust quality gates passed." >&2
exit 0
