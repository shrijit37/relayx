#!/usr/bin/env bash
# Wrapper: delegate to the single-source shared hook.
# Canonical implementation: scripts/hooks/ts-guard.sh
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec "$root/scripts/hooks/ts-guard.sh" "$@"
