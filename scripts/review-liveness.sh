#!/usr/bin/env bash
#
# review-liveness.sh — live "is the AI review still alive" progress comments.
#
# The `alibaba/open-code-review` action (`ocr` CLI) posts its sticky summary
# only AFTER the review finishes, so a long run looks silent on the PR. This
# script gives that action a live heartbeat: a single marker-guarded issue
# comment per run that is updated in place as the review progresses and is
# deleted once the review completes (or rewritten into a red "stopped" banner
# when it dies).
#
# The script only talks to the GitHub API through `gh` with the ambient
# GITHUB_TOKEN, so it needs the same token permissions as the review job:
#   issues: write  (issue comments)
#   pull-requests: write
#
# Comment identity: a body marker `<!-- ocr-liveness -->` + a run tag
# (`run_id`) so concurrent runs never fight over the same comment, and stale
# comments from a dead runner can be cleaned up by a later run or by the
# independent `workflow_run` cleanup workflow (review-cleanup.yml).
#
# Subcommands:
#   start  <pr> <run-id>  -- delete any stale liveness comments for previous
#                           runs, post a fresh one, print its comment ID.
#   watch  <pr> <comment-id> <run-id> [log] -- poll the OCR stderr log and
#                           PATCH the comment in place every ~15s. Keeps the
#                           last 5 file-ish progress lines + a heartbeat even
#                           when the agent hangs. Exits cleanly when the log
#                           stops growing (the review finished).
#   finish <pr> <comment-id> <outcome> -- outcome success: delete the comment;
#                           anything else: rewrite it into a red banner naming
#                           the failure and keep it (commenting on the PR is
#                           the whole point — a dead agent must never be
#                           silent).
#
# The script is deliberately dependency-light for a GitHub-hosted runner:
# bash, gh (preinstalled), jq (preinstalled) for JSON.

set -euo pipefail

MARKER="<!-- ocr-liveness -->"
POLL_SECONDS="${REVIEW_LIVENESS_POLL_SECONDS:-15}"
IDLE_EXIT_SECONDS="${REVIEW_LIVENESS_IDLE_EXIT:-90}"
GH="${GH_BIN:-gh}"
REPO="${GITHUB_REPOSITORY:-${GITHUB_REPO:-}}"

die() {
  printf 'review-liveness: %s\n' "$*" >&2
  exit 1
}

require_gh() {
  command -v "$GH" >/dev/null 2>&1 || die "gh CLI not found (install gh or set GH_BIN)"
  "$GH" auth status >/dev/null 2>&1 \
    || die "gh is not authenticated (set GH_TOKEN / GITHUB_TOKEN)"
  [ -n "${GITHUB_REPOSITORY:-}" ] || die "GITHUB_REPOSITORY unset"
}

# run_id tag — unique enough per repo+run even when PRs share a number.
run_tag() {
  printf '<!-- ocr-liveness run=%s -->' "$1"
}

# Post a fresh liveness comment for run $1, deleting any stale liveness
# comments from prior runs. Prints the comment ID.
start() {
  require_gh
  local pr="$1" run_id="$2"
  [ -n "$pr" ] && [ -n "$run_id" ] || die "start requires <pr> <run-id>"

  # Remove liveness comments from prior runs (a stale comment from a dead
  # runner would otherwise accumulate next to the new one).
  "$GH" api "repos/${GITHUB_REPOSITORY}/issues/${pr}/comments?per_page=100&sort=created&direction=desc" \
    | jq -r --arg marker "$MARKER" '.[] | select(.body | contains($marker)) | .id' \
    | while read -r id; do
      "$GH" api -X DELETE "repos/${GITHUB_REPOSITORY}/issues/comments/${id}" || true
    done

  local body
  body="$(printf '%s\n%s\n🔄 **AI review in progress** — watching run \`%s\` …' \
    "$MARKER" "$(run_tag "$run_id")" "$run_id")"
  "$GH" api "repos/${GITHUB_REPOSITORY}/issues/${pr}/comments" \
    -f "body=${body}" | jq -r '.id'
}

# PATCH the comment with the given body.
patch_comment() {
  local id="$1" body="$2"
  "$GH" api -X PATCH "repos/${GITHUB_REPOSITORY}/issues/comments/${id}" \
    -f "body=${body}" >/dev/null
}

# Rewrite the liveness comment into a red "stopped" banner that stays on the
# PR. Called on failure / cancellation / unexpected termination.
fail_banner() {
  local id="$1" reason="$2" heartbeat="$3" last_files="$4"
  local body
  body="$(printf '%s\n%s\n❌ **AI review stopped** — %s\n\n_Last activity: %s_' \
    "$MARKER" "$(run_tag "${CURRENT_RUN_ID:-}")" "$reason" "$heartbeat")"
  if [ -n "$last_files" ]; then
    body+=$(printf '\n\n**Last files reviewed:**\n%s\n' "$last_files")
  fi
  patch_comment "$id" "$body"
}

if ! command -v jq >/dev/null 2>&1; then
  die "jq not found (required; preinstalled on GitHub-hosted runners)"
fi

# Given a raw OCR log line, strip ANSI color, trim whitespace, and drop any
# non-informative prefixes so only the meaningful payload survives.
visible_line() {
  local line="$1" cleaned
  cleaned="$(printf '%s' "$line" \
    | sed -E $'s/\033\[[0-9;?]*m//g' \
    | sed -E 's/^[[:space:]]*(\[ocr\][[:space:]]*)?//' \
    | sed -E 's/[[:space:]]+$//' \
    | sed -E 's/^[[:space:]]+//')"
  [ -n "$cleaned" ] || return 1
  printf '%s\n' "$cleaned"
}

