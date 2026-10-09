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
