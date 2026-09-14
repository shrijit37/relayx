#!/usr/bin/env bash
# shell-guard.sh — PostToolUse hook for shell script edits
# Runs bash -n syntax check on .sh files. Advisory — logs errors, exits 0.
set -euo pipefail

# Read tool input from stdin (JSON, may be tool_input-wrapped)
INPUT=$(cat)

# Extract file path — handle both hook JSON and raw JSON
FILE_PATH=""
if command -v jq &>/dev/null; then
    FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // .file_path // .filePath // empty' 2>/dev/null || true)
fi
[ -n "$FILE_PATH" ] || exit 0

# Non-shell files: skip
case "$FILE_PATH" in *.sh) ;; *) exit 0 ;; esac
[ -f "$FILE_PATH" ] || exit 0

# Syntax check
CHECK_OUTPUT=$(bash -n "$FILE_PATH" 2>&1 || true)
if [ -n "$CHECK_OUTPUT" ]; then
    echo "[shell-guard] syntax error in $FILE_PATH:"
    echo "$CHECK_OUTPUT"
fi

exit 0
