#!/usr/bin/env bash
# Publishes the tail of a failed CI log as a pull-request comment.
#
# The build logs of this repository are not reachable from the development
# environment, so the workflow relays them into the pull request. The comment is
# updated in place (matched by a hidden marker) to keep the thread readable.
set -uo pipefail

log_file="${1:?usage: ci-report.sh <log-file> <label>}"
label="${2:?usage: ci-report.sh <log-file> <label>}"

sha="${GITHUB_SHA:-local}"
marker="<!-- tzim-ci:${label}:${sha:0:7} -->"

if [ ! -f "$log_file" ]; then
    echo "no log file at $log_file; nothing to report"
    exit 0
fi

body_file="$(mktemp)"
{
    echo "$marker"
    echo "### CI failure: \`${label}\`"
    echo
    echo '```'
    # Bounded: the last 150 lines, each capped, so the comment stays readable.
    tail -n 150 "$log_file" | cut -c1-2000
    echo '```'
} > "$body_file"

branch="${GITHUB_HEAD_REF:-$GITHUB_REF_NAME}"
pr="$(gh pr list --head "$branch" --state open --json number --jq '.[0].number // empty' 2>/dev/null || true)"

if [ -z "${pr:-}" ]; then
    echo "no open pull request for branch '$branch'; printing the tail instead"
    tail -n 80 "$log_file"
    exit 0
fi

existing="$(gh api "repos/${GITHUB_REPOSITORY}/issues/${pr}/comments" --paginate \
    --jq ".[] | select(.body | startswith(\"${marker}\")) | .id" 2>/dev/null | head -n 1 || true)"

if [ -n "${existing:-}" ]; then
    gh api -X PATCH "repos/${GITHUB_REPOSITORY}/issues/comments/${existing}" -F "body=@${body_file}" >/dev/null
    echo "updated comment ${existing} on PR ${pr}"
else
    gh api -X POST "repos/${GITHUB_REPOSITORY}/issues/${pr}/comments" -F "body=@${body_file}" >/dev/null
    echo "created a comment on PR ${pr}"
fi
