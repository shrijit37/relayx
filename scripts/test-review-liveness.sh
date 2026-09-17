#!/usr/bin/env bash
#
# Test review-liveness.sh against a mocked `gh` executable. Runs the script's
# subcommands with GH_BIN pointing at a fake gh that logs every invocation and
# returns canned responses, then asserts on the API calls and comment bodies.
#
# Usage:
#   scripts/test-review-liveness.sh          # run the tests
#   scripts/test-review-liveness.sh -v       # verbose
#
# Requires: bash, a POSIX environment, and the real jq (the script needs it;
# the fake gh returns raw JSON for the script to parse).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="bash $ROOT/scripts/review-liveness.sh"
VERBOSE=0
[ "${1:-}" = "-v" ] && VERBOSE=1

# Sandbox: a temp gh "bin" + git repo (the script only uses GITHUB_REPOSITORY,
# GH_BIN and the API, but run it from a throwaway cwd so it cannot touch the
# repo).
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0
FAIL=0

note() { [ "$VERBOSE" = "1" ] && printf '  %s\n' "$*"; }

fail() {
  printf 'FAIL: %s\n' "$*"
  FAIL=$((FAIL + 1))
}

pass() {
  PASS=$((PASS + 1))
  note "ok: $*"
}

# Write a fake gh script that logs calls to a transcript file and returns a
# canned JSON response depending on the URL it is called with. The real gh CLI
# is invoked as `gh --repo <repo> api <url> ...` (or `gh api <url> ...`), so
# the URL is the first argument starting with `repos/`; the fake finds it by
# scanning, not by position.
make_fake_gh() {
  local log="$1" list_json="$2" create_json="$3"
  cat > "$WORK/gh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
# log: <args...> (embedded newlines preserved)
printf '%s\n' "\$*" >> "$log"
url=""
for a in "\$@"; do
  case "\$a" in repos/*) url="\$a"; break ;; esac
done
case "\$url" in
  */issues/*/comments?per_page=100*)
    # list comments -> return the canned list
    cat <<'JSON'
$list_json
JSON
    ;;
  */issues/*/comments)
    # create comment -> return the created comment id
    cat <<'JSON'
$create_json
JSON
    ;;
esac
EOF
  chmod +x "$WORK/gh"
}

# Shared: fake gh that always succeeds.
make_fake_gh_success() {
  local log="$1"
  make_fake_gh "$log" \
    '[]' \
    '{"id": 42, "body": "<created>"}'
}

# Test 1 — start posts a comment with the marker and run tag.
test_start() {
  note "-- start: posts a fresh liveness comment"
  local log="$WORK/start.log"
  make_fake_gh_success "$log"

  local out
  out="$(cd "$WORK" \
    && GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" $SCRIPT start 123 456)" \
    || fail "start exited non-zero"

  # Output must be the comment id.
  [ "$out" = "42" ] || fail "start printed '$out', want 42"

  # The create call must carry the marker + run tag and the right PR.
  grep -q -- "repos/acme/relayx/issues/123/comments" "$log" \
    || fail "start did not POST to the PR comments endpoint"
  grep -q -- "body=<!-- ocr-liveness -->" "$log" \
    || fail "start comment body missing the liveness marker"
  grep -q -- "run=456" "$log" \
    || fail "start comment body missing the run tag"
  pass "start posts marker+run-tagged comment and prints id"
}

# Test 2 — start deletes stale liveness comments from prior runs.
test_start_cleans_stale() {
  note "-- start: deletes stale liveness comments"
  local log="$WORK/clean.log"
  # List returns two stale marker comments; create returns a fresh id.
  make_fake_gh "$log" \
    '[{"id": 7, "body": "<!-- ocr-liveness --> stale"}, {"id": 9, "body": "<!-- ocr-liveness --> older"}, {"id": 11, "body": "plain human comment"}]' \
    '{"id": 42, "body": "<created>"}'

  (cd "$WORK" \
    && GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" $SCRIPT start 123 456 >/dev/null) \
    || fail "start exited non-zero"

  grep -q -- "-X DELETE repos/acme/relayx/issues/comments/7" "$log" \
    || fail "start did not delete stale comment 7"
  grep -q -- "-X DELETE repos/acme/relayx/issues/comments/9" "$log" \
    || fail "start did not delete stale comment 9"
  grep -q -- "-X DELETE repos/acme/relayx/issues/comments/11" "$log" \
    && fail "start deleted a non-marker human comment" || true
  pass "start cleans only marker comments"
}

# Test 3 — finish success deletes the comment.
test_finish_success() {
  note "-- finish success: deletes the comment"
  local log="$WORK/finish-ok.log"
  # finish never calls the list endpoint; any fake gh suffices.
  make_fake_gh_success "$log"
  (cd "$WORK" \
    && GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" $SCRIPT finish 123 99 success) \
    || fail "finish success exited non-zero"
  grep -q -- "-X DELETE repos/acme/relayx/issues/comments/99" "$log" \
    || fail "finish success did not delete the comment"
  pass "finish success deletes the liveness comment"
}

# Test 4 — finish failure keeps a red banner.
test_finish_failure() {
  note "-- finish failure: rewrites to a red banner"
  local log="$WORK/finish-fail.log"
  make_fake_gh_success "$log"
  (cd "$WORK" \
    && GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" $SCRIPT finish 123 99 failure) \
    || fail "finish failure exited non-zero"
  grep -q -- "-X PATCH repos/acme/relayx/issues/comments/99" "$log" \
    || fail "finish failure did not PATCH the comment"
  grep -Fq "body=" "$log" && grep -Fq "AI review stopped" "$log" \
    || fail "finish failure body is not a red banner"
  grep -Fq "<!-- ocr-liveness -->" "$log" \
    || fail "finish failure body dropped the marker"
  pass "finish failure leaves a red banner"
}

# Test 5 — finish-pr (cleanup workflow) finds the comment by run tag.
test_finish_pr() {
  note "-- finish-pr: finds and deletes by run tag"
  local log="$WORK/finish-pr.log"
  # List returns our run's marker comment (run=456) plus another run's.
  make_fake_gh "$log" \
    '[{"id": 7, "body": "<!-- ocr-liveness --><!-- ocr-liveness run=456 --> 🔄"}, {"id": 8, "body": "<!-- ocr-liveness --><!-- ocr-liveness run=999 --> 🔄"}]' \
    '{"id": 9, "body": "<created>"}'

  (cd "$WORK" \
    && GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" $SCRIPT finish-pr 123 success 456) \
    || fail "finish-pr success exited non-zero"

  grep -q -- "-X DELETE repos/acme/relayx/issues/comments/7" "$log" \
    || fail "finish-pr did not delete the run=456 comment"
  grep -q -- "-X DELETE repos/acme/relayx/issues/comments/8" "$log" \
    && fail "finish-pr deleted another run's comment" || true
  pass "finish-pr targets the matching run's comment"
}

# Test 6 — watch: PATCHes the comment with last-5-files and heartbeat.
test_watch() {
  note "-- watch: updates comment with progress + heartbeat"
  local log="$WORK/watch.log"
  make_fake_gh_success "$log"

  # Feed a synthetic OCR stderr log, then let the watcher see it grow to EOF.
  local logfile="$WORK/ocr-stderr.log"
  printf 'pre\n' > "$logfile"
  {
    printf '[ocr]   ▶ apps/gateway/src/main.rs\n'
    printf '\033[32m  %s\033[0m [ocr] ▶ crates/protocol-core/src/adapter.rs\n'
    printf '[ocr]   ▶ apps/web/src/editor.tsx\n'
    printf '[ocr]   ▶ crates/workflow-runtime/src/exec.rs\n'
    printf '[ocr]   ▶ docs/architecture.md\n'
    printf '[ocr]   ▶ apps/control-plane/src/db.ts  \n'
    printf '[ocr]   ▶ crates/mock-upstream/src/lib.rs\n'
    printf 'ignored line with no file\n'
  } >> "$logfile"

  # Run watch with a short poll so it exits after upstream-idle.
  (REVIEW_LIVENESS_POLL_SECONDS=1 REVIEW_LIVENESS_IDLE_EXIT=2 \
    GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" \
    $SCRIPT watch 123 99 456 "$logfile") \
    || fail "watch exited non-zero"

  # At least one PATCH must have happened.
  grep -q -- "-X PATCH repos/acme/relayx/issues/comments/99" "$log" \
    || fail "watch never PATChed the comment"

  # Body must carry the marker, run tag, and the last 5 files (the most
  # recent 5 of the 7 file lines, deduped to the tail).
  # The body is embedded in the transcript with literal newlines, so grep
  # matches across them. The `-f` literal match avoids regex issues with `*`.
  grep -Fq "run=456" "$log" || fail "watch body missing run tag"
  grep -Fq "docs/architecture.md" "$log" || fail "watch body missing recent file"
  grep -Fq "Last activity" "$log" || fail "watch body missing heartbeat"
  grep -Fq "<details>" "$log" || fail "watch body missing collapsible details block"
  pass "watch posts last-5-files + heartbeat updates"
}

# Test 7 — die on missing PR number.
test_requires_pr() {
  note "-- start: errors on missing PR number"
  (cd "$WORK" \
    && GITHUB_REPOSITORY="acme/relayx" GH_BIN="$WORK/gh" $SCRIPT start "" 456 >/dev/null 2>&1) \
    && fail "start with empty PR number should fail" || true
  pass "start rejects an empty PR number"
}

test_start
test_start_cleans_stale
test_finish_success
test_finish_failure
test_finish_pr
test_watch
test_requires_pr

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
