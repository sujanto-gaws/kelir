#!/usr/bin/env bash
#
# Refuses a pull request whose body follows a closing keyword with a list of
# issues (#725, retrospective 21).
#
# GitHub closes the issue a closing keyword names when the pull request merges.
# It does not close every issue in a list after one keyword, and says nothing
# about the ones it skips: #707's body said `Closes #655, #690 and #673`, #690
# stayed open, and it was closed by hand. Git Workflow §4 already said to
# repeat the keyword, one issue per line; #219 and #223 broke that sentence
# before #707 did. **A check makes the list an edit to the body, made before
# the merge, not an issue found open after it.**
#
# **The rule.** A closing keyword is *close*, *closes*, *closed*, *fix*,
# *fixes*, *fixed*, *resolve*, *resolves* or *resolved*, as a whole word,
# optionally followed by a colon. An issue reference is `#N`, `owner/repo#N`
# or `https://github.com/owner/repo/issues/N`, or a link whose text is `#N` or
# `owner/repo#N` (`[#N](…)`: GitHub closed #587 from #588's `Fixes [#587](…)`).
# A line is refused when a keyword
# and its reference are followed directly by a list separator (a comma, *and*,
# `&`, `/` or `+`, with or without spaces, and a comma may take an *and*) and
# then by a reference with no keyword of its own. That reference, and each one
# listed after it, may not close.
#
# - `Closes #1, #2`, `Fixes #1 and #2`, `Resolves: #1, #2, #3` and
#   `closes #1 & #2` are refused;
# - `Closes #1` and `Closes #2` on lines of their own are accepted, and so is
#   `Closes #1, closes #2`: each issue has its own keyword, which is the form
#   GitHub's documentation gives for closing several;
# - `See #1, #2` and `Related: #1, #2` are accepted: no keyword governs them;
# - `Closes #1. It also touches #2 and #3.` is accepted: the separator must
#   follow the reference directly, so prose between them ends the list.
#
# Every line is read, not only the first: a keyword closes wherever it is.
# Before matching:
#
# - carriage returns are removed, so a body saved from GitHub's web editor,
#   which sends CRLF, reads the same as one sent from `gh`;
# - `*` is removed, and `_` where it starts or ends a word, so bold or italic
#   does not hide a list (`**Closes** #1, #2`, `Closes #1, _#2_`), and an
#   underscore inside a repository's name stays in the name;
# - case is ignored, so *CLOSES* and *Fixes* are keywords too;
# - spaces and tabs are matched in any number, and a no-break space (U+00A0,
#   U+2007, U+202F) is a space: the runner's glibc leaves those three out of
#   `[[:blank:]]`, so without this a list joined by one passed in CI.
#
# **What GitHub shows is read, and code is not.** A closing keyword inside code
# closes nothing, and a body that explains this rule has to quote the form it
# refuses: this check's own pull request does.
#
# - HTML comments are removed first, as `check-pull-request-ready.sh` removes
#   them: a comment spanning lines takes its line breaks with it, and one
#   opened and never closed hides the rest of the body.
# - A fenced code block is skipped: from a line opening with three or more
#   backticks or tildes, indented or not, to a line holding only a run of the
#   same character at least as long. A fence never closed hides the rest of
#   the body, as GitHub renders it. A backtick fence's info string holds no
#   backtick (CommonMark §4.5), so a line opening with ```x``` is a code span,
#   read below, and opens no fence.
# - An inline code span is removed: a run of backticks to the next run of the
#   same length on the same line. A run with no partner is text.
#
# A code span that spans lines is read as text, and so is an indented code
# block: nothing in this project's pull-request bodies writes either, and a
# list item's continuation is indented the same way.
#
# An empty body passes: it closes nothing.
#
# Usage: PR_BODY="<pull-request body>" scripts/check-closing-keywords.sh
#
# The body comes in through the environment and never through an argument the
# workflow interpolates, because a body is text anyone opening a pull request
# writes, and `${{ }}` pasted into `run:` is executed as shell.

set -euo pipefail

# Case folding and character classes behave the same on a runner whose default
# locale is `C` and on the author's machine (see check-pull-request-title.sh).
export LC_ALL=C.UTF-8

# **Unset is an error, empty is not.** A workflow that misspells the variable
# would otherwise pass every body as empty, which is a check that has silently
# stopped checking.
BODY="${PR_BODY?usage: PR_BODY=<pull-request body> check-closing-keywords.sh}"

# Keywords, separators and references match in any case; references are
# reported as written.
shopt -s nocasematch

KEYWORD='(close[sd]?|fix(e[sd])?|resolve[sd]?)'
NAME='[[:alnum:]._-]+'
# One group, and none inside it: the patterns below read it by position.
LINK='\]\([^)[:blank:]]*\)'
REFERENCE="(#[0-9]+|${NAME}/${NAME}#[0-9]+|https?://github\.com/${NAME}/${NAME}/issues/[0-9]+|\[#[0-9]+${LINK}|\[${NAME}/${NAME}#[0-9]+${LINK})"
SEPARATOR='([[:blank:]]*[,&/+][[:blank:]]*(and[[:blank:]]+)?|[[:blank:]]+and[[:blank:]]+)'

