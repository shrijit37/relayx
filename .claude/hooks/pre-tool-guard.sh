#!/usr/bin/env bash
# Wrapper: delegate to the single-source shared hook.
# Canonical implementation: scripts/hooks/pre-tool-guard.sh
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec "$root/scripts/hooks/pre-tool-guard.sh" "$@"
