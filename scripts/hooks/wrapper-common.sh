#!/usr/bin/env bash
# wrapper-common.sh — sourced by the thin per-agent hook wrappers
# (.claude/hooks/*, .factory/hooks/*). Delegates to the single-source
# canonical implementation in scripts/hooks/ and fails OPEN (exit 0) when
# the target is missing or not executable, so a broken install can never
# wedge an edit/stop. Real enforcement happens in the canonical script;
# a wrapper must not add behavior of its own.
set -euo pipefail

# Resolved at source time: BASH_SOURCE[1] is the wrapper that sourced us.
_wrapper="${BASH_SOURCE[1]:-}"
_wrapper="$(cd "$(dirname "$_wrapper")" && pwd)/$(basename "$_wrapper")"
_name="$(basename "$_wrapper")"
_root="$(cd "$(dirname "$_wrapper")/../.." && pwd)"
_target="$_root/scripts/hooks/$_name"

if [[ ! -f "$_target" || ! -x "$_target" ]]; then
  printf 'hook wrapper: canonical script missing or not executable: %s (fail-open)\n' "$_target" >&2
  exit 0
fi
exec "$_target" "$@"