# A keyword and its reference; the last two groups are the reference and the
# rest of the line after it.
CLOSING="(^|[^[:alnum:]])${KEYWORD}(:[[:blank:]]*|[[:blank:]]+)${REFERENCE}(.*)$"
# A separator and a reference with no keyword, at the start of what is left.
LISTED="^${SEPARATOR}${REFERENCE}(.*)$"

FENCE_OPEN='^[[:blank:]]*(`{3,}|~{3,})'
FENCE_CLOSE='^[[:blank:]]*(`+|~+)[[:blank:]]*$'
# The backticks are the pattern's, not a command substitution.
# shellcheck disable=SC2016
TICKS='^([^`]*)(`+)(.*)$'

# What GitHub shows: the body with every `<!-- ... -->` taken out. A comment
# spanning lines takes its line breaks with it, so the text after its `-->`
# stays on the line that `-->` closes, which is where the reader sees it.
visible=''
rest="${BODY}"
while [[ "${rest}" == *'<!--'* ]]; do
  visible+="${rest%%'<!--'*}"
  rest="${rest#*'<!--'}"
  if [[ "${rest}" == *'-->'* ]]; then
    rest="${rest#*'-->'}"
  else
    rest=''
  fi
done
visible+="${rest}"

# without_code_spans <line>: sets `text` to the line with each code span
# replaced by a space, so the words either side of one stay apart.
without_code_spans() {
  local rest="$1" ticks after closed
  text=''
  while [[ "${rest}" =~ ${TICKS} ]]; do
    text+="${BASH_REMATCH[1]}"
    ticks="${BASH_REMATCH[2]}"
    rest="${BASH_REMATCH[3]}"
    after="${rest}"
    closed=0
    while [[ "${after}" =~ ${TICKS} ]]; do
      after="${BASH_REMATCH[3]}"
      if (( ${#BASH_REMATCH[2]} == ${#ticks} )); then
        closed=1
        break
      fi
    done
    if (( closed )); then
      text+=' '
      rest="${after}"
    else
      text+="${ticks}"
    fi
  done
  text+="${rest}"
}

echo "Checking the pull-request body for a closing keyword followed by a list"

refused=0
fence=''
while IFS= read -r line || [[ -n "${line}" ]]; do
  line="${line//$'\r'/}"
  # U+00A0, U+2007 and U+202F, written as bytes so they mean the same in any
  # locale; the runner's `[[:blank:]]` matches none of them.
  line="${line//$'\xc2\xa0'/ }"
  line="${line//$'\xe2\x80\x87'/ }"
  line="${line//$'\xe2\x80\xaf'/ }"

  if [[ -n "${fence}" ]]; then
    if [[ "${line}" =~ ${FENCE_CLOSE} ]] \
      && [[ "${BASH_REMATCH[1]:0:1}" == "${fence:0:1}" ]] \
      && (( ${#BASH_REMATCH[1]} >= ${#fence} )); then
      fence=''
    fi
    continue
  fi
  # A backtick fence whose line holds another backtick is a code span instead.
  if [[ "${line}" =~ ${FENCE_OPEN} ]] \
    && [[ "${BASH_REMATCH[1]}" == '~'* || "${line#*"${BASH_REMATCH[1]}"}" != *'`'* ]]; then
    fence="${BASH_REMATCH[1]}"
    continue
  fi

  without_code_spans "${line}"
  text="${text//\*/}"
  if [[ "${text}" == *_* ]]; then
    text="$(sed -E 's/(^|[^[:alnum:]])_+/\1/g; s/_+([^[:alnum:]]|$)/\1/g' <<< "${text}")"
  fi

  keywords=()
  unclosed=()
  rest="${text}"
  while [[ "${rest}" =~ ${CLOSING} ]]; do
    keyword="${BASH_REMATCH[2]}"
    rest="${BASH_REMATCH[-1]}"
    while [[ "${rest}" =~ ${LISTED} ]]; do
      keywords+=("${keyword}")
      unclosed+=("${BASH_REMATCH[-2]}")
      rest="${BASH_REMATCH[-1]}"
    done
  done

  if (( ${#unclosed[@]} > 0 )); then
    refused=1
    listed="$(printf '%s, ' "${unclosed[@]}")"
    # Printed after a label, never at the start of a line. The runner reads a
    # line that begins `::` as a workflow command, and this text is the
    # author's.
    echo
    echo "  line    ${line}"
    echo "  FAIL    may not close: ${listed%, }"
    for i in "${!unclosed[@]}"; do
      echo "  write   ${keywords[i]} ${unclosed[i]}"
    done
  fi
done <<< "${visible}"

if (( refused )); then
  echo
  echo "A closing keyword is followed by a list of issues. GitHub does not close"
  echo "every issue in such a list, and says nothing about the ones it skips."
  echo "Give each issue its own keyword, one per line, as the 'write' lines show;"
  echo "see docs/standards/05. Git Workflow.md §4."
  echo "Edit the body and this job will re-run. Re-running the job from the"
  echo "Actions page does not: it reads the body as it was when the run began."
  exit 1
fi

echo "No closing keyword is followed by a list of issues."
