#!/usr/bin/env bash
#
# Probes `check-pull-request-ready.sh`: one pull-request body per shape, each
# expected to be refused or accepted (#696).
#
# Each shape below was seen red on 2026-10-09 against a copy of the check with
# one rule taken out, and green against the check as written: the rule named
# in a shape's comment is the one whose removal turned it red. **A probe that
# has stopped refusing turns the job red**, so a loosened rule cannot stay
# green.
#
# Usage: scripts/check-pull-request-ready.probe.sh

set -euo pipefail

CHECK="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-pull-request-ready.sh"

# The line a feature row's pull request carries (Git Workflow §4).
LINE="**Not ready: the test-engineer campaign is still running. Do not merge yet.**"
CR=$'\r'
TAB=$'\t'
BOM=$'\xef\xbb\xbf'

failures=0

# probe <refused|accepted> <name> <body>: the check with PR_BODY set to body.
probe() {
  local expected="$1" name="$2" body="$3"

  local actual=accepted
  if ! PR_BODY="${body}" "${CHECK}" > /dev/null; then
    actual=refused
  fi

  report "${expected}" "${actual}" "${name}"
}

report() {
  local expected="$1" actual="$2" name="$3"

  if [[ "${actual}" == "${expected}" ]]; then
    echo "  ok    ${expected}  ${name}"
  else
    echo "  FAIL  ${name}: expected ${expected}, was ${actual}"
    failures=$((failures + 1))
  fi
}

echo "Probing the first-line rule in ${CHECK##*/}"

# The line first, and the same body with it removed: the check's whole job.
# Seen red with the check made to pass every body, and to refuse every line.
probe refused "the line, first" "${LINE}

Closes #696"
probe accepted "the line removed" "Closes #696"

# Either phrase alone is the line. Seen red with each taken out of PHRASES.
probe refused "only 'do not merge yet'" "Waits on record 22. Do not merge yet."
probe refused "only 'not ready'" "Not ready: the browser flow is red."

# Case. Seen red without the lower-casing.
probe refused "upper case" "NOT READY: THE CAMPAIGN IS STILL RUNNING."
probe refused "title case" "Do Not Merge Yet"

# CRLF, which GitHub's web editor sends. Seen red without the carriage-return
# strip: a line holding only a carriage return counted as the first line.
probe refused "CRLF endings, the line first" "${LINE}${CR}
${CR}
Closes #696${CR}"
probe refused "CRLF endings, the line after a blank line" "${CR}
${LINE}${CR}
Closes #696${CR}"
probe accepted "CRLF endings, the line removed" "Closes #696${CR}
${CR}
## Summary${CR}"

# Bold and italic. Seen red without the `*` and `_` strip.
probe refused "bold around one word of the phrase" "Not **ready**: row 8's gate is open."
probe refused "italic inside the phrase" "Do not merge _yet_."

# Whitespace. Seen red without the blank-line skip, without the squeeze, and
# (the indented line) with the phrase anchored to the line's start.
probe refused "blank and whitespace-only lines before it" "

   ${TAB}
${LINE}"
probe refused "the line indented" "    ${LINE}"
probe refused "two spaces inside the phrase" "Not  ready."
probe refused "a tab inside the phrase" "Do not${TAB}merge yet."

# Markdown around it. Seen red with the phrase anchored to the line's start.
probe refused "a heading" "## Not ready"
probe refused "a quote" "> Not ready, the campaign is open."
probe refused "the phrase late in the line" "Row 8's campaign has three gaps open, so: do not merge yet."

# Punctuation hard against the phrase, with no space before it. Seen red with
# the boundary before the phrase narrowed to whitespace (test-engineer).
probe refused "a bracket against the phrase" "[Not ready] the campaign is open."

# A byte-order mark, which a body read from a file saved on Windows can open
# with if nothing on the way strips it. Seen red with the phrase anchored to
# the line's start (test-engineer). The body holds only the first phrase, so
# the second cannot satisfy the probe on the first's behalf.
probe refused "a byte-order mark before the line" "${BOM}Not ready: the campaign is open."

# A trailing backslash is a markdown hard break, not a line continuation: the
# line after it is the second line. Seen red with `read -r` loosened to `read`,
# which joins the two (test-engineer). The backslash and newline are the body.
# shellcheck disable=SC1004
probe accepted "a hard break before the line" 'Closes #696 \
Not ready: the campaign is open.'

# **The line not first passes, and that is the rule, not a gap.** The
# convention puts it first, where the merger reads it, and a body lower down
# quotes it, explains it, or records its removal. Reading the whole body would
# refuse every one of those. Seen red with the whole body matched.
probe accepted "the line not first" "Closes #696

${LINE}"
probe accepted "'not ready' mid-body" "Closes #696

## Summary

A body whose first line says not ready, or do not merge yet, is refused."

# Whole words. Seen red without the boundaries around the phrase.
probe accepted "a word that only contains the phrase" "The scanner cannot ready a file twice."
probe accepted "'ready' without 'not'" "Ready to merge: the campaign section is below."
# The boundary after the phrase. Seen red with it removed (test-engineer).
probe accepted "a word that starts with the phrase" "The form is not readymade: the builder assembles it."

# Nothing to read. Seen red with an empty first line refused.
probe accepted "an empty body" ""
probe accepted "a whitespace-only body" "  ${CR}
${TAB}${CR}
"

# **A misspelt variable must not read as an empty body.** Seen red with
# `${PR_BODY?}` loosened to `${PR_BODY:-}`.
actual=accepted
if ! env -u PR_BODY "${CHECK}" > /dev/null 2>&1; then
  actual=refused
fi
report refused "${actual}" "PR_BODY unset"

if (( failures > 0 )); then
  echo
  echo "${failures} probe(s) did not behave. The first-line rule has changed shape."
  exit 1
fi

echo
echo "Every probe behaved."
