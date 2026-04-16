#!/usr/bin/env bash
# Poll PR #9 and PR #11 until both green, then merge sequence.
# Usage: bash poll-and-merge-prs.sh

set -euo pipefail

REPO_DIR="/Users/randlee/Documents/github/scmux"
MAX_ATTEMPTS=10
INTERVAL=60

log() { echo "[$(date '+%H:%M:%S')] $*"; }

checks_green() {
  local pr="$1"
  local output
  output=$(gh pr checks "$pr" --repo randlee/scmux 2>&1)
  echo "$output"
  # Green = no lines containing 'pending' or 'fail'
  if echo "$output" | grep -qiE '^\S.*\s+(pending|fail)\s'; then
    return 1
  fi
  return 0
}

log "Starting poll loop for PR #9 and PR #11 (max $MAX_ATTEMPTS attempts, every ${INTERVAL}s)"

attempt=0
both_green=false

while [ $attempt -lt $MAX_ATTEMPTS ]; do
  attempt=$((attempt + 1))
  log "=== Attempt $attempt/$MAX_ATTEMPTS ==="

  pr9_out=$(gh pr checks 9 --repo randlee/scmux 2>&1)
  pr11_out=$(gh pr checks 11 --repo randlee/scmux 2>&1)

  log "PR #9 checks:"
  echo "$pr9_out"
  log "PR #11 checks:"
  echo "$pr11_out"

  pr9_pending=$(echo "$pr9_out" | grep -iE '^\S.*\s+(pending|fail)\s' || true)
  pr11_pending=$(echo "$pr11_out" | grep -iE '^\S.*\s+(pending|fail)\s' || true)

  if [ -z "$pr9_pending" ] && [ -z "$pr11_pending" ]; then
    log "Both PR #9 and PR #11 are fully green!"
    both_green=true
    break
  else
    [ -n "$pr9_pending" ] && log "PR #9 still has pending/failed checks."
    [ -n "$pr11_pending" ] && log "PR #11 still has pending/failed checks."
  fi

  if [ $attempt -lt $MAX_ATTEMPTS ]; then
    log "Sleeping ${INTERVAL}s before next poll..."
    sleep $INTERVAL
  fi
done

if [ "$both_green" = false ]; then
  log "ERROR: PRs #9 and #11 did not go green within $MAX_ATTEMPTS attempts."
  exit 1
fi

# --- Merge sequence ---

log "Merging PR #11 (integrate/phase-2 fixes into integrate/phase-3) with squash..."
gh pr merge 11 --squash --repo randlee/scmux --yes 2>&1
log "PR #11 merged."

log "Merging PR #9 (S3.2 fixes into integrate/phase-3) with squash..."
gh pr merge 9 --squash --repo randlee/scmux --yes 2>&1
log "PR #9 merged."

log "Waiting 30s for CI to re-trigger on PR #8..."
sleep 30

log "Starting poll loop for PR #8 (max $MAX_ATTEMPTS attempts, every ${INTERVAL}s)"

attempt=0
pr8_green=false

while [ $attempt -lt $MAX_ATTEMPTS ]; do
  attempt=$((attempt + 1))
  log "=== PR #8 Attempt $attempt/$MAX_ATTEMPTS ==="

  pr8_out=$(gh pr checks 8 --repo randlee/scmux 2>&1)
  log "PR #8 checks:"
  echo "$pr8_out"

  pr8_pending=$(echo "$pr8_out" | grep -iE '^\S.*\s+(pending|fail)\s' || true)

  if [ -z "$pr8_pending" ]; then
    log "PR #8 is fully green!"
    pr8_green=true
    break
  else
    log "PR #8 still has pending/failed checks."
  fi

  if [ $attempt -lt $MAX_ATTEMPTS ]; then
    log "Sleeping ${INTERVAL}s before next poll..."
    sleep $INTERVAL
  fi
done

if [ "$pr8_green" = false ]; then
  log "ERROR: PR #8 did not go green within $MAX_ATTEMPTS attempts after merging #9 and #11."
  exit 2
fi

log "Merging PR #8 (integrate/phase-3 into develop) with squash..."
gh pr merge 8 --squash --repo randlee/scmux --yes 2>&1
log "PR #8 merged."

log "=== ALL DONE: PRs #9, #11, and #8 successfully merged ==="
