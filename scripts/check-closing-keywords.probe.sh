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
# Written as bytes, so the probe means the same character in any locale.
NBSP=$'\xc2\xa0'
NNBSP=$'\xe2\x80\xaf'
ISSUES='https://github.com/sujanto-gaws/kelir/issues'

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
# naming the references that will not close, exactly.
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
probe_names "#707's line names #690 and #673" "${LINE}" "  FAIL    will not close: #690, #673"

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
# A semicolon, and a dash with no space either side, which writes a range.
# Seen red against the check before either was a separator, and with each
# taken out of SEPARATOR.
probe refused "';'" "Closes #1; #2"
probe refused "a range" "Closes #1-#3"
# A spaced dash is punctuation, and a word between references is prose. Seen
# red with spaces allowed around the dash, and with 'through' made a
# separator.
probe accepted "a spaced dash" "Closes #1 - #3"
probe accepted "'through'" "Closes #1 through #3"
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
# A separator in upper case. Seen red with the split into references made
# case-sensitive, which found the list and named nothing in it.
probe_names "an upper-case 'AND'" "Fixes #1 AND #2" "  FAIL    will not close: #2"

# Emphasis. Seen red without the '*' strip, and without the '_' strip.
probe refused "bold around the keyword" "**Closes** #1, #2"
probe refused "italic around a listed reference" "Closes #1, _#2_"

# An underscore inside a repository's name stays in the name. Seen red with
# every '_' removed, which names the repository wrongly.
probe_names "a repository name with an underscore" \
  "Closes sujanto-gaws/my_repo#1, sujanto-gaws/my_repo#2" \
  "  FAIL    will not close: sujanto-gaws/my_repo#2"

# The other two reference forms. Seen red with each taken out of REFERENCE.
probe refused "owner/repo references" "Closes sujanto-gaws/kelir#1, sujanto-gaws/kelir#2"
probe refused "issue URLs" "Fixes https://github.com/sujanto-gaws/kelir/issues/1, https://github.com/sujanto-gaws/kelir/issues/2"

# The keyword anywhere on the line, against punctuation. Seen red with the
# keyword anchored to the line's start, and with the boundary before it
# narrowed to whitespace.
probe refused "a keyword mid-line" "This pull request closes #1, #2."
probe refused "a bracket against the keyword" "(closes #1, #2)"

# A list after a second keyword on the line, and two lists on one line. Seen
# red with the line given up after the first keyword's list ended; the second
# with only a line's first list read.
probe refused "a list after a second keyword" "Closes #1, closes #2, #3"
probe_names "two lists on one line" "Closes #1, #2. Fixes #3, #4." "  FAIL    will not close: #2, #4"

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

# A separator then a word, not a reference. Seen red with any word allowed
# after a separator.
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

# **Shapes from the test-engineer campaign on #729.** Each is a rule the header
# states that no probe above held: seen red with the change named, made to a
# copy of the check, and green against the check as written.

# A span closes on a run of its own length, never a longer one. Seen red with
# a span closed by a run at least as long.
probe accepted "a list in a span holding longer runs" "\`a\`\` Closes #1, #2 \`\`b\`"

# A span reads as a space, so the words either side stay apart. Seen red with
# a span replaced by nothing, which joins 'the' to the keyword.
probe refused "a keyword right after a code span" "the\`check\`closes #1, #2"

# The report writes the keyword for each listed issue. Seen red with the
# 'write' line given another group of the match than the keyword.
probe_names "#707's line writes a keyword for #690" "${LINE}" "  write   Closes #690"

# A fence's closing line holds only the fence. Seen red with a fence closed by
# a line that opens with the fence and goes on.
probe accepted "a list after a line that does not close a fence" "${FENCE}
${FENCE} not a closing line
${LINE}
${FENCE}"

# Each comment ends at its own '-->', so text between two comments is read,
# and so is text after a stray '-->'. Seen red with a comment ended at the last
# '-->' in the body; the second with it ended at the last '-->' before the next
# '<!--'.
probe refused "a list between two comments" "<!-- a --> Closes #1, #2 <!-- b -->"
probe refused "a list before a stray '-->'" "<!-- a --> Closes #1, #2 -->"

# A comment never closed hides the rest of the body, as GitHub renders it.
# Seen red with the text after an unclosed '<!--' kept.
probe accepted "a list after a comment never closed" "<!-- a draft note
${LINE}"

# The colon after a keyword needs no space. Seen red with a space required.
probe refused "a colon and no space" "Fix:#1, #2"

# A repository's name may hold a dot. Seen red with '.' taken out of NAME.
probe_names "a repository name with a dot" \
  "Closes sujanto-gaws/kelir.github.io#1, sujanto-gaws/kelir.github.io#2" \
  "  FAIL    will not close: sujanto-gaws/kelir.github.io#2"

# Two backticks open no fence. Seen red with a fence opened by two.
probe refused "a list after a line opening with two backticks" "\`\` is two backticks
Closes #1, #2"

# A backtick fence's info string holds no backtick (CommonMark §4.5), so a
# line opening with three backticks and holding more is a code span, not a
# fence hiding the rest of the body. Seen red against the check before the
# fence opener read the rest of its line.
probe refused "a list after a line opening with a triple-backtick span" "${FENCE}x${FENCE} is a span
Closes #1, #2"
probe accepted "a list in a triple-backtick span" "${FENCE}Closes #1, #2${FENCE} is a span"

