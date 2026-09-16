#!/usr/bin/env bash
# ts-guard.sh — PostToolUse hook for TS/TSX edits.
#
# Enforced (exit 2 blocks the edit):
#   - Project typecheck (`bun run typecheck` in the nearest app) — tsc is the
#     real authority for type correctness.
#   - eslint @typescript-eslint/no-explicit-any + parse errors.
# Advisory (logged, never blocks):
#   - prettier formatting findings.
#
# This is the SINGLE SOURCE OF TRUTH shared by .claude/hooks/ts-guard.sh and
# .factory/hooks/ts-guard.sh. Do not fork per-agent copies.
#
# Input: hook JSON on stdin (Claude Code {tool_input}, Factory Droid
# {tool_input}, or OpenCode {tool_args}/{files}/{changes}).
#
# Design constraints:
#   - Uses the project's OWN node_modules/.bin binaries only. `bunx`/`npx`
#     without --no-install fetch over the network and a hook must never block
#     on a download; the pre-push gate already documents that
#     bunx eslint / typescript-eslint cannot load under the repo's pinned
#     typescript 7.0 — the in-repo binaries are the only reliable loader.
#   - If no local eslint binary exists there is no eslint config either — the
#     "no-explicit-any" rule is not installed, so there is nothing to enforce;
#     skip rather than guess. Same for prettier.
#   - Typecheck runs from the nearest package.json project root, so the
#     change the agent just made is validated against the whole app.
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

# Resolve to an absolute path early: hooks can run from a CWD that differs
# from the file's location.
resolve_abs() {
  local p="$1"
  [[ "$p" == /* ]] || p="$(pwd)/$p"
  printf '%s' "$p"
}

find_project_root() {
  local dir
  dir="$(dirname "$1")"
  while [ "$dir" != "/" ] && [ "$dir" != "$HOME" ]; do
    if [ -f "$dir/package.json" ]; then
      printf '%s' "$dir"
      return 0
    fi
    dir="$(dirname "$dir")"
  done
  return 1
}

BLOCKED=0

while IFS= read -r FILE_PATH; do
  [ -n "$FILE_PATH" ] || continue
  case "$FILE_PATH" in *.ts|*.tsx) ;; *) continue ;; esac

  ABS="$(resolve_abs "$FILE_PATH")"
  [ -f "$ABS" ] || continue

  PROJECT_ROOT="$(find_project_root "$ABS")" || continue
  cd "$PROJECT_ROOT"

  BIN="$PROJECT_ROOT/node_modules/.bin"

  # ── Typecheck (enforced) ──────────────────────────────────────────────
  # Runs at the app level so the whole project typechecks, not just the file.
  if [ -d "$PROJECT_ROOT/node_modules/typescript" ]; then
    set +e
    TSC_OUTPUT="$(bun run typecheck 2>&1)"
    TSC_EXIT=$?
    set -euo pipefail

    if [ "$TSC_EXIT" -ne 0 ]; then
      {
        echo ""
        echo "❌ BLOCKED: TypeScript typecheck failed in $PROJECT_ROOT"
        echo "   (run: bun run typecheck)"
        echo ""
        printf '%s\n' "$TSC_OUTPUT" | head -40
        echo ""
      } >&2
      BLOCKED=1
    else
      printf '[ts-guard] typecheck OK (%s)\n' "$PROJECT_ROOT"
    fi
  else
    printf '[ts-guard] no local typescript in %s — skipping typecheck\n' "$PROJECT_ROOT"
  fi

  # ── prettier (advisory) ───────────────────────────────────────────────
  if [ -x "$BIN/prettier" ]; then
    set +e
    PRETTIER_OUTPUT="$("$BIN/prettier" --check "$ABS" 2>&1)"
    PRETTIER_EXIT=$?
    set -euo pipefail
    if [ "$PRETTIER_EXIT" -ne 0 ]; then
      echo "[ts-guard] prettier findings in $ABS:"
      printf '%s\n' "$PRETTIER_OUTPUT" | awk 'NR<=10'
    fi
  fi

  # ── eslint (enforced: no-explicit-any + parse errors) ─────────────────
  # eslint exits 1 BOTH for rule violations and parse errors. A parse error
  # stops eslint before rules run, so an `any` behind a syntax error would
  # never surface — blocking on parse errors closes that hole.
  #
  # Match in-shell instead of `… | grep -q`: grep -q exits at the first
  # match and SIGPIPEs the writer past the 64 KiB pipe buffer, and pipefail
  # then scores the pipeline 141 — a real violation would read as "no match".
  if [ -x "$BIN/eslint" ]; then
    set +e
    ESLINT_OUTPUT="$(cd "$PROJECT_ROOT" && "$BIN/eslint" "$ABS" 2>&1)"
    ESLINT_EXIT=$?
    set -euo pipefail

    BLOCK=""
    if [ "$ESLINT_EXIT" -ne 0 ]; then
      # Pre-existing toolchain incompatibility: typescript-eslint cannot load
      # under the repo's pinned typescript 7.0 ("typescript-eslint does not
      # support TS 7.0"). The pre-push gate treats this as a warn-and-continue
      # (a genuine lint violation still blocks the gate). Mirror that here: do
      # not block the edit on a broken eslint loader.
      case "$ESLINT_OUTPUT" in
        *"typescript-eslint does not support"*|*does\ not\ support\ TS*) ESLINT_TOOLCHAIN=1 ;;
      esac
      if [ -z "${ESLINT_TOOLCHAIN:-0}" ]; then
        case "$ESLINT_OUTPUT" in
          *no-explicit-any*)                BLOCK="explicit 'any' type" ;;
          *"Parsing error"*|*"Parse error"*) BLOCK="parse error" ;;
        esac
      fi
    fi

    if [ -n "$BLOCK" ]; then
      {
        echo ""
        echo "❌ BLOCKED: $BLOCK in $(basename "$ABS")"
        echo ""
        if [ "$BLOCK" = "parse error" ]; then
          echo "eslint could not parse this file, so no rules ran. Fix the"
          echo "syntax error before continuing — an 'any' behind it is invisible."
        else
          echo "Use 'unknown' + narrowing/validation instead of 'any'."
          echo "Fix the violation before continuing."
        fi
        echo ""
        printf '%s\n' "$ESLINT_OUTPUT" | grep -E "no-explicit-any|Parsing error|Parse error" || true
        echo ""
      } >&2
      BLOCKED=1
    elif [ -n "$ESLINT_OUTPUT" ]; then
      echo "[ts-guard] eslint advisory (not blocking):"
      printf '%s\n' "$ESLINT_OUTPUT" | awk 'NR<=30'
    fi
  fi
done <<< "$paths"

[ "$BLOCKED" -eq 1 ] && exit 2 || exit 0
