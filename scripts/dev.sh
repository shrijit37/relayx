#!/usr/bin/env bash
# Start the full relay-x stack for manual testing.
#   mock-upstream :8101 → gateway :8080/:9090 → control-plane :9091 → web :5173
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

PIDS=()
cleanup() {
  trap - INT TERM EXIT
  # Kill each tracked process group (negative PID = the setsid group).
  for p in "${PIDS[@]:-}"; do kill -- "-$p" 2>/dev/null || kill "$p" 2>/dev/null || true; done
}
trap 'cleanup' INT TERM EXIT

run() {
  local name=$1; shift
  "$@" > "/tmp/relayx-$name.log" 2>&1 &
  PIDS+=("$!")
  echo "  started $name (pid $!) → /tmp/relayx-$name.log"
}

# DB
if ! docker ps --format '{{.Names}}' | grep -qx relayx-pg; then
  echo "relayx-pg not running — start it first:"
  echo "  docker run -d --name relayx-pg -p 127.0.0.1:5433:5432 -e POSTGRES_PASSWORD=relayx-dev -e POSTGRES_USER=relayx -e POSTGRES_DB=relayx postgres:16-alpine"
  exit 1
fi

echo "relay-x dev stack"

# Rust services run under cargo watch: first run builds, later runs rebuild and
# restart on any workspace change (cargo's build lock serializes the two
# watchers; the second rebuild is a no-op compile).
#
# setsid gives each watcher its own process group so Ctrl+C reaps the whole
# subtree (cargo-watch + the compiled service). Without it, $! is the subshell
# pid and killing it orphans cargo-watch and the service with ports 8101/9090
# still bound. The (cd "$ROOT" && …) anchor keeps cargo-watch's relative
# config/workspace paths correct when dev.sh runs outside the repo root.
( cd "$ROOT" && setsid cargo watch -x 'run -p mock-upstream -- --port 8101 --mode sse --chunks 10' ) > /tmp/relayx-mock-upstream.log 2>&1 &
PIDS+=($!); echo "  started mock-upstream (pid $!) → /tmp/relayx-mock-upstream.log"

( cd "$ROOT" && setsid cargo watch -x 'run -p relay-gateway -- --config apps/gateway/config/gateway.toml' ) > /tmp/relayx-gateway.log 2>&1 &
PIDS+=($!); echo "  started gateway (pid $!) → /tmp/relayx-gateway.log"

# bun must run from the right directory — two subshells, backgrounded
( cd "$ROOT/apps/control-plane" && bun run dev ) > /tmp/relayx-control-plane.log 2>&1 &
PIDS+=($!); echo "  started control-plane (pid $!) → /tmp/relayx-control-plane.log"

( cd "$ROOT/apps/web" && bun run dev -- --port 5173 --strictPort ) > /tmp/relayx-web.log 2>&1 &
PIDS+=($!); echo "  started web (pid $!) → /tmp/relayx-web.log"

sleep 2
echo
echo "  all up"
echo "  web            http://localhost:5173"
echo "  control-plane  http://127.0.0.1:9091"
echo "  gateway        http://127.0.0.1:8080  (admin :9090)"
echo "  mock-upstream  http://127.0.0.1:8101"
echo
echo "  logs: /tmp/relayx-{mock-upstream,gateway,control-plane,web}.log"
echo "  watch: scripts/logs.sh              (all services)"
echo "        scripts/logs.sh gateway web   (selected services)"
echo "  stop: Ctrl+C"
wait