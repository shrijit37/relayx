#!/usr/bin/env bash
# ts-guard.sh — PostToolUse hook for TS/TSX edits
# Advisory: runs prettier --check + eslint after every Edit/Write to a .ts/.tsx file.
# Logs findings, always exits 0.
set -euo pipefail

# Read tool input from stdin (JSON)
INPUT=$(cat)

# Extract file path — handle both hook JSON (tool_input nested) and raw JSON
FILE_PATH=""
if command -v jq &>/dev/null; then
    FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // .file_path // .filePath // empty' 2>/dev/null || true)
fi
[ -n "$FILE_PATH" ] || exit 0

# Non-TS files: skip
case "$FILE_PATH" in *.ts|*.tsx) ;; *) exit 0 ;; esac
[ -f "$FILE_PATH" ] || exit 0

# Find project root (nearest package.json)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT=""
DIR=$(dirname "$FILE_PATH")
while [ "$DIR" != "/" ] && [ "$DIR" != "$HOME" ]; do
    if [ -f "$DIR/package.json" ]; then
        PROJECT_ROOT="$DIR"
        break
    fi
    DIR=$(dirname "$DIR")
done
[ -n "$PROJECT_ROOT" ] || exit 0

cd "$PROJECT_ROOT"

# Determine package runner
RUNNER="npx"
command -v bunx &>/dev/null && RUNNER="bunx"

# prettier (advisory check — don't auto-reformat)
PRETTIER_OUTPUT=$($RUNNER prettier --check "$FILE_PATH" 2>&1 || true)
if [ -n "$PRETTIER_OUTPUT" ]; then
    echo "[ts-guard] prettier findings in $PROJECT_ROOT:"
    echo "$PRETTIER_OUTPUT" | head -10
fi

# eslint (advisory — log but don't fail the hook)
LINT_OUTPUT=$($RUNNER eslint "$FILE_PATH" 2>&1 || true)
if [ -n "$LINT_OUTPUT" ]; then
    echo "[ts-guard] eslint findings in $PROJECT_ROOT:"
    echo "$LINT_OUTPUT" | head -30
fi

exit 0
