#!/usr/bin/env bash
# block-rust-panics.sh — PreToolUse hook.
# Block todo!() and unimplemented!() in Rust source files.
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

if echo "$CONTENT" | grep -nE '\btodo!\s*\('; then
  echo "Forbidden: todo!() macro in Rust source" >&2
  echo "Replace with actual implementation, or return Result::Err if blocked on design." >&2
  echo "Never ship placeholder panics in data-plane code." >&2
  exit 2
fi

if echo "$CONTENT" | grep -nE '\bunimplemented!\s*\('; then
  echo "Forbidden: unimplemented!() macro in Rust source" >&2
  echo "Replace with actual implementation, or return Result::Err if blocked on design." >&2
  echo "Never ship placeholder panics in data-plane code." >&2
  exit 2
fi

exit 0
