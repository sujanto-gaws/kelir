#!/usr/bin/env bash
#
# Validates a pull-request title against docs/standards/03. Commit Message
# Convention.md, **as the header it will become**.
#
# This repository squash-merges, so the commit that lands on main takes its
# header from the pull-request title — not from any commit on the branch, which
# is what check-commit-messages.sh reads. Git Workflow §4 already says the title
# governs; nothing checked it, and four of the eight headers on main at the time
# this was written were over the maximum, the longest at 117 (decision D-38).
#
# **The ` (#NNN)` suffix is the whole reason this is a separate script.** GitHub
# appends the pull-request number to the title when it squashes, so an author
# who spends the full 72 characters produces a header that is over it by the
# width of their own PR number. The budget checked here is the title plus that
# suffix, which is the string a reader of `git log main` will actually see.
#
# Usage: scripts/check-pull-request-title.sh <title> <pull-request-number>

set -euo pipefail

TITLE="${1?usage: check-pull-request-title.sh <title> <pull-request-number>}"
PR_NUMBER="${2:?usage: check-pull-request-title.sh <title> <pull-request-number>}"

# Types from the convention, section 2. Kept identical to
# check-commit-messages.sh deliberately: two lists that can drift are two rules.
TYPES='feat|fix|docs|refactor|perf|test|build|ci|chore|revert'
HEADER_PATTERN="^(${TYPES})(\([a-z0-9-]+\))?!?: [a-z].*$"
MAX_HEADER_LENGTH=72

SUFFIX=" (#${PR_NUMBER})"
HEADER="${TITLE}${SUFFIX}"

failures=0

fail() {
  echo "  FAIL  $1"
  shift
  for line in "$@"; do
    echo "        ${line}"
  done
  failures=$((failures + 1))
}

echo "Checking the pull-request title as the header it becomes"
echo "  title   ${TITLE}"
echo "  squash  ${HEADER}"
echo

if [[ ! "${TITLE}" =~ ${HEADER_PATTERN} ]]; then
  fail "the title is not '<type>(<scope>): <subject>'" \
    "types: ${TYPES//|/, }" \
    "and the subject starts lowercase"
fi

if (( ${#HEADER} > MAX_HEADER_LENGTH )); then
  fail "the squashed header is ${#HEADER} characters, maximum is ${MAX_HEADER_LENGTH}" \
    "the title is ${#TITLE} and GitHub appends '${SUFFIX}' (${#SUFFIX}) when it squashes" \
    "so the budget for the title itself is $(( MAX_HEADER_LENGTH - ${#SUFFIX} )) characters here"
fi

if [[ "${TITLE}" == *. ]]; then
  fail "the title ends with a period"
fi

if (( failures > 0 )); then
  echo
  echo "${failures} problem(s) with the pull-request title."
  echo "See docs/standards/03. Commit Message Convention.md §1."
  echo "Edit the title on the pull request and this job will re-run."
  exit 1
fi

echo "The pull-request title is a header that follows the convention."
