#!/usr/bin/env bash
#
# Probes `check-commit-messages.sh`'s session rule: one scratch commit per
# shape, in a throwaway repository, each expected to be refused or accepted.
#
# #476 ran these probes by hand and recorded them in a comment. Record 17
# finding 4 (#485) then found four more shapes the rule accepted, and a
# comment could not show them failing. **Here each shape is a case that runs
# on every pull request.** A probe that has stopped refusing turns the job red.
#
# Usage: scripts/check-commit-messages.probe.sh

set -euo pipefail

CHECK="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-commit-messages.sh"
REPO="$(mktemp -d)"
trap 'rm -rf "${REPO}"' EXIT

git -C "${REPO}" init -q
git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
  commit -q --allow-empty -m "chore: base"
BASE="$(git -C "${REPO}" rev-parse HEAD)"

UUID='1d834a2a-7eed-483a-84d2-149758e94129'
CLAUDE='Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>'
FENCE='```'

failures=0

# probe <refused|accepted> <name> <body>: one commit on BASE, then the check.
probe() {
  local expected="$1" name="$2" body="$3"

  git -C "${REPO}" checkout -q --detach "${BASE}"
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
    commit -q --allow-empty -F - <<< "test: probe

${body}"

  local actual=accepted
  if ! (cd "${REPO}" && "${CHECK}" "${BASE}" HEAD > /dev/null); then
    actual=refused
  fi

  if [[ "${actual}" == "${expected}" ]]; then
    echo "  ok    ${expected}  ${name}"
  else
    echo "  FAIL  ${name}: expected ${expected}, was ${actual}"
    failures=$((failures + 1))
  fi
}

echo "Probing the session rule in ${CHECK##*/}"

# #476's refusals.
probe refused "no session line" "${CLAUDE}"
probe refused "an empty value" "Claude-Session:
${CLAUDE}"
probe refused "a placeholder" "Claude-Session: <your id>
${CLAUDE}"
probe refused "a truncated UUID" "Claude-Session: 1d834a2a-7eed-483a
${CLAUDE}"
probe refused "a session URL on another host" "Claude-Session: https://example.com/code/session_01Pk4iAR9KWF38Jq2z9ECDqY
${CLAUDE}"
probe refused "the co-author key in lower case" "co-authored-by: claude <noreply@anthropic.com>"

# Record 17 finding 4 (#485): each was accepted before this change.
probe refused "an Anthropic address without the word claude" \
  "Co-Authored-By: Opus 5 (1M context) <noreply@anthropic.com>"
probe refused "another Anthropic address" \
  "Co-authored-by: Anthropic Assistant <assistant@anthropic.com>"
probe refused "a space before the key's colon" \
  "Co-Authored-By : Claude <noreply@anthropic.com>"
probe refused "another session's line quoted in a fence" "Record 17 read this range:

${FENCE}text
Claude-Session: ${UUID}
${FENCE}

${CLAUDE}"
probe refused "the nil UUID" "Claude-Session: 00000000-0000-0000-0000-000000000000
${CLAUDE}"

# Controls, last: each shape above has a correct neighbour that still passes.
probe accepted "a UUID in the trailer block" "Claude-Session: ${UUID}
${CLAUDE}"
probe accepted "the URL form" "Claude-Session: https://claude.ai/code/session_01Pk4iAR9KWF38Jq2z9ECDqY
${CLAUDE}"
probe accepted "the line in a paragraph above the co-author" "Claude-Session: ${UUID}

${CLAUDE}"
probe accepted "a quoted line plus the commit's own" "${FENCE}text
Claude-Session: 00000000-0000-0000-0000-000000000000
${FENCE}

Claude-Session: ${UUID}
${CLAUDE}"
probe accepted "a human commit with no co-author" "Written by hand."
probe accepted "a human commit quoting a Claude trailer in a fence" "The trailer looks like this:

~~~
${CLAUDE}
~~~"

if (( failures > 0 )); then
  echo
  echo "${failures} probe(s) did not behave. The session rule has changed shape."
  exit 1
fi

echo
echo "Every probe behaved."
