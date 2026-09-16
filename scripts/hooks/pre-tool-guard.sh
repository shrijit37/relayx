#!/usr/bin/env bash
# pre-tool-guard.sh — PreToolUse hook: protect files agents must not edit.
#
# Singleton: one hook instead of three inline `jq` one-liners in each agent's
# settings.json. Blocks (exit 2):
#   - lockfiles:      Cargo.lock, bun.lock, bun.lockb, pnpm-lock.*,
#                     package-lock.json, yarn.lock  (single source of truth)
#   - .env files:     secrets must be edited manually, never by an agent
#   - docs/archive/:  immutable historical snapshots
#
# Field precedence note: Factory Droid sends {tool_input: {file_path}},
# Claude Code sends {tool_input: {file_path}}, OpenCode sends
# {tool_args.filePath} / {files} / {changes}.
set -euo pipefail

INPUT=$(cat)

FILE_PATH=$(echo "$INPUT" | jq -r '
  .tool_input.file_path // .tool_input.filePath //
  .tool_args.filePath // .tool_args.file_path // .tool_args.path //
  .file_path // .filePath // empty' 2>/dev/null || true)

# Multiple-path payloads (OpenCode {files: [...]} / {changes: [...]}): check
# each one queued / written — one match blocks the whole batch.
if [ -z "$FILE_PATH" ]; then
  FILE_PATH=$(echo "$INPUT" | jq -r '
    (.files[]? // empty), (.changes[]?.path // empty)
    | select(. != null and . != "")' 2>/dev/null | while IFS= read -r p; do
    printf '%s\n' "$p"
  done || true)
fi

[ -n "$FILE_PATH" ] || exit 0

blocked=0
found=0

while IFS= read -r f; do
  [ -n "$f" ] || continue
  found=1
  b="${f##*/}"

  case "$b" in
    Cargo.lock|bun.lock|bun.lockb|pnpm-lock.*|package-lock.json|yarn.lock)
      echo "Blocked: lock files are managed by the package manager — do not hand-edit ($f)." >&2
      blocked=1
      ;;
    .env|.env.*)
      case "$b" in
        .env.example|.env.sample|.env.template) ;; # templates are safe
        *) echo "Blocked: '$b' holds secrets and must be edited manually, not by an agent." >&2; blocked=1 ;;
      esac
      ;;
    *)
      case "$f" in
        */docs/archive/*|docs/archive/*)
          echo "Blocked: docs/archive/ holds immutable historical snapshots — write a living doc in docs/ instead." >&2
          blocked=1
          ;;
      esac
      ;;
  esac
done <<< "$FILE_PATH"

[ "$found" -eq 1 ] || exit 0
[ "$blocked" -eq 1 ] && exit 2 || exit 0
