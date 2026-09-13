#!/usr/bin/env bash
# Start the full relay-x stack for manual testing.
#   mock-upstream :8101 → gateway :8080/:9090 → control-plane :9091 → web :5173
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

PIDS=()
cleanup() {
  trap - INT TERM EXIT
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done
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

run mock-upstream  "$ROOT/target/debug/mock-upstream" --port 8101 --mode sse --chunks 10
sleep 1
run gateway        "$ROOT/target/debug/relay-gateway"  --config "$ROOT/apps/gateway/config/gateway.toml"

# bun must run from the right directory — two subshells, backgrounded
( cd "$ROOT/apps/control-plane" && bun run src/index.ts ) > /tmp/relayx-control-plane.log 2>&1 &
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
echo "  stop: Ctrl+C"
wait