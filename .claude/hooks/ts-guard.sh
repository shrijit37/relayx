#!/usr/bin/env bash
# ts-guard.sh — PostToolUse hook for TS/TSX edits
# Enforced: rejects edits introducing explicit `any` types.
# Advisory: prettier formatting check.
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

# Resolve to absolute path (hook CWD may not match file location)
if [[ "$FILE_PATH" != /* ]]; then
    FILE_PATH="$(pwd)/$FILE_PATH"
fi
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

# Resolve absolute path for eslint (it ignores files outside the base path)
ABS_FILE_PATH=$(cd "$(dirname "$FILE_PATH")" && echo "$(pwd)/$(basename "$FILE_PATH")")

# Determine package runner (--no-install: never prompt to fetch a missing
# package interactively — a hook must not hang CI or an editor)
RUNNER="npx --no-install"
command -v bunx &>/dev/null && RUNNER="bunx"

# prettier (advisory — log but don't block)
PRETTIER_OUTPUT=$($RUNNER prettier --check "$ABS_FILE_PATH" 2>&1 || true)
if [ -n "$PRETTIER_OUTPUT" ]; then
    echo "[ts-guard] prettier findings:"
    echo "$PRETTIER_OUTPUT" | head -10
fi

# eslint — enforced for @typescript-eslint/no-explicit-any (and parse errors)
# Capture exit code properly: set -e + || true makes $? always 0.
set +e
ESLINT_OUTPUT=$($RUNNER eslint "$ABS_FILE_PATH" 2>&1)
ESLINT_EXIT=$?
set -euo pipefail

# eslint exits 1 BOTH for rule violations and for a parse error. A parse error
# stops eslint before it runs any rules, so an explicit `any` behind a syntax
# error would never produce the "no-explicit-any" string — the guard would
# silently pass. Blocking on parse errors too closes that hole (a file whose
# syntax is broken must be fixed first; the `any` then surfaces).
if [ $ESLINT_EXIT -ne 0 ] && { echo "$ESLINT_OUTPUT" | grep -q "no-explicit-any" || echo "$ESLINT_OUTPUT" | grep -qE "Parsing error|Parse error"; }; then
    echo ""
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║  ❌ BLOCKED: Explicit 'any' type (or parse error).        ║"
    if echo "$ESLINT_OUTPUT" | grep -q "no-explicit-any"; then
        echo "║  Fix the violation before continuing.                     ║"
        echo "║  Use 'unknown' + narrowing/validation instead.            ║"
    else
        echo "║  Fix the syntax error before continuing.                   ║"
    fi
    echo "╚══════════════════════════════════════════════════════════════╝"
    echo ""
    echo "$ESLINT_OUTPUT" | grep -E "no-explicit-any|Parsing error|Parse error"
    echo ""
    exit 1
fi

# Other eslint warnings: advisory (log only)
if [ -n "$ESLINT_OUTPUT" ]; then
    echo "[ts-guard] eslint advisory:"
    echo "$ESLINT_OUTPUT" | head -30
fi

exit 0
