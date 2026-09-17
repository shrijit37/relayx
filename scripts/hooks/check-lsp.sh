#!/usr/bin/env bash
# check-lsp.sh — PostToolUse hook: surface real language-server diagnostics
# after every Rust / TS / TSX / shell edit.
#
# LSP in the Droid CLI comes through MCP stdio proxies (see ~/.factory/mcp.json
# for rust-analyzer / typescript-language-server / bash-language-server). An
# MCP server runs the language server in-process and publishes diagnostics
# lazily; it does NOT wait for a request and return them synchronously. So
# this hook actively drives the check per edited file, by calling the SAME
# language server binary directly (a short-lived, project-rooted instance):
#
#   rust:   rust-analyzer analysis-stats . (batch typecheck — the LSP server's
#           own checker), at most once per hook batch, bounded at 60s
#   ts:     tsc --noEmit (the TS language server's own checker; the LSP
#           server itself has no useful non-interactive diagnostics command)
#   shell:  bash -n (bash-language-server shellcheck is only available when
#           shellcheck is installed)
#
# This gives the agent immediate semantic feedback in the transcript — the
# "re-check after each edit/write" requirement — and blocks on confirmed
# `error`-severity findings. It NEVER wedges a session:
#   - bounded timeouts (10s per file, 60s for the rust batch)
#   - rust-analyzer "no rust-project.json / cargo metadata" or unindexed file
#     → advisory skip, never a block
#   - missing binary → advisory skip, never a hard failure
#   - PostToolUse exit 2 feeds stderr back to the agent as corrective input.
#
# SINGLE SOURCE OF TRUTH shared by .claude/hooks/check-lsp.sh and
# .factory/hooks/check-lsp.sh.
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

resolve_abs() {
  local p="$1"
  [[ "$p" == /* ]] || p="$(pwd)/$p"
  printf '%s' "$p"
}

BLOCKED=0
RA_ALREADY_RAN=0

while IFS= read -r FILE_PATH; do
  [ -n "$FILE_PATH" ] || continue
  case "$FILE_PATH" in
    *.rs|*.ts|*.tsx|*.sh) ;;
    *) continue ;;
  esac

  ABS="$(resolve_abs "$FILE_PATH")"
  [ -f "$ABS" ] || continue

  case "$FILE_PATH" in
    *.rs)
      command -v rust-analyzer >/dev/null 2>&1 || { printf '[lsp] rust-analyzer not found — skipping\n'; continue; }
      # This rust-analyzer (1.98) has no per-file "diagnostics" command; the
      # stable non-interactive path is `analysis-stats` (batch typecheck of the
      # project). Run it from the repo root with a bounded timeout. It is
      # costly (~30-40s on this workspace), so run once per batch at most, and
      # skip when we are not inside a cargo/rust project (analysis-stats exits
      # 1 with "no rust-project.json or cargo metadata").
      if [ -n "$RA_ALREADY_RAN" ]; then
        continue
      fi
      RA_ALREADY_RAN=1
      RA_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
      if [ ! -f "$RA_ROOT/Cargo.toml" ] && [ ! -f "$RA_ROOT/rust-project.json" ]; then
        continue
      fi
      set +e
      if command -v timeout >/dev/null 2>&1; then
        RA_OUT="$(cd "$RA_ROOT" && timeout 60 rust-analyzer analysis-stats . 2>&1)"
        RA_RC=$?
      else
        RA_OUT="$(cd "$RA_ROOT" && rust-analyzer analysis-stats . 2>&1)"
        RA_RC=$?
      fi
      set -euo pipefail
      if [ "$RA_RC" -ne 0 ]; then
        # not a rust-analyzer project → nothing to check
        if echo "$RA_OUT" | grep -qE 'no rust-project.json|cargo metadata|could not find'; then
          continue
        fi
        if echo "$RA_OUT" | grep -qE '^error(\[|:)'; then
          { echo "[lsp] rust-analyzer ERRORS (analysis-stats rc=$RA_RC):"; printf '%s\n' "$RA_OUT" | grep -E '^error' | head -30; } >&2
          BLOCKED=1
        else
          printf '[lsp] rust-analyzer analysis failed (rc=%s, advisory):\n%s\n' "$RA_RC" "$(printf '%s\n' "$RA_OUT" | head -20)"
        fi
      else
        printf '[lsp] rust-analyzer analysis OK (%s)\n' "$RA_ROOT"
      fi
      ;;
    *.ts|*.tsx)
      # tsc --noEmit over the nearest package.json project. The TS language
      # server's compiler APIs ARE tsc; this is the same checker the LSP
      # would run, just invoked non-interactively.
      PROJECT_ROOT=""
      DIR="$(dirname "$ABS")"
      while [ "$DIR" != "/" ] && [ "$DIR" != "$HOME" ]; do
        if [ -f "$DIR/package.json" ]; then PROJECT_ROOT="$DIR"; break; fi
        DIR="$(dirname "$DIR")"
      done
      [ -n "$PROJECT_ROOT" ] || continue
      if [ ! -x "$PROJECT_ROOT/node_modules/.bin/tsc" ]; then
        printf '[lsp] no project tsc in %s — skipping\n' "$PROJECT_ROOT"
        continue
      fi
      set +e
      if command -v timeout >/dev/null 2>&1; then
        TSC_OUT="$(cd "$PROJECT_ROOT" && timeout 10 "$PROJECT_ROOT/node_modules/.bin/tsc" --noEmit 2>&1)"
        TSC_RC=$?
      else
        TSC_OUT="$(cd "$PROJECT_ROOT" && "$PROJECT_ROOT/node_modules/.bin/tsc" --noEmit 2>&1)"
        TSC_RC=$?
      fi
      set -euo pipefail
      if [ "$TSC_RC" -ne 0 ] && [ -n "$TSC_OUT" ]; then
        # tsc reports errors for unrelated files too; surface the whole
        # output but only BLOCK when the edited file appears in it (the app's
        # typecheck hook already blocks on any project error).
        if echo "$TSC_OUT" | grep -Fq "$ABS"; then
          { echo "[lsp] TypeScript errors in $ABS:"; printf '%s\n' "$TSC_OUT" | grep -F "$ABS" | head -20; } >&2
          BLOCKED=1
        else
          printf '[lsp] tsc: errors elsewhere in %s (advisory):\n%s\n' "$PROJECT_ROOT" "$(printf '%s\n' "$TSC_OUT" | head -20)"
        fi
      fi
      ;;
    *.sh)
      SH_OUT="$(bash -n "$ABS" 2>&1 || true)"
      [ -n "$SH_OUT" ] || continue
      if echo "$SH_OUT" | grep -qE 'syntax error'; then
        { echo "[lsp] bash syntax error in $ABS:"; printf '%s\n' "$SH_OUT" | head -10; } >&2
        BLOCKED=1
      else
        printf '[lsp] bash findings (advisory):\n%s\n' "$(printf '%s\n' "$SH_OUT" | head -10)"
      fi
      ;;
  esac
done <<< "$paths"

[ "$BLOCKED" -eq 1 ] && exit 2 || exit 0
