#!/usr/bin/env bash
# Wrapper: delegate to the single-source shared hook (fail-open).
# Canonical implementation: scripts/hooks/check-rust-policy.sh
# Shared logic (exec guard, fail-open): scripts/hooks/wrapper-common.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/hooks/wrapper-common.sh"
