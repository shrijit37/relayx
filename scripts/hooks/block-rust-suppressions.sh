#!/usr/bin/env bash
# block-rust-suppressions.sh — PreToolUse hook.
# Block #[allow(dead_code)] / #[expect(dead_code)] in Rust source files.
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

if echo "$CONTENT" | grep -nE '#!?\[allow\s*\([^]]*dead_code'; then
  echo "Forbidden: #[allow(dead_code)] / #![allow(dead_code)] in Rust source" >&2
  echo "Fix the underlying issue instead: remove unused code or add the usage first." >&2
  echo "Narrow #[allow(...)] for FFI/generated code is permitted with a documented reason." >&2
  exit 2
fi

if echo "$CONTENT" | grep -nE '#!?\[expect\s*\([^]]*dead_code'; then
  echo "Forbidden: #[expect(dead_code)] / #![expect(dead_code)] in Rust source" >&2
  echo "Fix the underlying issue instead of suppressing the compiler warning." >&2
  exit 2
fi

exit 0
