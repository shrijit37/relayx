#!/usr/bin/env bash
# shell-guard.sh — PostToolUse hook for shell script edits.
#
# Runs `bash -n` syntax checks on edited shell files. Covers BOTH `.sh` files
# and the extension-less git hooks under `.githooks/` (which are bash scripts
# named pre-commit / pre-push / commit-msg and would otherwise slip through a
# *.sh matcher).
#
# Advisory by design: logs errors and exits 0. A broken shell file surfaces in
# the transcript, but does not block the agent (blocking on `bash -n` here
# would also block while a hook is mid-edit).
#
# SINGLE SOURCE OF TRUTH shared by .claude/hooks/shell-guard.sh and
# .factory/hooks/shell-guard.sh.
set -euo pipefail

INPUT=$(cat)

extract_paths() {
  printf '%s' "$INPUT" | jq -r '
    [.tool_input.file_path, .tool_input.filePath,
     .tool_args.filePath, .tool_args.file_path, .tool_args.path,
     (.files[]? // empty),
     (.changes[]?.path // empty)]
    | map(select(. != null and . != "")) | unique | .[]
  ' 2>/dev/null || true
}

paths="$(extract_paths)"
[ -n "$paths" ] || exit 0

while IFS= read -r FILE_PATH; do
  [ -n "$FILE_PATH" ] || continue

  # Shell files: *.sh plus the extension-less .githooks/* executables.
  case "$FILE_PATH" in
    *.sh) ;;
    *.githooks/*) ;;
    *) continue ;;
  esac
  [ -f "$FILE_PATH" ] || continue

  # Symlinked wrappers (scripts/hooks/* -> .githooks/*) — bash -n follows
  # symlinks fine; just make sure the target parses.
  CHECK_OUTPUT="$(bash -n "$FILE_PATH" 2>&1 || true)"
  if [ -n "$CHECK_OUTPUT" ]; then
    echo "[shell-guard] syntax error in $FILE_PATH:"
    echo "$CHECK_OUTPUT"
  fi
done <<< "$paths"

exit 0