# **A no-break space is a space.** The runner's glibc leaves U+00A0, U+2007
# and U+202F out of '[[:blank:]]', which the author's machine may not, so
# these passed on the runner and were refused locally. Seen red on
# ubuntu (bash 5.2.21) against the check before it read them as spaces.
probe refused "a no-break space after the comma" "Closes #1,${NBSP}#2"
probe refused "a no-break space after the keyword" "Closes${NBSP}#1, #2"
probe refused "a narrow no-break space after the comma" "Closes #1,${NNBSP}#2"

# **A link whose text is a reference is a reference.** GitHub closed #587 from
# #588's 'Fixes [#587](…)'. Seen red against the check before REFERENCE held
# the link form.
probe_names "issue links" \
  "Fixes [#1](${ISSUES}/1), [#2](${ISSUES}/2)" \
  "  FAIL    will not close: [#2](${ISSUES}/2)"
probe refused "a link listed after a reference" "Fixes #1, [#2](${ISSUES}/2)"
probe refused "a reference listed after a link" "Fixes [#1](${ISSUES}/1) and #2"
probe accepted "a keyword for each link" "Fixes [#1](${ISSUES}/1), fixes [#2](${ISSUES}/2)"

# **A list wrapped across lines is a list** (#730). Commit bodies wrap at 72
# characters, so a separator can end a line and its reference open the next.
# Seen red against the check before lines were joined.
probe refused "a comma ending a line" "Closes #1,
#2"
probe refused "'and' ending a line" "Fixes #1 and
#2"
probe refused "a comma and 'and' ending a line" "Fixes #1, and
#2"
probe refused "a semicolon ending a line" "Closes #1;
#2"
probe refused "a wrapped list in a list item" "- Closes #1,
  #2"
probe refused "emphasis around the separator" "**Closes #1,**
#2"
# A wrap may fall before the word, not only after it. Seen red with only a
# line's end read; the '&' with it taken out of the opening separators.
probe refused "'and' opening a line" "Closes #1
and #2"
probe refused "'&' opening a line" "Closes #1
& #2"
# Each line of a wrapped list is named, and the report keeps its line numbers.
# Seen red with a chain joined two lines at a time, and with the joined lines
# not made up in blanks, which named the wrong line.
probe_names "a list wrapped over three lines" "Closes #1,
#2,
#3" "  FAIL    will not close: #2, #3"
probe_names "the line after a wrapped list keeps its number" "Closes #1,
#2

Fixes #3, #4" "  line    Fixes #3, #4"

# What ends a wrap. Seen red with a blank line joined over, with any next line
# taken as a reference, and with a fence's lines joined across.
probe accepted "a blank line ends a list" "Closes #1,

#2"
probe accepted "a next line of prose" "Closes #1,
which also touches #2"
probe accepted "a keyword for each across lines" "Closes #1,
closes #2"
probe accepted "'and' opening a line after a blank line" "Closes #1

and #2"
probe accepted "a next line in a fence" "Closes #1,
${FENCE}
#2
${FENCE}"

# **A fence in a list item** (#731). The opener follows the list marker, and
# the item's indented closing fence closes it. Seen red against the check
# before the opener read a marker: the closing fence opened a second fence and
# hid the rest of the body.
probe refused "a list after a list item's fence" "- ${FENCE}
  code
  ${FENCE}
- Closes #1, #2"
probe refused "a list after an ordered item's fence" "1. ${FENCE}
   code
   ${FENCE}
2. Closes #1, #2"
probe refused "a list after a '*' item's tilde fence" "* ~~~
  code
  ~~~
* Closes #1, #2"
probe accepted "a list in a list item's fence" "- ${FENCE}
  ${LINE}
  ${FENCE}"
probe accepted "a blank line in a list item's fence" "- ${FENCE}
  code

  ${LINE}
  ${FENCE}"

# **An unclosed fence stays in its item.** A line indented less than the item's
# content ends the item, and the fence with it. Seen red with a fence ended only
# by its closing line; the nested one with the item's own indent ignored; the
# indented opener with a fence's container read only from its own line.
probe refused "a fence never closed, then the next item" "- ${FENCE}
  code
- Closes #1, #2"
probe refused "a fence never closed, then a line out of the list" "- ${FENCE}
  code
Closes #1, #2"
probe refused "a nested fence never closed, then the next item" "- a
  - ${FENCE}
    code
  - Closes #1, #2"
probe refused "an indented fence never closed, then the next item" "- item
  ${FENCE}
  code
- Closes #1, #2"
# A line still in the item stays in the fence. Seen red with any next line
# ending it.
probe accepted "a list indented as the item's fence, never closed" "- ${FENCE}
  ${LINE}"
# Past the item, the same line is a fence of its own. Seen red with a fence's
# end not read again as an opener.
probe accepted "a fence opening where an item's fence ended" "- ${FENCE}
  code
${FENCE}
${LINE}"

# **Out of any list a fence never closed still hides the rest**, as GitHub
# renders it. Seen red with every opener's fence ended by a line indented less.
probe accepted "a list after a fence never closed" "${FENCE}
${LINE}"
probe accepted "a list after an indented fence never closed" "  ${FENCE}
code
${LINE}"
probe accepted "a list after a list ended and a fence never closed" "- item

Prose.

  ${FENCE}
${LINE}"

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
