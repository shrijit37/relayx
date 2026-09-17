#!/usr/bin/env bash
# check-docs.sh — PostToolUse hook: keep relay-x docs truthful.
#
# Runs the documentation verifier after edits to docs/ or the root markdown
# contract files, and prints any drift so it can be fixed in the same change.
# Advisory by design: it never blocks an edit, it makes the drift visible.
#
# SINGLE SOURCE OF TRUTH shared by .claude/hooks/check-docs.sh and
# .factory/hooks/check-docs.sh.
set -euo pipefail

INPUT=$(cat)

FILE_PATH=""
if command -v jq &>/dev/null; then
  FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // .file_path // .filePath // empty' 2>/dev/null || true)
fi
[ -n "$FILE_PATH" ] || exit 0

# Resolve to an absolute path. Claude Code / Factory Droid normally pass one,
# but a relative path would silently slip past the patterns below and skip
# the verifier.
if [[ "$FILE_PATH" != /* ]]; then
  DIR="$(dirname "$FILE_PATH")"
  if [[ -d "$DIR" ]]; then
    FILE_PATH="$(cd "$DIR" && pwd)/$(basename "$FILE_PATH")"
  fi
fi

# In scope: docs/, and the root markdown contract files.
case "$FILE_PATH" in
  */docs/*|*/README.md|*/AGENTS.md|*/CLAUDE.md) ;;
  *) exit 0 ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
[ -x "$ROOT/scripts/verify-docs.sh" ] || exit 0

set +e
OUTPUT=$("$ROOT/scripts/verify-docs.sh" 2>&1)
STATUS=$?
set -euo pipefail

if [ "$STATUS" -ne 0 ]; then
  {
    echo ""
    echo "📄 Documentation drift detected after editing $(basename "$FILE_PATH")."
    echo ""
    printf '%s\n' "$OUTPUT" | grep -vE '^✓ ' | grep -v '^$' || printf '%s\n' "$OUTPUT"
    echo ""
    echo "Fix the docs in this change, or regenerate counts with:"
    echo "  scripts/verify-docs.sh --write"
    echo ""
  } >&2
fi

exit 0
