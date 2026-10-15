#!/usr/bin/env bash
#
# Probes `check-closing-keywords.sh`: one pull-request body per shape, each
# expected to be refused or accepted (#725).
#
# Each shape below was seen red on 2026-10-15 against a copy of the check with
# one rule taken out or loosened, and green against the check as written: the
# rule named in a shape's comment is the one whose change turned it red. **A
# probe that has stopped refusing turns the job red**, so a loosened rule
# cannot stay green.
#
# Usage: scripts/check-closing-keywords.probe.sh

set -euo pipefail

CHECK="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-closing-keywords.sh"

# The line #707's body carried; #690 stayed open and was closed by hand.
LINE="Closes #655, #690 and #673."
# The same three issues as Git Workflow §4 asks for them.
PER_LINE="Closes #655
Closes #690
Closes #673"
CR=$'\r'
TAB=$'\t'
FENCE='```'

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

# probe_names <name> <body> <line>: refused, and the report holds the line
# naming the references that may not close, exactly.
probe_names() {
  local name="$1" body="$2" expected_line="$3"

  local output actual=accepted
  if ! output="$(PR_BODY="${body}" "${CHECK}")"; then
    actual=refused
  fi
  if [[ "${actual}" == refused ]] && ! grep -qxF -- "${expected_line}" <<< "${output}"; then
    actual="refused without '${expected_line# }'"
  fi

  report refused "${actual}" "${name}"
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

echo "Probing the closing-keyword rule in ${CHECK##*/}"

# #707's line, and the same issues one keyword per line: the check's whole
# job. Seen red with the check made to pass every body, and (the second) with
# any keyword and reference refused whatever follows it.
probe refused "#707's line" "${LINE}"
probe accepted "one keyword per line" "${PER_LINE}"

# The report names the references after the first, and only those. Seen red
# with every reference on the line named, the keyword's own included.
probe_names "#707's line names #690 and #673" "${LINE}" "  FAIL    may not close: #690, #673"

# Each keyword GitHub honours. Seen red with each taken out of KEYWORD.
for keyword in close closes closed fix fixes fixed resolve resolves resolved; do
  probe refused "'${keyword}' before a list" "${keyword} #1, #2"
done

# Each separator. Seen red with each taken out of SEPARATOR: the comma
# (which #707's line also needs), 'and', '&', '/' and '+'.
probe refused "a comma" "Closes #1, #2"
probe refused "'and'" "Fixes #1 and #2"
probe refused "'&'" "closes #1 & #2"
probe refused "'/'" "Resolves #1 / #2"
probe refused "'+'" "Fixes #1 + #2"
# An 'and' after the comma. Seen red with the comma's optional 'and' removed.
probe refused "a comma and 'and'" "Closes #1, and #2"

# Spacing. Seen red with no space allowed before a separator, with a space
# required after a comma, and with the space after a keyword narrowed to one
# space character.
probe refused "a space before the comma" "Closes #1 , #2"
probe refused "no space after the comma" "Closes #1,#2"
probe refused "a tab after the keyword" "Closes${TAB}#1, #2"

# A colon after the keyword. Seen red with the colon not allowed.
probe refused "a colon after the keyword" "Resolves: #1, #2, #3"

# Case. Seen red with nocasematch off.
probe refused "upper case" "FIXES #1, #2"

# Emphasis. Seen red without the '*' strip, and without the '_' strip.
probe refused "bold around the keyword" "**Closes** #1, #2"
probe refused "italic around a listed reference" "Closes #1, _#2_"

# An underscore inside a repository's name stays in the name. Seen red with
# every '_' removed, which names the repository wrongly.
probe_names "a repository name with an underscore" \
  "Closes sujanto-gaws/my_repo#1, sujanto-gaws/my_repo#2" \
  "  FAIL    may not close: sujanto-gaws/my_repo#2"

# The other two reference forms. Seen red with each taken out of REFERENCE.
probe refused "owner/repo references" "Closes sujanto-gaws/kelir#1, sujanto-gaws/kelir#2"
probe refused "issue URLs" "Fixes https://github.com/sujanto-gaws/kelir/issues/1, https://github.com/sujanto-gaws/kelir/issues/2"

# The keyword anywhere on the line, against punctuation. Seen red with the
# keyword anchored to the line's start, and with the boundary before it
# narrowed to whitespace.
probe refused "a keyword mid-line" "This pull request closes #1, #2."
probe refused "a bracket against the keyword" "(closes #1, #2)"

# A list after a second keyword on the line. Seen red with the line given up
# after the first keyword's list ended.
probe refused "a list after a second keyword" "Closes #1, closes #2, #3"

# Every line is read. Seen red with only the first non-blank line read.
probe refused "a list on a later line" "Closes #1

Fixes #2, #3"

# CRLF, which GitHub's web editor sends. Seen red without the carriage-return
# strip: the fence's closing line, ending in a carriage return, never closed it.
probe refused "CRLF endings, a list after a fenced block" "${FENCE}${CR}
${LINE}${CR}
${FENCE}${CR}
Closes #1, #2${CR}"
probe accepted "CRLF endings, one keyword per line" "Closes #1${CR}
Closes #2${CR}"

# A backtick with no partner is text. Seen red with an unpartnered run hiding
# the rest of the line.
probe refused "a lone backtick before a list" "It's a \` alone; closes #1, #2"

# **Each issue with its own keyword passes, on one line or several.** GitHub's
# documentation gives that form. Seen red with a keyword and reference refused
# when any separator follows, whatever comes after it.
probe accepted "a keyword for each, on one line" "Closes #1, closes #2, fixes #3"

# **A list no keyword governs passes.** Seen red with the keyword requirement
# removed, so that any reference followed by a list was refused.
probe accepted "'See' before a list" "See #1, #2"
probe accepted "'Related:' before a list" "Related: #1, #2"

# The separator follows the reference directly. Seen red with anything allowed
# between the reference and the separator.
probe accepted "prose between the reference and a list" "Closes #1. It also touches #2 and #3."

# Whole words. Seen red without the boundary before the keyword.
probe accepted "a word that ends with a keyword" "The parser prefixes #1, #2 with a hash."

# A separator then a word, not a reference. Seen red with the reference after
# the separator made optional.
probe accepted "a separator then a word" "Fixes #1, and the rest is prose."

# HTML comments, which GitHub renders as nothing. Seen red without the comment
# removal; the second with a comment ended at its own line.
probe accepted "a list in a comment" "<!-- ${LINE} -->"
probe accepted "a list in a multi-line comment" "<!--
${LINE}
-->
Closes #1"

# Code. A list quoted in a fenced block, an indented one, and one fenced with
# tildes. Seen red without the fence skip, with the fence anchored to column 0,
# and with tildes not opening a fence.
probe accepted "a list in a fenced block" "Refused:

${FENCE}
${LINE}
${FENCE}

${PER_LINE}"
probe accepted "a list in an indented fenced block" "- Refused:

  ${FENCE}text
  ${LINE}
  ${FENCE}"
probe accepted "a list in a tilde fence" "~~~
${LINE}
~~~"

# A fence closes only on its own character, at least as long, so a fence can
# quote a fenced block. Seen red with a shorter fence closing a longer one, and
# with a backtick fence closing a tilde one.
probe accepted "a list in a four-backtick fence holding three" "\`\`\`\`markdown
${FENCE}
${LINE}
${FENCE}
\`\`\`\`"
probe accepted "a list in a tilde fence holding backticks" "~~~markdown
${FENCE}
${LINE}
${FENCE}
~~~"

# A list after a fence closes is read again. Seen red with a fence that never
# closes.
probe refused "a list after a fenced block" "${FENCE}
quoted
${FENCE}
${LINE}"

# Inline code. Seen red without the span removal; the double-backtick span with
# a span taken as one backtick to the next, and the span holding a backtick
# with a span ended at the next run of any length.
probe accepted "a list in a code span" "Write \`${LINE}\` as three lines."
probe accepted "a list in a double-backtick span" "Not \`\`Closes #1, #2\`\` but one per line."
probe accepted "a list in a span holding a backtick" "The span \`\`a \` Closes #1, #2 \` b\`\` is code."

# The text after a span is read. Seen red with a span hiding the rest of its
# line.
probe refused "a list after a code span" "The form \`Closes #1\` works; Closes #2, #3 does not."

# Nothing to read. Seen red with the check made to refuse every body.
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
  echo "${failures} probe(s) did not behave. The closing-keyword rule has changed shape."
  exit 1
fi

echo
echo "Every probe behaved."
