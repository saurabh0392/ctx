#!/usr/bin/env bash
# Run Fitcheck locally for the exact committed PR head and publish the required commit status.
set -euo pipefail

usage() {
  echo "usage: scripts/pr-fitcheck.sh [PR number, URL, or branch]"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
if [[ $# -gt 1 || "${1:-}" == -* ]]; then
  usage >&2
  exit 2
fi

command -v gh >/dev/null 2>&1 || {
  echo "pr-fitcheck: GitHub CLI (gh) is required"
  exit 2
}
command -v git >/dev/null 2>&1 || {
  echo "pr-fitcheck: git is required"
  exit 2
}

REPO="$(git rev-parse --show-toplevel)"
cd "$REPO"

# Tracked modifications only. What must match the PR head is the committed content under review;
# untracked files sitting in the developer's tree (build residue, media from another branch) do not
# change it, and refusing to run because of them just invites parking files by hand before a gate.
if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  echo "pr-fitcheck: tracked files are modified; the review must match the committed PR head"
  git status --short --untracked-files=no
  exit 2
fi
# Snapshot what was already untracked. The check afterwards is "did the review add anything", not
# "is the tree pristine": residue that predates the run is not something the review did.
UNTRACKED_BEFORE="$(git status --porcelain --untracked-files=all | grep '^??' | sort || true)"

PR_REF="${1:-}"
PR_ARGS=()
[[ -n "$PR_REF" ]] && PR_ARGS=("$PR_REF")

PR_DATA="$(gh pr view "${PR_ARGS[@]}" \
  --json number,state,isDraft,headRefOid,headRefName,baseRefName,url \
  --template '{{.number}}|{{.state}}|{{.isDraft}}|{{.headRefOid}}|{{.headRefName}}|{{.baseRefName}}|{{.url}}')"
IFS='|' read -r PR_NUMBER PR_STATE PR_DRAFT PR_HEAD PR_BRANCH PR_BASE PR_URL <<< "$PR_DATA"

if [[ "$PR_STATE" != "OPEN" ]]; then
  echo "pr-fitcheck: PR #${PR_NUMBER} is ${PR_STATE}, not open"
  exit 2
fi

LOCAL_HEAD="$(git rev-parse HEAD)"
if [[ "$LOCAL_HEAD" != "$PR_HEAD" ]]; then
  echo "pr-fitcheck: local HEAD ${LOCAL_HEAD} does not match PR #${PR_NUMBER} head ${PR_HEAD}"
  echo "Check out and update ${PR_BRANCH}, then run the command again."
  exit 2
fi

REPO_SLUG="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
STATUS_CONTEXT="Local Fitcheck"

post_status() {
  local state="$1"
  local description="$2"
  gh api --method POST "repos/${REPO_SLUG}/statuses/${PR_HEAD}" \
    -f state="$state" \
    -f context="$STATUS_CONTEXT" \
    -f description="$description" >/dev/null
}

echo "pr-fitcheck: PR #${PR_NUMBER} ${PR_BRANCH} -> ${PR_BASE}"
[[ "$PR_DRAFT" == "true" ]] && echo "pr-fitcheck: note: PR is still a draft"
post_status pending "Running locally on PR #${PR_NUMBER}"

set +e
bash "$REPO/scripts/coherence/fitcheck-local.sh"
FITCHECK_CODE=$?
set -e

if [[ $FITCHECK_CODE -ne 0 ]]; then
  post_status failure "Local Fitcheck failed for PR #${PR_NUMBER}"
  echo "pr-fitcheck: FAILED; merge remains blocked"
  exit "$FITCHECK_CODE"
fi

# The invariant that matters: the review must not touch the code it is judging. Its own report under
# docs/fitcheck/ is the paper trail the skill exists to produce, so that one path is allowed.
UNTRACKED_AFTER="$(git status --porcelain --untracked-files=all | grep '^??' | sort || true)"
ADDED="$(comm -13 <(printf '%s\n' "$UNTRACKED_BEFORE") <(printf '%s\n' "$UNTRACKED_AFTER") || true)"
STRAY="$(git status --porcelain --untracked-files=no; printf '%s\n' "$ADDED" | grep '^??' | grep -v '^?? docs/fitcheck/' || true)"
if [[ -n "$STRAY" ]]; then
  post_status failure "Local Fitcheck modified the worktree for PR #${PR_NUMBER}"
  echo "pr-fitcheck: FAILED; Fitcheck must not change anything outside docs/fitcheck/"
  echo "$STRAY"
  exit 2
fi
NEW_REPORT="$(printf '%s\n' "$ADDED" | grep '^?? docs/fitcheck/' | sed 's/^?? //' || true)"
[[ -n "$NEW_REPORT" ]] && echo "pr-fitcheck: review saved its report to ${NEW_REPORT} (untracked; commit it to keep the trail)"

LATEST_HEAD="$(gh pr view "$PR_NUMBER" --json headRefOid --jq '.headRefOid')"
if [[ "$LATEST_HEAD" != "$PR_HEAD" ]]; then
  post_status error "PR head changed during Local Fitcheck"
  echo "pr-fitcheck: PR head changed during the review; rerun on ${LATEST_HEAD}"
  exit 2
fi

post_status success "Passed locally on $(git rev-parse --short HEAD)"
echo "pr-fitcheck: PASS posted for ${PR_URL}"
