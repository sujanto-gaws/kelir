#!/usr/bin/env bash
#
# Validates commit message headers against docs/standards/03. Commit Message
# Convention.md. Checks what a regex can check: type, optional scope, the
# breaking-change marker, and the header rules (length, no trailing period,
# lowercase subject). Imperative mood is a review concern, not a CI one.
#
# **It also refuses a commit co-authored by Claude that does not say which
# session wrote it** (#475, decision D-84). Sprint plan §2 counts a reader as
# independent when their session appears on no commit in the range, and the
# only record of a commit's session is its `Claude-Session:` line. Record 16
# found 64 of 268 such commits without one, #459 among them, and a criterion
# cannot find an author who left nothing to match.
#
# Usage: scripts/check-commit-messages.sh <base-ref> [head-ref]

set -euo pipefail

# **Characters, not bytes.** `${#var}` counts bytes under a `C` locale, which is
# what a CI runner often has, so a header carrying an em-dash — and this project
# writes a lot of them — measures two longer there than on the author's machine.
# The convention says 72 *characters*, and a rule whose answer depends on where
# it is checked is not one.
#
# Forced rather than defaulted, because inheriting `LC_ALL=C` is the case this
# exists for. Where `C.UTF-8` is unavailable libc falls back to `C` and the
# count reverts to bytes, which is no worse than not setting it.
export LC_ALL=C.UTF-8

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

# **The line is looked for anywhere in the message, not through git's trailer
# parser.** GitHub's squash puts `Claude-Session:` in its own paragraph above
# `Co-authored-by:`, so `%(trailers)` reads none of them on `main`, even for a
# branch commit that had it in the trailer block (#471's did; its squash
# `c66453b` does not). A check that used the parser would pass nothing that
# lands.
#
# **Two forms, because there are two kinds of session.** A claude.ai session
# writes its URL. A local session has no URL, and its ID is the bare UUID in
# `CLAUDE_CODE_SESSION_ID`. Anything else after the key is refused, so an empty
# or placeholder value is not a way through.
#
# **Seen to fail, 2026-09-16** (coding standard §2.9). Ten scratch commits in a
# throwaway repository, the script run on each, positive controls last:
#
#   no session line; an empty value; `<your id>`; a truncated UUID; a session
#   URL on another host; `co-authored-by: claude` in lower case   → refused
#   CONTROL a UUID in the trailer block; the URL form; the line in a paragraph
#   above the co-author's; a commit with no co-author            → accepted
#
# Three mutations, each restored:
#
#   `check_session` never called            → the six refusals all accepted
#   SESSION_LINE reduced to `^Claude-Session:` → empty, placeholder, truncated
#                                              and foreign-host all accepted
#   the co-author match made case-sensitive  → the lower-case probe accepted
#
# **Against the commit that earned it:** `62f14c9` (#459) is refused, as are
# #460, #462 and #463. Sprint 18's construction, #450 to #458, passes.
SESSION_URL='https://claude\.ai/code/session_[A-Za-z0-9]+'
SESSION_UUID='[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
SESSION_LINE="^Claude-Session:[[:space:]]+(${SESSION_URL}|${SESSION_UUID})[[:space:]]*$"

check_session() {
  local sha="$1"
  local short="${sha:0:8}"
  local message
  message="$(git log -1 --format=%B "${sha}" | tr -d '\r')"

  # A human commit needs no line. Only a message that names Claude as a
  # co-author is governed, and the key's case is not a way around it.
  if ! grep -qiE '^co-authored-by:.*claude' <<< "${message}"; then
    return 0
  fi

  if grep -qE "${SESSION_LINE}" <<< "${message}"; then
    return 0
  fi

  if grep -qiE '^claude-session:' <<< "${message}"; then
    echo "  FAIL  ${short}  its Claude-Session line is not a session URL or UUID"
    echo "        $(grep -iE '^claude-session:' <<< "${message}" | head -1)"
  else
    echo "  FAIL  ${short}  co-authored by Claude with no Claude-Session line"
  fi
  echo "        add 'Claude-Session: <id>' to the message, where <id> is"
  echo "        https://claude.ai/code/session_… or \$CLAUDE_CODE_SESSION_ID"
  failures=$((failures + 1))
}

echo "Checking commit headers in ${BASE_REF}..${HEAD_REF}"

commits=$(git rev-list "${BASE_REF}..${HEAD_REF}")

if [[ -z "${commits}" ]]; then
  echo "No commits in range; nothing to check."
  exit 0
fi

while read -r sha; do
  header="$(git log -1 --format=%s "${sha}")"
  check_header "${sha}" "${header}"
  if [[ ! "${header}" =~ ^Merge\  ]]; then
    check_session "${sha}"
  fi
done <<< "${commits}"

if (( failures > 0 )); then
  echo
  echo "${failures} commit message(s) do not follow the convention."
  echo "See docs/standards/03. Commit Message Convention.md"
  exit 1
fi

echo
echo "All commit messages follow the convention."
