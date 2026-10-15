#!/usr/bin/env bash
#
# Refuses a pull request whose body follows a closing keyword with a list of
# issues (#725, retrospective 21).
#
# GitHub closes the issue a closing keyword names when the pull request merges.
# **Of a list after one keyword it closes only the first issue**, and says
# nothing about the rest. #707's body said `Closes #655, #690 and #673` and
# GitHub linked #655 alone: #673 was closed by the squash commit's own body,
# and #690 by hand. Across eleven merged pull requests whose bodies held such a
# list, GitHub linked the first reference every time and no other. Git
# Workflow §4 already said to repeat the keyword, one issue per line; #219 and
# #223 broke that sentence before #707 did. **A check makes the list an edit to
# the body, made before the merge, not an issue found open after it.**
#
# **The rule.** A closing keyword is *close*, *closes*, *closed*, *fix*,
# *fixes*, *fixed*, *resolve*, *resolves* or *resolved*, as a whole word,
# optionally followed by a colon. An issue reference is `#N`, `owner/repo#N`
# or `https://github.com/owner/repo/issues/N`, or a link whose text is `#N` or
# `owner/repo#N` (`[#N](…)`: GitHub closed #587 from #588's `Fixes [#587](…)`).
# A line is refused when a keyword and its reference are followed directly by
# a list separator and then by a reference with no keyword of its own. A
# separator is *and*; a comma, a semicolon, `&`, `/` or `+`, with or without
# spaces and with an *and* after it or not (`#1, and #2`); or a `-` with no
# space either side, which writes a range (`#1-#3`). That reference, and each
# one listed after it, will not close.
#
# - `Closes #1, #2`, `Fixes #1 and #2`, `Resolves: #1, #2, #3`,
#   `closes #1 & #2`, `Closes #1; #2` and `Closes #1-#3` are refused;
# - `Closes #1` and `Closes #2` on lines of their own are accepted, and so is
#   `Closes #1, closes #2`: each issue has its own keyword, which is the form
#   GitHub's documentation gives for closing several;
# - `See #1, #2` and `Related: #1, #2` are accepted: no keyword governs them;
# - `Closes #1. It also touches #2 and #3.` is accepted: the separator must
#   follow the reference directly, so prose between them ends the list. So are
#   `Closes #1 - #3` and `Closes #1 through #3`: a spaced dash is punctuation,
#   and a word is prose.
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
# **Linear in the body's length.** A body is anyone's text, up to GitHub's
# 65,536 characters, and the first version of this check took minutes on some
# 64 KB bodies: it paired backtick runs by rescanning the line for each one,
# re-matched the rest of a line for each listed reference, and forked a `sed`
# per line. Now each step reads the body once:
#
# - the shell's own string work runs under the `C` locale, byte by byte (see
#   the locale below);
# - comments are cut out after one split of the body at every `<!--`, and
#   lines are split once, by `mapfile`;
# - a line's backtick runs are split out once, and each run is paired with the
#   next run of its length through a table built in one pass from the right;
# - emphasis is removed by one `sed` over the whole body, the lists are found
#   by one `grep`, and one more `sed` splits each list into its references.
#
# A 64 KB body of any shape the test-engineer campaign on #729 tried runs in
# well under five seconds on the runner, and the job's `timeout-minutes` bounds
# whatever this misses.
#
# Usage: PR_BODY="<pull-request body>" scripts/check-closing-keywords.sh
#
# The body comes in through the environment and never through an argument the
# workflow interpolates, because a body is text anyone opening a pull request
# writes, and `${{ }}` pasted into `run:` is executed as shell.

set -euo pipefail

# Nothing here expands a filename, and the lists below are split by `IFS`.
set -f

# **Two locales, each for its own work.** Matching runs in `grep` and `sed`
# under `C.UTF-8`, so case folding and character classes behave the same on a
# runner whose default locale is `C` and on the author's machine (see
# check-pull-request-title.sh), and a letter such as *é* is a letter (*véfixes*
# is not *fixes*). The shell's own string work runs under `C`: under a UTF-8
# locale bash converts a string to wide characters for each pattern it tries,
# a pass over the whole body each time. Every byte the shell looks for (`<!--`,
# `-->`, backticks, fences, carriage returns, the no-break spaces written as
# bytes) is ASCII or a whole UTF-8 sequence, and an ASCII byte never occurs
# inside a multibyte character, so working on bytes splits nothing.
MATCHING=C.UTF-8
export LC_ALL=C