# A line that plausibly names a file: contains a path-ish `/` or matches a
# common source/config extension. Progress UI emits "[ocr]   ▶ <file>", so a
# single word with an extension is usually the filename.
fileish() {
  local s="$1"
  [[ "$s" == */* ]] && return 0
  [[ "$s" == *.rs || "$s" == *.ts || "$s" == *.tsx || "$s" == *.go || \
     "$s" == *.py || "$s" == *.js || "$s" == *.json || "$s" == *.md || \
     "$s" == *.toml || "$s" == *.yml || "$s" == *.yaml || "$s" == *.sh || \
     "$s" == *.sql || "$s" == *.h || "$s" == *.c || "$s" == *.cpp ]] && return 0
  return 1
}

# Poll the given log until it stops growing (the review finished) and keep the
# last 5 progress lines in the comment. Exits on EOF so the final `always()`
# step can finish/delete the comment.
watch() {
  require_gh
  local pr="$1" comment_id="$2" run_id="$3" log="${4:-/tmp/ocr-stderr.log}"
  [ -n "$comment_id" ] || die "watch requires <pr> <comment-id> <run-id> [log]"

  local since=0 heartbeat=0
  local prev_body="" body lines seen last updates size
  while :; do
    sleep "$POLL_SECONDS"

    # New bytes since the last poll; also detect log truncation/rotation.
    size="$(wc -c < "$log" 2>/dev/null || echo 0)"
    updates=""
    if [ "$size" -gt "$since" ]; then
      updates="$(tail -c +"$((since + 1))" "$log" 2>/dev/null || true)"
    elif [ "$size" -lt "$since" ]; then
      # Log was rotated/truncated: restart from its current start.
      since=0
      updates="$(cat "$log" 2>/dev/null || true)"
    fi
    [ -n "$updates" ] && since="$size"

    if [ -n "$updates" ]; then
      while IFS= read -r line; do
        if seen="$(visible_line "$line")" && fileish "$seen"; then
          lines+="${seen}"$'\n'
        fi
      done <<< "$updates"
      lines="$(printf '%s' "${lines}" | grep -v '^$' | tail -n 5)"
      heartbeat="$(date +%s)"
    fi

    body="$(printf '%s\n%s\n🔄 **AI review in progress** — run \`%s\`\n' \
      "$MARKER" "$(run_tag "$run_id")" "$run_id")"
    if [ -n "${lines:-}" ]; then
      body+=$(printf '%s\n' \
        "<details><summary>Last 5 files reviewed</summary>" \
        "" \
        "${lines}" \
        "" \
        "</details>" \
        "")
    else
      body+=$(printf '%s\n' "_Waiting for the first file…_")
    fi
    body+="$(printf '\n_Last activity: %s_\n' \
      "$(date -u -d "@${heartbeat}" '+%Y-%m-%d %H:%M:%S UTC' 2>/dev/null || echo "$heartbeat")")"

    if [ "$body" != "$prev_body" ]; then
      patch_comment "$comment_id" "$body" || true
      prev_body="$body"
    fi

    # Idle-exit: the log stopped growing for IDLE_EXIT_SECONDS. Fall back to
    # polling a couple more times before exiting, in case a long LLM call is
    # between file bytes (the action captures the whole run to the log).
    if [ "$size" = "$since" ] \
      && [ $(( $(date +%s) - heartbeat )) -ge "$IDLE_EXIT_SECONDS" ]; then
      break
    fi
  done
}

# Finalize the liveness comment after the review: delete on success, red
# banner otherwise.
finish() {
  require_gh
  local pr="$1" comment_id="$2" outcome="$3"
  [ -n "$comment_id" ] || return 0
  if [ "$outcome" = "success" ]; then
    "$GH" api -X DELETE "repos/${GITHUB_REPOSITORY}/issues/comments/${comment_id}" >/dev/null || true
  else
    fail_banner "$comment_id" "outcome: ${outcome}" "$(date -u '+%Y-%m-%d %H:%M:%S UTC')" ""
  fi
}

# Called by the review-cleanup workflow after a workflow_run completes. Finds
# liveness comments for the given PR+run and deletes on success, banners on
# failure. Unlike `finish`, we don't know the comment id, so we search by
# run tag in the comment body.
finish_pr() {
  require_gh
  local pr="$1" outcome="$2" run_id="$3"
  [ -n "$pr" ] && [ -n "$run_id" ] || die "finish-pr requires <pr> <outcome> <run-id>"

  local comment_id
  comment_id="$("$GH" api \
    "repos/${GITHUB_REPOSITORY}/issues/${pr}/comments?per_page=100&sort=created&direction=desc" \
    | jq -r --arg marker "$MARKER" --arg run "run=${run_id}" \
      '.[] | select(.body | contains($marker) and contains($run)) | .id' \
    | head -n 1)" || true

  [ -n "$comment_id" ] || return 0

  if [ "$outcome" = "success" ]; then
    "$GH" api -X DELETE \
      "repos/${GITHUB_REPOSITORY}/issues/comments/${comment_id}" >/dev/null || true
  else
    fail_banner "$comment_id" "outcome: ${outcome} (runner-level)" \
      "$(date -u '+%Y-%m-%d %H:%M:%S UTC')" ""
  fi
}

case "${1:-}" in
  start) shift; start "$@" ;;
  watch) shift; watch "$@" ;;
  finish) shift; finish "$@" ;;
  finish-pr) shift; finish_pr "$@" ;;
  *)
    die "usage: review-liveness.sh {start|watch|finish|finish-pr} … (see header for arguments)"
    ;;
esac
