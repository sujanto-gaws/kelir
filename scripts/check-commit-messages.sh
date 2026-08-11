#!/usr/bin/env bash
#
# Validates commit message headers against docs/standards/03. Commit Message
# Convention.md. Checks what a regex can check: type, optional scope, the
# breaking-change marker, and the header rules (length, no trailing period,
# lowercase subject). Imperative mood is a review concern, not a CI one.
#
# Usage: scripts/check-commit-messages.sh <base-ref> [head-ref]

set -euo pipefail

BASE_REF="${1:?usage: check-commit-messages.sh <base-ref> [head-ref]}"
HEAD_REF="${2:-HEAD}"

# Types from the convention, section 2.
TYPES='feat|fix|docs|refactor|perf|test|build|ci|chore|revert'
HEADER_PATTERN="^(${TYPES})(\([a-z0-9-]+\))?!?: [a-z].*$"
MAX_HEADER_LENGTH=72

failures=0

check_header() {
  local sha="$1" header="$2"
  local short="${sha:0:8}"

  if [[ "${header}" =~ ^Merge\  ]]; then
    echo "  skip  ${short}  merge commit"
    return 0
  fi

  if [[ ! "${header}" =~ ${HEADER_PATTERN} ]]; then
    echo "  FAIL  ${short}  ${header}"
    echo "        expected '<type>(<scope>): <subject>' with type one of: ${TYPES//|/, }"
    echo "        and a lowercase subject"
    failures=$((failures + 1))
    return 0
  fi

  if (( ${#header} > MAX_HEADER_LENGTH )); then
    echo "  FAIL  ${short}  header is ${#header} characters, maximum is ${MAX_HEADER_LENGTH}"
    echo "        ${header}"
    failures=$((failures + 1))
    return 0
  fi

  if [[ "${header}" == *. ]]; then
    echo "  FAIL  ${short}  header ends with a period"
    echo "        ${header}"
    failures=$((failures + 1))
    return 0
  fi

  echo "  ok    ${short}  ${header}"
}

echo "Checking commit headers in ${BASE_REF}..${HEAD_REF}"

commits=$(git rev-list "${BASE_REF}..${HEAD_REF}")

if [[ -z "${commits}" ]]; then
  echo "No commits in range; nothing to check."
  exit 0
fi

while read -r sha; do
  check_header "${sha}" "$(git log -1 --format=%s "${sha}")"
done <<< "${commits}"

if (( failures > 0 )); then
  echo
  echo "${failures} commit message(s) do not follow the convention."
  echo "See docs/standards/03. Commit Message Convention.md"
  exit 1
fi

echo
echo "All commit messages follow the convention."
