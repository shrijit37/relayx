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

# Use the project's own installed binaries. Never fall back to a package
# runner: `bunx`/`npx` without --no-install fetch a missing package over the
# network, and a hook must never block on a download. No local eslint also
# means no eslint config — nothing to enforce, so skip.
BIN="$PROJECT_ROOT/node_modules/.bin"
[ -x "$BIN/eslint" ] || exit 0

# prettier (advisory — log but don't block)
if [ -x "$BIN/prettier" ]; then
    PRETTIER_OUTPUT=$("$BIN/prettier" --check "$ABS_FILE_PATH" 2>&1 || true)
    if [ -n "$PRETTIER_OUTPUT" ]; then
        echo "[ts-guard] prettier findings:"
        printf '%s\n' "$PRETTIER_OUTPUT" | awk 'NR<=10'
    fi
fi

# eslint — enforced for @typescript-eslint/no-explicit-any (and parse errors)
# Capture exit code properly: set -e + || true makes $? always 0.
set +e
ESLINT_OUTPUT=$("$BIN/eslint" "$ABS_FILE_PATH" 2>&1)
ESLINT_EXIT=$?
set -euo pipefail

# eslint exits 1 BOTH for rule violations and for a parse error. A parse error
# stops eslint before it runs any rules, so an explicit `any` behind a syntax
# error would never produce the "no-explicit-any" string — the guard would
# silently pass. Blocking on parse errors too closes that hole (a file whose
# syntax is broken must be fixed first; the `any` then surfaces).
#
# Matched in-shell rather than with `echo … | grep -q`: grep -q exits at the
# first match, which SIGPIPEs the writer once the output exceeds the 64 KiB
# pipe buffer, and `pipefail` then scores the pipeline 141 — so a real
# violation reads as "no match" and the guard silently lets it through.
BLOCK=""
if [ "$ESLINT_EXIT" -ne 0 ]; then
    case "$ESLINT_OUTPUT" in
        *no-explicit-any*)                  BLOCK="explicit 'any' type" ;;
        *"Parsing error"*|*"Parse error"*)  BLOCK="parse error" ;;
    esac
fi

if [ -n "$BLOCK" ]; then
    {
        echo ""
        echo "❌ BLOCKED: $BLOCK in $(basename "$ABS_FILE_PATH")"
        echo ""
        if [ "$BLOCK" = "parse error" ]; then
            echo "eslint could not parse this file, so no rules ran. Fix the"
            echo "syntax error before continuing — an 'any' behind it is invisible."
        else
            echo "Use 'unknown' + narrowing/validation instead of 'any'."
            echo "Fix the violation before continuing."
        fi
        echo ""
        printf '%s\n' "$ESLINT_OUTPUT" | grep -E "no-explicit-any|Parsing error|Parse error"
        echo ""
    } >&2
    exit 2
fi

# Other eslint findings (incl. prettier/prettier formatting): advisory, log only
if [ -n "$ESLINT_OUTPUT" ]; then
    echo "[ts-guard] eslint advisory:"
    printf '%s\n' "$ESLINT_OUTPUT" | awk 'NR<=30'
fi

exit 0
