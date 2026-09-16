#!/usr/bin/env bash
# block-rust-unwrap.sh — PreToolUse hook.
# Block .unwrap() and .expect(...) in Rust source files (non-test only).
#
# Single source shared via .factory/hooks/ + .claude/hooks/ wrappers.
# Exit 2 blocks the edit; stderr is fed back to the agent.
set -euo pipefail

INPUT=$(cat)

FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // .tool_args.filePath // empty' 2>/dev/null || true)
CONTENT=$(echo "$INPUT" | jq -r '.tool_input.content // empty' 2>/dev/null || true)

case "${FILE_PATH:-}" in
  *.rs) ;;
  *) exit 0 ;;
esac

[[ -n "$CONTENT" ]] || exit 0

# Test files are exempt — a panicking test just fails that test.
if echo "$FILE_PATH" | grep -qE '(test|tests|_test\.rs|test_)'; then
  exit 0
fi

if echo "$CONTENT" | grep -nE '\.unwrap\s*\(\s*\)'; then
  echo "Forbidden: .unwrap() in Rust source" >&2
  echo "Replace with ?, .ok_or(...), or explicit match/if-let." >&2
  echo "For tests: use assert! / assert_eq! or #[should_panic]." >&2
  exit 2
fi

if echo "$CONTENT" | grep -nE '\.expect\s*\('; then
  echo "Forbidden: .expect() in Rust source" >&2
  echo "Replace with .context(\"msg\")? or .map_err(|e| Error::...)?" >&2
  exit 2
fi

exit 0
