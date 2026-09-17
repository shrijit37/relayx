#!/usr/bin/env bash
# check-rust-policy.sh — enforce the Rust Engineering Policy on .rs files.
#
# Usage:
#   check-rust-policy.sh            # working tree + index + untracked
#   check-rust-policy.sh --all      # every tracked and untracked .rs file
#   check-rust-policy.sh file.rs…   # explicit paths
#
# Hook mode: when stdin carries hook JSON ({tool_input: {file_path|content}}),
# checks the single edited file. Exit 2 blocks the edit / fails the step.
#
# This is the SINGLE SOURCE OF TRUTH shared by:
#   - .claude/hooks/check-rust-policy.sh   (Claude Code PostToolUse + Stop)
#   - .factory/hooks/check-rust-policy.sh  (Factory Droid PostToolUse)
#   - .githooks/pre-push, .githooks/pre-commit, CI
# Do not fork per-agent copies; extend this file and re-point wrappers.
#
# Policy (see AGENTS.md / CLAUDE.md — non-negotiable):
#   Dead code:  #[allow(dead_code)] / #[expect(dead_code)] — hard ban.
#   Escapes:    todo!() / unimplemented!() — hard ban.
#   Panics:     .unwrap() / .expect(...) — banned outside test files.
#   Narrow #[allow(...)] for FFI/generated/platform code is permitted, except
#   dead_code. No alternate escape hatches. Never weaken lint config instead.
set -euo pipefail

# Forbidden-pattern regexes come from the single shared source used by the
# PreToolUse blockers too — a policy change lives in exactly one place.
# shellcheck source=scripts/hooks/rust-policy-patterns.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/rust-policy-patterns.sh"

usage() {
  echo "Usage: $0 [--all] [file.rs ...]" >&2
}

list_changed_rust_files() {
  {
    git diff --name-only -z
    git diff --cached --name-only -z
    git ls-files --others --exclude-standard -z
  } | sort -z -u | grep -z -E '\.rs$' || true
}

list_all_rust_files() {
  {
    git ls-files -z -- '*.rs'
    git ls-files --others --exclude-standard -z | grep -z -E '\.rs$' || true
  } | sort -z -u
}

# ── Hook mode ────────────────────────────────────────────────────────────
# Reads stdin once; a blocking hook must never leave an unread pipe behind.
INPUT=""
if [[ ! -t 0 ]]; then
  INPUT=$(cat)
fi

files=""
if [[ -n "$INPUT" ]]; then
  # Non-tty stdin. CLI mode is only valid when stdin is a terminal (or a
  # pipe feeding explicit file args). In every ambiguous case — unreadable
  # payload, missing jq — fail OPEN (exit 0) instead of falling through to
  # a whole-tree scan that could fail an unrelated edit/stop on a
  # pre-existing violation anywhere in the working tree.
  if ! command -v jq >/dev/null 2>&1; then
    printf 'check-rust-policy: jq not found — cannot parse hook payload, skipping (fail-open)\n' >&2
    exit 0
  fi
  if echo "$INPUT" | jq -e '.tool_input' >/dev/null 2>&1; then
    # Claude Code / Factory Droid / OpenCode hook payload.
    FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // .tool_args.filePath // empty' 2>/dev/null || true)
    [[ -n "$FILE_PATH" ]] || exit 0
    case "$FILE_PATH" in *.rs) ;; *) exit 0 ;; esac
    [[ -f "$FILE_PATH" ]] || exit 0
    files="$FILE_PATH"
  else
    # Neither a hook payload nor explicit file args on stdin — ambiguous.
    # Do not silently scan the whole tree on garbage stdin.
    exit 0
  fi
fi

# ── CLI mode (pre-commit, pre-push, CI, manual) ─────────────────────────
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ -z "$files" ]]; then
  if [[ "${1:-}" == "--all" ]]; then
    mapfile -d '' -t rust_files < <(list_all_rust_files)
  elif [[ $# -gt 0 ]]; then
    rust_files=("$@")
  else
    mapfile -d '' -t rust_files < <(list_changed_rust_files)
  fi
else
  mapfile -d '' -t rust_files <<< "$files"
fi

if [[ ${#rust_files[@]} -eq 0 ]]; then
  exit 0
fi

# Test files are exempt from the .unwrap() / .expect() checks: a panicking
# test only fails that test — no production risk. Match the BASENAME with
# explicit suffixes only, never free substring matching over the whole path
# (contest.rs, attest.rs, or crates/test-harness/src/*.rs are NOT tests).
is_test_file() {
  local base
  base="$(basename "$1")"
  case "$base" in
    *_test.rs) return 0 ;;
  esac
  case "$1" in
    */tests/*) return 0 ;;
  esac
  return 1
}

failed=0

check_pattern() {
  local description="$1"
  local pattern="$2"
  local test_exempt="${3:-false}"
  local file

  for file in "${rust_files[@]}"; do
    [[ -f "$file" ]] || continue

    if [[ "$test_exempt" == "true" ]] && is_test_file "$file"; then
      continue
    fi

    if grep -nE -- "$pattern" "$file"; then
      echo >&2
      echo "ERROR: Forbidden Rust pattern detected: $description" >&2
      echo "File: $file" >&2
      failed=1
    fi
  done
}

# ─────────────────────────────────────────────────────────────────────────
# DEAD CODE — hard ban, including whitespace-disguised forms and mixed lint
# lists such as #[allow(unused, dead_code)].
# ─────────────────────────────────────────────────────────────────────────
check_pattern \
  "#[allow(dead_code)] / #![allow(dead_code)]" \
  "$RUST_PATTERN_ALLOW_DEAD_CODE"

check_pattern \
  "#[expect(dead_code)] / #![expect(dead_code)]" \
  "$RUST_PATTERN_EXPECT_DEAD_CODE"

# ─────────────────────────────────────────────────────────────────────────
# TEMPORARY / ESCAPE-HATCH MACROS
# ─────────────────────────────────────────────────────────────────────────
check_pattern \
  "todo!()" \
  "$RUST_PATTERN_TODO"

check_pattern \
  "unimplemented!()" \
  "$RUST_PATTERN_UNIMPLEMENTED"

# ─────────────────────────────────────────────────────────────────────────
# PANIC-PRONE SHORTCUTS
# .unwrap_or / .unwrap_or_else / .unwrap_err are allowed. Test files exempt.
# ─────────────────────────────────────────────────────────────────────────
check_pattern \
  ".unwrap()" \
  "$RUST_PATTERN_UNWRAP" \
  true

check_pattern \
  ".expect(...)" \
  "$RUST_PATTERN_EXPECT" \
  true

# ─────────────────────────────────────────────────────────────────────────
if [[ "$failed" -ne 0 ]]; then
  echo >&2
  echo "Rust policy violation." >&2
  echo "Fix the underlying issue instead of suppressing the compiler or using escape hatches." >&2
  echo "Narrow #[allow(...)] is permitted except dead_code; ask before any exception." >&2
  exit 2
fi

exit 0