# **Unset is an error, empty is not.** A workflow that misspells the variable
# would otherwise pass every body as empty, which is a check that has silently
# stopped checking.
BODY="${PR_BODY?usage: PR_BODY=<pull-request body> check-closing-keywords.sh}"

# The keyword is read back from each list found, in any case.
shopt -s nocasematch

KEYWORD='(close[sd]?|fix(e[sd])?|resolve[sd]?)'
NAME='[[:alnum:]._-]+'
LINK='\]\([^)[:blank:]]*\)'
# One group, and none inside it: the split below reads it as `\1`.
REFERENCE="(#[0-9]+|${NAME}/${NAME}#[0-9]+|https?://github\.com/${NAME}/${NAME}/issues/[0-9]+|\[#[0-9]+${LINK}|\[${NAME}/${NAME}#[0-9]+${LINK})"
SEPARATOR='([[:blank:]]*[,;&/+][[:blank:]]*(and[[:blank:]]+)?|[[:blank:]]+and[[:blank:]]+|-)'

# A keyword, its reference, and the list after it: each separator directly
# followed by a reference with no keyword of its own.
LIST="(^|[^[:alnum:]])${KEYWORD}(:[[:blank:]]*|[[:blank:]]+)${REFERENCE}(${SEPARATOR}${REFERENCE})+"

FENCE_OPEN='^[[:blank:]]*(`{3,}|~{3,})'
FENCE_CLOSE='^[[:blank:]]*(`+|~+)[[:blank:]]*$'

