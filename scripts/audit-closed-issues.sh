#!/usr/bin/env bash
#
# #437: Audit closed issues for a corresponding merged PR
#
# For bounty accounting, "issue closed" is only meaningful if code actually
# landed for it. This cross-references every closed issue carrying the wave
# label against the merged pull requests that reference it, and reports any
# issue that was closed without a merged PR behind it (closed manually, closed
# by mistake, or closed prematurely).
#
# An issue counts as backed by code when either:
#   - a merged PR's body references it with a closing keyword
#     (Closes/Fixes/Resolves #N), the convention the issue template asks for; or
#   - the issue's timeline records it being closed by a merged PR.
#
# Usage:
#   ./scripts/audit-closed-issues.sh [--repo OWNER/NAME] [--label LABEL]
#
# Environment:
#   GH_TOKEN   GitHub token used by gh (or an existing `gh auth login` session)
#
# Exit codes:
#   0 - every closed issue has a matching merged PR
#   1 - one or more closed issues have no matching merged PR
#   2 - prerequisites missing (gh, jq, authentication)

set -euo pipefail

REPO=""
LABEL="Stellar Wave"

while [ $# -gt 0 ]; do
    case "$1" in
        --repo)
            REPO="$2"
            shift 2
            ;;
        --label)
            LABEL="$2"
            shift 2
            ;;
        -h | --help)
            sed -n '2,25p' "$0"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

for tool in gh jq; do
    if ! command -v "$tool" > /dev/null 2>&1; then
        echo "Error: $tool is required but not installed." >&2
        exit 2
    fi
done

if ! gh auth status > /dev/null 2>&1 && [ -z "${GH_TOKEN:-}" ]; then
    echo "Error: gh is not authenticated. Run 'gh auth login' or set GH_TOKEN." >&2
    exit 2
fi

if [ -z "$REPO" ]; then
    REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
fi

echo "=== Closed-issue audit ==="
echo "Repo:  $REPO"
echo "Label: $LABEL"
echo

# Closed issues carrying the label. The issues endpoint also returns pull
# requests, so anything with a .pull_request key is dropped.
closed_issues=$(
    gh api --paginate \
        -X GET "repos/$REPO/issues" \
        -f state=closed \
        -f labels="$LABEL" \
        -f per_page=100 \
        --jq '.[] | select(has("pull_request") | not) | .number'
)

if [ -z "$closed_issues" ]; then
    echo "No closed issues found with label '$LABEL'. Nothing to audit."
    exit 0
fi

# Every issue number referenced with a closing keyword by a merged PR. One pass
# over the merged PRs is far cheaper than a timeline query per issue, so this
# set answers most lookups without further API calls.
referenced=$(
    gh api --paginate \
        -X GET "repos/$REPO/pulls" \
        -f state=closed \
        -f per_page=100 \
        --jq '.[] | select(.merged_at != null) | .body // "" ' \
        | grep -oEi '(clos(e|es|ed)|fix(e|es|ed)?|resolv(e|es|ed))[[:space:]]+#[0-9]+' \
        | grep -oE '[0-9]+' \
        | sort -u
)

# Fallback for an issue not covered by the set above: GitHub records a
# "closed by PR" link in the timeline even when the PR body omits the keyword.
closed_by_merged_pr() {
    local issue="$1"
    gh api --paginate \
        -H "Accept: application/vnd.github+json" \
        "repos/$REPO/issues/$issue/timeline" \
        --jq '.[] | select(.event == "cross-referenced" or .event == "closed")
                  | .source.issue // empty
                  | select(.pull_request != null and .pull_request.merged_at != null)
                  | .number' 2> /dev/null | head -n 1
}

total=0
matched=0
unmatched=""

for issue in $closed_issues; do
    total=$((total + 1))
    if echo "$referenced" | grep -qx "$issue"; then
        matched=$((matched + 1))
        continue
    fi
    pr=$(closed_by_merged_pr "$issue")
    if [ -n "$pr" ]; then
        matched=$((matched + 1))
        continue
    fi
    unmatched="$unmatched $issue"
done

echo "Closed issues audited: $total"
echo "Backed by a merged PR: $matched"
echo

if [ -z "$unmatched" ]; then
    echo "OK: every closed '$LABEL' issue has a corresponding merged PR."
    exit 0
fi

echo "The following closed issues have no merged PR referencing them:"
for issue in $unmatched; do
    title=$(gh api "repos/$REPO/issues/$issue" --jq .title)
    echo "  #$issue  $title"
    echo "          https://github.com/$REPO/issues/$issue"
done
echo
echo "Each one was closed without code landing for it. Verify whether the work"
echo "shipped under a PR that omitted the closing keyword, or reopen the issue."
exit 1
