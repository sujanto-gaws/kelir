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

# probe <refused|accepted> <name> <body> [title]: one commit on BASE, then the
# check.
probe() {
  local expected="$1" name="$2" body="$3" title="${4:-test: probe}"

  git -C "${REPO}" checkout -q --detach "${BASE}"
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
    commit -q --allow-empty -F - <<< "${title}

${body}"

  judge "${expected}" "${name}"
}

# probe_merge <refused|accepted> <name> <title> <body>: a commit of two parents
# on BASE, then the check.
probe_merge() {
  local expected="$1" name="$2" title="$3" body="$4"
  local side

  git -C "${REPO}" checkout -q --detach "${BASE}"
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
    commit -q --allow-empty -m "chore: side"
  side="$(git -C "${REPO}" rev-parse HEAD)"
  git -C "${REPO}" checkout -q --detach "${BASE}"
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
    commit -q --allow-empty -m "chore: main"
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
    merge -q --no-ff -m "${title}

${body}" "${side}"

  judge "${expected}" "${name}"
}

# probe_octopus <name> <title>: a commit of three parents on BASE, then the
# check, which should not read it.
probe_octopus() {
  local name="$1" title="$2"
  local tree first second third merge

  tree="$(git -C "${REPO}" rev-parse "${BASE}^{tree}")"
  first="$(commit_tree "${tree}" -p "${BASE}" -m "chore: first")"
  second="$(commit_tree "${tree}" -p "${BASE}" -m "chore: second")"
  third="$(commit_tree "${tree}" -p "${BASE}" -m "chore: third")"
  merge="$(commit_tree "${tree}" -p "${first}" -p "${second}" -p "${third}" -m "${title}")"
  git -C "${REPO}" checkout -q --detach "${merge}"

  judge accepted "${name}"
}

commit_tree() {
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com commit-tree "$@"
}

# probe_says <name> <body> <text>: refused, and the log holds the text.
probe_says() {
  local name="$1" body="$2" text="$3"
  local output actual=accepted

  git -C "${REPO}" checkout -q --detach "${BASE}"
  git -C "${REPO}" -c user.name=probe -c user.email=probe@example.com \
    commit -q --allow-empty -F - <<< "test: probe

${body}"

  if ! output="$(cd "${REPO}" && "${CHECK}" "${BASE}" HEAD)"; then
    actual=refused
  fi
  if [[ "${actual}" == refused ]] && ! grep -qF -- "${text}" <<< "${output}"; then
    actual="refused without '${text}'"
  fi

  if [[ "${actual}" == refused ]]; then
    echo "  ok    refused  ${name}"
  else
    echo "  FAIL  ${name}: expected refused, was ${actual}"
    failures=$((failures + 1))
  fi
}

# judge <refused|accepted> <name>: the check over what HEAD holds.
judge() {
  local expected="$1" name="$2"

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

# The Sprint 20 mutation campaign: each shape below held nothing, so a
# loosened rule stayed green. Seen red, 2026-09-25: the fence regex without
# its leading [[:space:]]* (the indented fence's line counted), the co-author
# pattern without its ^ anchor (the sentence was governed), and the address
# loosened to `anthropic` (the fan site was governed).
probe refused "another session's line inside an indented fence" "- the range record 17 read:
  ${FENCE}text
Claude-Session: ${UUID}
  ${FENCE}

${CLAUDE}"

# **A fence never closed hides only what is not the trailer block** (#557,
# record 19's P15). Each was accepted before this change. Seen red with the
# held lines never printed, and the next with them held past a blank line.
probe refused "the co-author inside a fence never closed" "${FENCE}text
${CLAUDE}"
probe refused "a fence opened in a list, never closed, the co-author after" "- the range:
  ${FENCE}text
  quoted

${CLAUDE}"
probe refused "a blank line closing the message after a fence never closed" "${FENCE}text
${CLAUDE}
"
# What the fence still hides, though nothing closes it: a line quoted in an
# earlier paragraph. Seen red with a fence never closed taken for no fence.
probe refused "a session line quoted before the trailers, fence never closed" "${FENCE}text
Claude-Session: ${UUID}

${CLAUDE}"
probe accepted "a human commit quoting a trailer, fence never closed" "${FENCE}text
${CLAUDE}

Written by hand."

# **A merge is a commit of two parents, not a title** (#557). Each `Merge …`
# below was accepted before this change. Seen red with the skip keyed on the
# title again.
probe refused "one parent, a Merge title, the co-author, no session line" \
  "${CLAUDE}" "Merge the outbox work"
probe refused "one parent, a Merge title, and no co-author" \
  "Written by hand." "Merge the outbox work"
probe_merge accepted "two parents, a Merge title, the co-author, no session line" \
  "Merge branch 'main' into work" "${CLAUDE}"
probe_merge accepted "two parents and another title" "Bring main in" "Written by hand."

# **A commit's body closes issues too** (#730): `Closes #1, #2` closes #1 alone,
# as it does in a pull-request body. Seen red with the closing-keyword check
# not called; the wrap against the check before it joined lines.
probe refused "a list in a commit body" "Closes #1, #2"
probe refused "a list wrapped across a commit body" "Closes #1,
#2"
probe refused "a list on the commit's first line" "" "fix: closes #1, #2"
probe accepted "one keyword per line in a commit body" "Closes #1
Closes #2"
probe accepted "a list quoted in a commit body's fence" "Refused:

${FENCE}
Closes #1, #2
${FENCE}"
probe_merge accepted "two parents and a list in the body" "Merge branch 'main' into work" \
  "Closes #1, #2"

# **The closing gate's mutation campaign on row 16.** Each shape held nothing,
# so the mutation named stayed green: seen red under it, in a copy of the check.
#
# Each fence starts with nothing held, so a line a closed fence hid is not
# revealed by one left open after it. Seen red with `held` kept at a fence line.
probe refused "a session line in a closed fence, then a fence never closed" "${CLAUDE}

${FENCE}text
Claude-Session: ${UUID}
${FENCE}

${FENCE}text
quoted"
# More than one parent, not exactly two. Seen red with the skip narrowed to two.
probe_octopus "three parents and another title" "Bring three lines in"
# The commit's refusal says what to write. Seen red with the report's lines not
# printed, which left a failure naming nothing.
probe_says "the log names what will not close" "Closes #1, #2" "write   Closes #2"

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
probe accepted "the commit's own trailers after a fence never closed" "${FENCE}text
quoted

Claude-Session: ${UUID}
${CLAUDE}"
probe accepted "its own trailers inside a fence never closed" "${FENCE}text
Claude-Session: ${UUID}
${CLAUDE}"
probe accepted "a human commit quoting a Claude trailer in a fence" "The trailer looks like this:

~~~
${CLAUDE}
~~~"
probe accepted "a human commit naming the trailer mid-sentence" \
  "Whether Co-authored-by: Claude belongs on a human commit was asked and answered."
probe accepted "a human co-author whose address only contains anthropic" \
  "Co-authored-by: Jane Doe <jane@anthropicfans.org>"

if (( failures > 0 )); then
  echo
  echo "${failures} probe(s) did not behave. The session rule has changed shape."
  exit 1
fi

echo
echo "Every probe behaved."
