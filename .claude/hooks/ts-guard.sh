#!/usr/bin/env bash
# ts-guard.sh — PostToolUse hook for TS/TSX edits
# Runs prettier + eslint after every Edit/Write to a .ts/.tsx file.
# Advisory like rust-guard.sh — logs findings, always exits 0.
set -euo pipefail

# Read tool input from stdin (JSON)
INPUT=$(cat)

# Extract file path
FILE_PATH=""
if command -v jq &>/dev/null; then
    FILE_PATH=$(echo "$INPUT" | jq -r '.file_path // .filePath // empty' 2>/dev/null || true)
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

# prettier (silent, best-effort)
npx prettier --write "$FILE_PATH" 2>/dev/null || true

# eslint (advisory — log but don't fail the hook)
LINT_OUTPUT=$(npx eslint "$FILE_PATH" 2>&1 || true)
if [ -n "$LINT_OUTPUT" ]; then
    echo "[ts-guard] eslint findings in $PROJECT_ROOT:"
    echo "$LINT_OUTPUT" | head -30
fi

exit 0