# What GitHub shows: the body with every `<!-- ... -->` taken out. A comment
# spanning lines takes its line breaks with it, so the text after its `-->`
# stays on the line that `-->` closes, which is where the reader sees it.
#
# The body is split once, at every `<!--`, each first replaced by a byte no
# body needs (STX, deleted from the body beforehand; not SOH, which bash uses
# inside its own strings). Every piece after the first starts inside a
# comment, and only what follows its first `-->` is shown, so a comment never
# closed hides everything after it, and a `<!--` inside a comment opens nothing.
body="${BODY//$'\x02'/}"
body="${body//'<!--'/$'\x02'}"
IFS=$'\x02'
# shellcheck disable=SC2206 # Split at the marks, by design; `set -f` holds.
pieces=( ${body} )
IFS=$' \t\n'
visible="${pieces[0]:-}"
for piece in "${pieces[@]:1}"; do
  inside="${piece%%'-->'*}"
  if (( ${#inside} < ${#piece} )); then
    visible+="${piece:${#inside}+3}"
  fi
done

visible="${visible//$'\r'/}"
# U+00A0, U+2007 and U+202F, written as bytes so they mean the same in any
# locale; the runner's `[[:blank:]]` matches none of them.
visible="${visible//$'\xc2\xa0'/ }"
visible="${visible//$'\xe2\x80\x87'/ }"
visible="${visible//$'\xe2\x80\xaf'/ }"

mapfile -t lines <<< "${visible}"

# without_code_spans <line>: sets `text` to the line with each code span
# replaced by a space, so the words either side of one stay apart.
#
# The line is split at its backticks once. Between two pieces of text, `n`
# empty fields mean a run of `n + 1` backticks; an `x` on each end makes the
# first and last fields text. `closer[r]` is the next run after run `r` of the
# same length, filled from the right in one pass, so pairing a run is a look-up
# and not a scan.
without_code_spans() {
  local IFS='`'
  local -a fields
  # shellcheck disable=SC2206 # Split on backticks, by design; `set -f` holds.
  fields=( x$1x )

  local -a pieces=("${fields[0]}") runs=(0) closer=()
  local -A next=()
  local field run=1 count=0 r j ticks
  for field in "${fields[@]:1}"; do
    if [[ -z "${field}" ]]; then
      run=$((run + 1))
    else
      count=$((count + 1))
      runs[count]="${run}"
      pieces[count]="${field}"
      run=1
    fi
  done
  for (( r = count; r >= 1; r-- )); do
    closer[r]="${next[${runs[r]}]:-0}"
    next[${runs[r]}]="${r}"
  done

  text="${pieces[0]}"
  r=1
  while (( r <= count )); do
    j="${closer[r]}"
    if (( j > 0 )); then
      text+=" ${pieces[j]}"
      r=$((j + 1))
    else
      printf -v ticks '%*s' "${runs[r]}" ''
      text+="${ticks// /\`}${pieces[r]}"
      r=$((r + 1))
    fi
  done
  text="${text#x}"
  text="${text%x}"
}

# Each line as matching reads it: empty inside a fence, its code spans removed
# outside one. The line numbers stay those of `lines`.
texts=()
fence=''
for line in "${lines[@]}"; do
  if [[ -n "${fence}" ]]; then
    if [[ "${line}" =~ ${FENCE_CLOSE} ]] \
      && [[ "${BASH_REMATCH[1]:0:1}" == "${fence:0:1}" ]] \
      && (( ${#BASH_REMATCH[1]} >= ${#fence} )); then
      fence=''
    fi
    texts+=('')
    continue
  fi
  # A backtick fence whose line holds another backtick is a code span instead.
  if [[ "${line}" =~ ${FENCE_OPEN} ]] \
    && [[ "${BASH_REMATCH[1]}" == '~'* || "${line#*"${BASH_REMATCH[1]}"}" != *'`'* ]]; then
    fence="${BASH_REMATCH[1]}"
    texts+=('')
    continue
  fi

  if [[ "${line}" == *'`'* ]]; then
    without_code_spans "${line}"
  else
    text="${line}"
  fi
  texts+=("${text}")
done

# Emphasis out; each list found, as `<line number>:<match>`; then each match
# split at its separators, so a tab follows every reference but the last.
# The split's `s` is delimited by `@`, which no pattern here holds: a
# reference holds `/`. `grep` finding nothing is not an error, and anything
# else from it is: a command substitution carries the pipeline's status to
# `set -e`, which a process substitution would not.
matches="$(
  printf '%s\n' "${texts[@]}" \
    | LC_ALL="${MATCHING}" sed -E 's/\*//g; s/(^|[^[:alnum:]])_+/\1/g; s/_+([^[:alnum:]]|$)/\1/g' \
    | { LC_ALL="${MATCHING}" grep -a -n -o -i -E -e "${LIST}" || (( $? == 1 )); } \
    | LC_ALL="${MATCHING}" sed -E "s@${REFERENCE}${SEPARATOR}@\1\t@gI"
)"
found=()
if [[ -n "${matches}" ]]; then
  mapfile -t found <<< "${matches}"
fi

echo "Checking the pull-request body for a closing keyword followed by a list"

refused=0
current=0
keywords=()
unclosed=()

# report: prints the current line and what its lists leave unclosed.
report() {
  (( ${#unclosed[@]} > 0 )) || return 0
  refused=1
  local listed i
  printf -v listed '%s, ' "${unclosed[@]}"
  # Printed after a label, never at the start of a line. The runner reads a
  # line that begins `::` as a workflow command, and this text is the
  # author's.
  echo
  echo "  line    ${lines[current - 1]}"
  echo "  FAIL    will not close: ${listed%, }"
  for i in "${!unclosed[@]}"; do
    echo "  write   ${keywords[i]} ${unclosed[i]}"
  done
  keywords=()
  unclosed=()
}

for match in "${found[@]}"; do
  number="${match%%:*}"
  if (( number != current )); then
    report
    current="${number}"
  fi
  # The keyword and its own reference, then a field per listed reference.
  IFS=$'\t'
  # shellcheck disable=SC2206 # Split on tabs, by design; `set -f` holds.
  fields=( ${match#*:} )
  IFS=$' \t\n'
  [[ "${fields[0]}" =~ ${KEYWORD} ]]
  for reference in "${fields[@]:1}"; do
    keywords+=("${BASH_REMATCH[0]}")
    unclosed+=("${reference}")
  done
done
report

if (( refused )); then
  echo
  echo "A closing keyword is followed by a list of issues. GitHub closes only the"
  echo "first issue of such a list, and says nothing about the rest."
  echo "Give each issue its own keyword, one per line, as the 'write' lines show;"
  echo "see docs/standards/05. Git Workflow.md §4."
  echo "Edit the body and this job will re-run. Re-running the job from the"
  echo "Actions page does not: it reads the body as it was when the run began."
  exit 1
fi

echo "No closing keyword is followed by a list of issues."
