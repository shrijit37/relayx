#!/usr/bin/env bash
# sweep-build.sh — keep build artifacts from bloating disk without hurting
# build or runtime performance.
#
# The dev/release profiles are deliberately tuned (see Cargo.toml), so the
# bloat is accumulation, not configuration: target/ accumulates stale
# incremental dirs and dep artifacts from old compiler/feature combos.
#
# This script uses `cargo-sweep` (the standard safe cargo GC): it prunes
# target artifacts whose .fingerprint entry is older than N days, while
# keeping artifacts used in the last N days so rebuilds stay fast. Unlike
# `cargo clean`, today's warm cache survives.
#
# Usage:
#   scripts/sweep-build.sh              # maximal safe prune (keep today's cache)
#   scripts/sweep-build.sh --days 7     # keep the last week, prune the rest
#   scripts/sweep-build.sh --all        # maximal safe prune (same as default)
#   scripts/sweep-build.sh --web        # also clear regenerable web build caches
#   scripts/sweep-build.sh --dry-run    # show what would be removed, remove nothing
#
# Exit codes: 0 on success, 2 on usage error.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DAYS=30           # default: maximal safe prune (keep artifacts touched today)
WEB=0
ALL=0
DRY=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --days)
      [ "$#" -ge 2 ] || { echo "usage: --days N" >&2; exit 2; }
      DAYS="$2"
      shift 2
      ;;
    --all) ALL=1; shift ;;
    --web) WEB=1; shift ;;
    --dry-run) DRY=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if ! command -v cargo-sweep >/dev/null 2>&1; then
  echo "cargo-sweep not found — installing via cargo (may take a few minutes)…"
  cargo install cargo-sweep
fi

before="$(du -sh target 2>/dev/null | awk '{print $1}' || echo '0B')"
echo "target/ before: $before"

# `--all` (and the default) prune everything not used today — the maximal
# release of disk. In this repo that reclaims most of target/ (21 of 37 GB)
# while keeping today's warm cache so rebuilds stay fast. Pass `--days N`
# for a gentler window that keeps the last N days intact.
if [ "$ALL" -eq 1 ] || [ "$DAYS" -eq 30 ]; then
  args=(--all)
else
  args=(--time "$DAYS")
fi
[ "$DRY" -eq 1 ] && args+=(--dry-run)

# cargo-sweep prints the amount it removed; wrap to keep the diff visible.
set +e
output="$(cargo sweep "${args[@]}" 2>&1)"
set -e
if [ -n "$output" ]; then
  printf '%s\n' "$output" | sed 's/^/  cargo-sweep: /'
fi

if [ "$WEB" -eq 1 ] && [ "$DRY" -eq 0 ]; then
  echo "clearing regenerable web build caches:"
  for d in "apps/web/.output" "apps/web/node_modules/.vite" "apps/web/node_modules/.cache"; do
    if [ -d "$d" ]; then
      size="$(du -sh "$d" | awk '{print $1}')"
      echo "  rm -rf $d ($size)"
      rm -rf "$d"
    fi
  done
elif [ "$WEB" -eq 1 ]; then
  echo "(dry run) would clear regenerable web build caches (.output, node_modules/.vite, node_modules/.cache)"
fi

after="$(du -sh target 2>/dev/null | awk '{print $1}' || echo '0B')"
echo "target/ after:  $after"
echo "done."
exit 0
