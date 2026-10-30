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
# **It also runs `check-closing-keywords.sh` over each commit's message** (#730).
# A commit's body closes issues on its own, as #707's squash commit closed #673,
# and of a list after one keyword it closes only the first.
#
# **A merge is a commit of two parents, and nothing here reads one** (#557). The
# title is not what makes it one: a commit of one parent titled `Merge the
# outbox work` used to skip every check.
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

SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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
#
# **Since #485 those probes are a script**, `check-commit-messages.probe.sh`,
# with record 17 finding 4's five shapes added. CI runs it on every pull
# request. Four mutations, run 2026-09-18, each red on its own probe and then
# restored:
#
#   `unfenced` never applied       → the fenced line accepted, and the human
#                                    commit quoting a trailer refused
#   the co-author match back to `claude` alone → both Anthropic addresses accepted
#   the key's `[[:space:]]*` removed → the spaced key accepted
#   the nil-UUID filter removed    → the nil UUID accepted
#
# The script as it was before #485 fails the same five refusals, and it fails
# the fenced human-commit control too.
SESSION_URL='https://claude\.ai/code/session_[A-Za-z0-9]+'
SESSION_UUID='[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
SESSION_LINE="^Claude-Session:[[:space:]]+(${SESSION_URL}|${SESSION_UUID})[[:space:]]*$"
NIL_SESSION='^Claude-Session:[[:space:]]+0{8}-0{4}-0{4}-0{4}-0{12}[[:space:]]*$'

# **Who counts as Claude** (#485, record 17 finding 4). The name is not
# enough: `Opus 5 <noreply@anthropic.com>` never says `claude`, so an Anthropic
# address counts too. **The key is matched the way git parses it**, and git
# reads `Co-Authored-By : …`, with a space before the colon, as a trailer.
CO_AUTHOR='^co-authored-by[[:space:]]*:.*(claude|@anthropic\.com)'

# A message with its fenced blocks removed. **A line quoted in a fence is not
# the commit's own line**: a body that quotes another session's
# `Claude-Session:` in a code block would otherwise pass for that session
# (#485). The co-author is looked for the same way, so a human commit that
# quotes a Claude trailer in a fence stays a human commit. A fence opens and
# closes on a line starting with ``` or ~~~, and the two are not told apart.
#
# **A fence never closed keeps the message's last paragraph out of it** (#557).
# That paragraph is the block git reads trailers from, and git knows nothing of
# fences: `git interpret-trailers --parse` printed the co-author this function
# had dropped, so a Claude commit passed for a human one. The paragraphs before
# it stay hidden, so a session line quoted there is still not the commit's own.
# `held` is the paragraph being read, and a trailing blank line does not end it.
unfenced() {
  awk '
    /^[[:space:]]*(```|~~~)/ { fenced = !fenced; held = ""; gap = 0; next }
    !fenced { print; next }
    /^[[:space:]]*$/ { gap = 1; next }
    { if (gap) { held = ""; gap = 0 } held = held $0 "\n" }
    END { if (fenced) printf "%s", held }
  '
}

check_session() {
  local sha="$1"
  local short="${sha:0:8}"
  local message
  message="$(git log -1 --format=%B "${sha}" | tr -d '\r' | unfenced)"

  # A human commit needs no line. Only a message that names Claude or an
  # Anthropic address as a co-author is governed, and neither the key's case
  # nor a space before its colon is a way around it.
  if ! grep -qiE "${CO_AUTHOR}" <<< "${message}"; then
    return 0
  fi

  # **The nil UUID is a placeholder, not a session**: it is well-formed, so the
  # shape check alone accepted it.
  if grep -E "${SESSION_LINE}" <<< "${message}" | grep -qvE "${NIL_SESSION}"; then
    return 0
  fi

  if grep -qiE '^claude-session' <<< "${message}"; then
    echo "  FAIL  ${short}  its Claude-Session line is not a session URL or UUID"
    echo "        $(grep -iE '^claude-session' <<< "${message}" | head -1)"
  else
    echo "  FAIL  ${short}  co-authored by Claude with no Claude-Session line"
    echo "        (a line inside a fenced block is quoted, and does not count)"
  fi
  echo "        add 'Claude-Session: <id>' to the message, where <id> is"
  echo "        https://claude.ai/code/session_… or \$CLAUDE_CODE_SESSION_ID"
  failures=$((failures + 1))
}

# The whole message, title too, since a keyword closes wherever it is. Only the
# report's own lines are printed: the rest of that script's output speaks of a
# pull-request body, and each of these is indented, so no line of the author's
# text starts a line of the log.
check_closing_keywords() {
  local sha="$1"
  local short="${sha:0:8}"
  local report

  if report="$(PR_BODY="$(git log -1 --format=%B "${sha}")" \
    "${SCRIPTS}/check-closing-keywords.sh" 2>&1)"; then
    return 0
  fi

  echo "  FAIL  ${short}  a closing keyword followed by a list closes only the first"
  grep -E '^  (line|FAIL|write) ' <<< "${report}" | sed 's/^/      /' || true
  echo "        give each issue its own keyword, one per line;"
  echo "        see docs/standards/05. Git Workflow.md §4"
  failures=$((failures + 1))
}

echo "Checking commit headers in ${BASE_REF}..${HEAD_REF}"

commits=$(git rev-list --parents "${BASE_REF}..${HEAD_REF}")

if [[ -z "${commits}" ]]; then
  echo "No commits in range; nothing to check."
  exit 0
fi

while read -r sha parents; do
  if [[ "${parents}" == *' '* ]]; then
    echo "  skip  ${sha:0:8}  merge commit"
    continue
  fi
  header="$(git log -1 --format=%s "${sha}")"
  check_header "${sha}" "${header}"
  check_session "${sha}"
  check_closing_keywords "${sha}"
done <<< "${commits}"

if (( failures > 0 )); then
  echo
  echo "${failures} commit message(s) do not follow the convention."
  echo "See docs/standards/03. Commit Message Convention.md"
  exit 1
fi

echo
echo "All commit messages follow the convention."
