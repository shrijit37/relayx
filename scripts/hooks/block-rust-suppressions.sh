#!/usr/bin/env bash
# block-rust-suppressions.sh — PreToolUse hook.
# Block #[allow(dead_code)] / #[expect(dead_code)] in Rust source files.
#
# Single source shared via .factory/hooks/ + .claude/hooks/ wrappers.
# Exit 2 blocks the edit; stderr is fed back to the agent.
#
# Payload shapes: reads the pending edit text from every field the wired
# harnesses can send (Write: `tool_input.content`; Edit/MultiEdit:
# `tool_input.new_string` / `tool_input.updates[].new_string`; others:
# `tool_args.content` / top-level `content`). A dead_code suppression
# introduced via Edit must not bypass the block.
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

# shellcheck source=scripts/hooks/rust-policy-patterns.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/rust-policy-patterns.sh"

if echo "$CONTENT" | grep -nE "$RUST_PATTERN_ALLOW_DEAD_CODE"; then
  echo "Forbidden: #[allow(dead_code)] / #![allow(dead_code)] in Rust source" >&2
  echo "Fix the underlying issue instead: remove unused code or add the usage first." >&2
  echo "Narrow #[allow(...)] for FFI/generated code is permitted with a documented reason." >&2
  exit 2
fi

if echo "$CONTENT" | grep -nE "$RUST_PATTERN_EXPECT_DEAD_CODE"; then
  echo "Forbidden: #[expect(dead_code)] / #![expect(dead_code)] in Rust source" >&2
  echo "Fix the underlying issue instead of suppressing the compiler warning." >&2
  exit 2
fi

exit 0
