#!/usr/bin/env bash
# block-rust-panics.sh — PreToolUse hook.
# Block todo!() and unimplemented!() in Rust source files.
#
# Single source shared via .factory/hooks/ + .claude/hooks/ wrappers.
# Exit 2 blocks the edit; stderr is fed back to the agent.
#
# Payload shapes: the wired harnesses send the pending edit text in
# different fields — whole-file Write uses `tool_input.content`, Claude
# Code Edit/MultiEdit use `tool_input.new_string` /
# `tool_input.updates[].new_string`, and other harnesses may use
# `tool_args.content` / a top-level `content`. All of them are checked;
# an edit that introduces a banned macro must never bypass the block by
# landing in a field we do not read.
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

if echo "$CONTENT" | grep -nE "$RUST_PATTERN_TODO"; then
  echo "Forbidden: todo!() macro in Rust source" >&2
  echo "Replace with actual implementation, or return Result::Err if blocked on design." >&2
  echo "Never ship placeholder panics in data-plane code." >&2
  exit 2
fi

if echo "$CONTENT" | grep -nE "$RUST_PATTERN_UNIMPLEMENTED"; then
  echo "Forbidden: unimplemented!() macro in Rust source" >&2
  echo "Replace with actual implementation, or return Result::Err if blocked on design." >&2
  echo "Never ship placeholder panics in data-plane code." >&2
  exit 2
fi

exit 0
