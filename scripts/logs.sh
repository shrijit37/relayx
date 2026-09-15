#!/usr/bin/env bash
# Unified log viewer for the relay-x dev stack (see scripts/dev.sh).
#
# Merges the per-service logs written by dev.sh into one color-coded stream:
#   - every service is followed live; if a log does not exist yet (dev.sh not
#     started, or a service still booting) the follower waits and picks it up
#   - each line is prefixed with its service label and a wall-clock timestamp
#     in the current locale (%c); set RELAYX_TS=0 to disable timestamps
#   - severity is highlighted: errors red, warnings yellow
#   - cargo build chatter (cargo-watch status lines) is dimmed so actual
#     runtime output stands out
#   - control-plane pino JSON lines are reduced to readable [level] messages
#
# Usage:
#   scripts/logs.sh                    # follow every service
#   scripts/logs.sh gateway web        # follow only the given services
#   scripts/logs.sh --help
#
# Env overrides:
#   RELAYX_LOG_DIR=/path   where the per-service logs live (default /tmp)
#   RELAYX_TAIL_LINES=N    history lines replayed on attach (default 50)
#   RELAYX_TS=0            disable per-line timestamps
set -euo pipefail

LOG_DIR="${RELAYX_LOG_DIR:-/tmp}"
TAIL_LINES="${RELAYX_TAIL_LINES:-50}"
SHOW_TS="${RELAYX_TS:-1}"

ESC=$'\033'
BOLD=$'\033[1m'
DIM=$'\033[2m'
RESET=$'\033[0m'
RED=$'\033[1;31m'
YEL=$'\033[33m'

# name | log file suffix | color | description
SERVICES=(
  "mock-upstream|relayx-mock-upstream.log|$ESC[36m|mock LLM server :8101"
  "gateway|relayx-gateway.log|$ESC[32m|gateway data plane :8080"
  "control-plane|relayx-control-plane.log|$ESC[35m|control plane API :9091"
  "web|relayx-web.log|$ESC[33m|web editor :5173"
)

usage() {
  echo "usage: $(basename "$0") [service ...]"
  echo "services: mock-upstream gateway control-plane web"
  echo "follows all services by default; pass names to limit the set."
  exit 0
}

for arg in "$@"; do
  case "$arg" in
    -h|--help) usage ;;
  esac
done

# Widen the label column to the longest service name so all prefixes align.
WIDTH=0
for s in "${SERVICES[@]}"; do
  name="${s%%|*}"
  ((${#name} > WIDTH)) && WIDTH=${#name}
done

# Resolve the requested service set.
selected=()
if (($# > 0)); then
  for want in "$@"; do
    found=0
    for s in "${SERVICES[@]}"; do
      [[ "$want" == "${s%%|*}" ]] && { found=1; selected+=("$want"); }
    done
    if ((found == 0)); then
      echo "unknown service: $want (valid: mock-upstream gateway control-plane web)" >&2
      exit 1
    fi
  done
else
  for s in "${SERVICES[@]}"; do selected+=("${s%%|*}"); done
fi

# One-time header so it is always clear which files are being watched.
echo
echo "  relay-x combined logs — following ${#selected[@]} service(s), dir: $LOG_DIR"
for s in "${SERVICES[@]}"; do
  IFS='|' read -r name file color desc <<<"$s"
  for want in "${selected[@]}"; do
    if [[ "$want" == "$name" ]]; then
      printf '    %s%s%-*s%s  %s%s%s%s\n' \
        "$BOLD" "$color" "$WIDTH" "$name" "$RESET" \
        "$DIM" "$LOG_DIR/$file" "  — $desc" "$RESET"
      break
    fi
  done
done
echo "    quit: Ctrl+C"
echo

# Stream one service: prefix every line with label + timestamp, highlight
# severity, and dim cargo build status lines. Runs as a background job so
# all services are tailed in parallel.
follow() {
  local name=$1 file=$2 color=$3 desc=$4
  local label
  label=$(printf '%s%s%-*s%s' "$BOLD" "$color" "$WIDTH" "$name" "$RESET")

  # If the log does not exist yet (dev.sh not running, service still
  # booting), announce it once and keep waiting — never drop the service.
  if [ ! -r "$file" ]; then
    printf '%s%s%-*s%s  %swaiting for %s (start dev.sh)…%s\n' \
      "$BOLD" "$color" "$WIDTH" "$name" "$RESET" "$DIM" "$file" "$RESET"
    while [ ! -r "$file" ]; do sleep 1; done
  fi

  # -n replays recent history; -F follows across truncation/rotation when
  # dev.sh restarts a service. sed strips any ANSI the services themselves
  # emit so our own coloring stays clean.
  tail -n "$TAIL_LINES" -F "$file" 2>/dev/null \
    | LC_ALL=C sed -u 's/\x1b\[[0-9;]*[A-Za-z]//g' \
    | while IFS= read -r line; do
        local body="$line" body_color="" body_style=""

        # control-plane emits pino JSON — reduce to "[level] message".
        if [[ "$body" =~ ^\{ ]]; then
          local lvl=0 lvl_name="info" msg="" code=""
          [[ "$body" =~ \"level\":([0-9]+) ]] && lvl="${BASH_REMATCH[1]:-0}"
          [[ "$body" =~ \"msg\":\"([^\"]*)\" ]] && msg="${BASH_REMATCH[1]:-}"
          [[ "$body" =~ \"code\":\"([A-Za-z0-9_]+)\" ]] && code="${BASH_REMATCH[1]:-}"
          msg="${msg//\\\"/\"}"
          msg="${msg//\\n/ }"
          if ((lvl >= 50)); then lvl_name="error"; body_color="$RED"
          elif ((lvl >= 40)); then lvl_name="warn"; body_color="$YEL"
          fi
          [ -n "$code" ] && msg="$msg ($code)"
          body="[$lvl_name] $msg"
        else
          local lower="${body,,}"
          if [[ "$lower" =~ error|fatal|panic|exception|unreachable|crashed|EADDRINUSE|ECONNREFUSED ]]; then
            body_color="$RED"
          elif [[ "$lower" =~ warn|deprecated|timeout|retry ]]; then
            body_color="$YEL"
          elif [[ "$body" =~ ^\[(Running|Waiting|Signal|Build) ]] \
            || [[ "$body" =~ ^[[:space:]]+(Finished|Compiling|Blocking|Running|Building|Updating|Checking) ]]; then
            body_style="$DIM"
          fi
        fi

        if ((SHOW_TS == 1)); then
          printf '%s %s%s%s %s%s%s\n' \
            "$label" "$DIM" "$(date '+%c')" "$RESET" \
            "$body_color$body_style$body$RESET"
        else
          printf '%s %s%s%s\n' "$label" "$body_color$body_style$body$RESET"
        fi
      done
}

for s in "${SERVICES[@]}"; do
  IFS='|' read -r name file color desc <<<"$s"
  for want in "${selected[@]}"; do
    if [[ "$want" == "$name" ]]; then
      follow "$name" "$LOG_DIR/$file" "$color" "$desc" &
      break
    fi
  done
done

# Ctrl+C stops every follower (kill 0 = our process group). Without this,
# backgrounded tails keep running with ports/terminals looking busy.
trap 'trap - INT TERM; kill 0 2>/dev/null; exit 0' INT TERM
wait
