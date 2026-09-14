#!/usr/bin/env bash
set -euo pipefail

# Inspect Rust files for forbidden patterns.
#
# Usage:
#   check-rust-policy.sh            # working tree + index + untracked
#   check-rust-policy.sh --all      # every tracked and untracked .rs file
#   check-rust-policy.sh file.rs…   # explicit paths
#
# Narrow #[allow(...)] / #[expect(...)] for FFI, generated code, and
# compiler false positives is permitted. #[allow(dead_code)] and
# #[expect(dead_code)] are not.

usage() {
  echo "Usage: $0 [--all] [file.rs ...]" >&2
}

list_changed_rust_files() {
  {
    git diff --name-only
    git diff --cached --name-only
    git ls-files --others --exclude-standard
  } | sort -u | grep -E '\.rs$' || true
}

list_all_rust_files() {
  {
    git ls-files -z -- '*.rs' | tr '\0' '\n'
    git ls-files --others --exclude-standard | grep -E '\.rs$' || true
  } | sort -u
}

# ── Claude Code hook mode ────────────────────────────────────
# When invoked by Claude Code, stdin contains JSON with tool_input.
# Extract the file path and check just that one file.
INPUT=""
if [[ ! -t 0 ]]; then
  INPUT=$(cat)
fi

if [[ -n "$INPUT" ]] && echo "$INPUT" | jq -e '.tool_input' &>/dev/null 2>&1; then
  FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // empty' 2>/dev/null || true)
  [[ -n "$FILE_PATH" ]] || exit 0
  case "$FILE_PATH" in *.rs) ;; *) exit 0 ;; esac
  [[ -f "$FILE_PATH" ]] || exit 0
  files="$FILE_PATH"
elif [[ -n "$INPUT" ]]; then
  # Not a hook invocation — fall through to CLI logic below
  :
fi

# ── CLI mode (pre-commit, CI, manual) ────────────────────────
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

# Only resolve files if not already set by hook mode above
if [[ -z "${files:-}" ]]; then
  if [[ "${1:-}" == "--all" ]]; then
    files="$(list_all_rust_files)"
  elif [[ $# -gt 0 ]]; then
    files="$(printf '%s\n' "$@")"
  else
    files="$(list_changed_rust_files)"
  fi
fi

rust_files=()
while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  rust_files+=("$file")
done <<< "$files"

if [[ ${#rust_files[@]} -eq 0 ]]; then
  exit 0
fi

# Exempt test files from .unwrap() / .expect() checks.
is_test_file() {
  grep -qE '(test|tests|_test\.rs|test_)' <<< "$1" && return 0
  return 1
}

failed=0

check_pattern() {
  local description="$1"
  local pattern="$2"
  local test_exempt="${3:-false}"
  local file

  for file in "${rust_files[@]}"; do
    [[ -f "$file" ]] || continue

    if [[ "$test_exempt" == "true" ]] && is_test_file "$file"; then
      continue
    fi

    if grep -nE "$pattern" "$file"; then
      echo >&2
      echo "ERROR: Forbidden Rust pattern detected: $description" >&2
      echo "File: $file" >&2
      failed=1
    fi
  done
}

# ─────────────────────────────────────────────────────────────
# DEAD CODE — hard ban, including whitespace-disguised forms
# and mixed lint lists such as #[allow(unused, dead_code)]
# ─────────────────────────────────────────────────────────────

check_pattern \
  "#[allow(dead_code)] / #![allow(dead_code)]" \
  '#!?\[allow\s*\([^]]*dead_code'

check_pattern \
  "#[expect(dead_code)] / #![expect(dead_code)]" \
  '#!?\[expect\s*\([^]]*dead_code'

# ─────────────────────────────────────────────────────────────
# TEMPORARY / ESCAPE-HATCH MACROS
# ─────────────────────────────────────────────────────────────

check_pattern \
  "todo!()" \
  '\btodo!\s*\('

check_pattern \
  "unimplemented!()" \
  '\bunimplemented!\s*\('

# ─────────────────────────────────────────────────────────────
# PANIC-PRONE SHORTCUTS
# .unwrap_or / .unwrap_or_else / .unwrap_err are allowed.
# Test files are exempt — unwrap/expect in a failing test is fine.
# ─────────────────────────────────────────────────────────────

check_pattern \
  ".unwrap()" \
  '\.unwrap\s*\(\s*\)' \
  true

check_pattern \
  ".expect(...)" \
  '\.expect\s*\(' \
  true

# ─────────────────────────────────────────────────────────────

if [[ "$failed" -ne 0 ]]; then
  echo >&2
  echo "Rust policy violation." >&2
  echo "Fix the underlying issue instead of suppressing the compiler or using escape hatches." >&2
  echo "Narrow #[allow(...)] is permitted except dead_code; ask before any exception." >&2
  exit 2
fi

exit 0
