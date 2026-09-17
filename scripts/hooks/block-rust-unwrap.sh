#!/usr/bin/env bash
# block-rust-unwrap.sh — PreToolUse hook.
# Block .unwrap() and .expect(...) in Rust source files (non-test only).
#
# Single source shared via .factory/hooks/ + .claude/hooks/ wrappers.
# Exit 2 blocks the edit; stderr is fed back to the agent.
#
# Payload shapes: reads the pending edit text from every field the wired
# harnesses can send (Write: `tool_input.content`; Edit/MultiEdit:
# `tool_input.new_string` / `tool_input.updates[].new_string`; others:
# `tool_args.content` / top-level `content`), so a panic-prone shortcut
# introduced via Edit cannot bypass this enforcement layer.
set -euo pipefail

INPUT=$(cat)

FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // .tool_args.filePath // .file_path // empty' 2>/dev/null || true)
CONTENT=$(echo "$INPUT" | jq -r '
  [ .tool_input.content,
    .tool_input.new_string,
    (.tool_input.updates[]?.new_string // empty),
    .tool_args.content,
    .content
  ] | map(select(. != null)) | join("\n")' 2>/dev/null || true)

case "${FILE_PATH:-}" in
  *.rs) ;;
  *) exit 0 ;;
esac

[[ -n "$CONTENT" ]] || exit 0

# Test files are exempt — a panicking test just fails that test. Match the
# BASENAME with explicit suffixes only: a path is a test file iff its file
# name ends in _test.rs or the path sits under a tests/ directory. Free
# substring matching over the whole path is a footgun (contest.rs,
# attest.rs, or crates/test-harness/src/*.rs would wrongly be exempt).
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

if is_test_file "$FILE_PATH"; then
  exit 0
fi

# shellcheck source=scripts/hooks/rust-policy-patterns.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/rust-policy-patterns.sh"

if echo "$CONTENT" | grep -nE "$RUST_PATTERN_UNWRAP"; then
  echo "Forbidden: .unwrap() in Rust source" >&2
  echo "Replace with ?, .ok_or(...), or explicit match/if-let." >&2
  echo "For tests: use assert! / assert_eq! or #[should_panic]." >&2
  exit 2
fi

if echo "$CONTENT" | grep -nE "$RUST_PATTERN_EXPECT"; then
  echo "Forbidden: .expect() in Rust source" >&2
  echo "Replace with .context(\"msg\")? or .map_err(|e| Error::...)?" >&2
  exit 2
fi

exit 0
