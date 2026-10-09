#!/usr/bin/env bash
#
# Refuses a pull request whose body opens by saying it is not ready (#696,
# decision D-112).
#
# A feature row's pull request carries this as its first line until its
# `test-engineer` campaign section is written:
#
#     **Not ready: <gate> is still running. Do not merge yet.**
#
# Plan 18 §7.3 made that line the control, and the control was a sentence:
# three Sprint 22 rows and #693 in Sprint 23 were merged while their bodies
# said not to. **A check makes the override an edit to the body, not a
# click.** It does not stop a merger who deletes the line; it makes deleting
# it deliberate. Git Workflow §4 states the convention.
#
# **The rule.** The first line of the body that has anything but spaces, tabs
# and carriage returns on it is read, and refused when it contains *not ready*
# or *do not merge yet* as whole words. Before matching:
#
# - carriage returns are removed, so a body saved from GitHub's web editor,
#   which sends CRLF, reads the same as one sent from `gh`;
# - `*` and `_` are removed, so bold or italic anywhere in the phrase does not
#   hide it (`**Not ready**`, `Do not merge _yet_`);
# - letters are lower-cased, so *NOT READY* and *Not Ready* are the line too;
# - runs of spaces and tabs become one space, so *Not  ready* is as well.
#
# Leading whitespace and markdown prefixes (`#`, `>`) need no rule: the phrase
# is searched for, not anchored, so whatever precedes it on the line does not
# matter. Whole words means *cannot ready* is not *not ready*.
#
# **Only the first line.** The convention puts the line first, where the
# merger reads it, and a body further down quotes it, explains it, or records
# that it was removed: this check's own pull request does all three. Reading
# the whole body would refuse those, and the remedy would be to stop writing
# the words, which is a convention nobody keeps.
#
# An empty body passes: it says nothing about readiness.
#
# Usage: PR_BODY="<pull-request body>" scripts/check-pull-request-ready.sh
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
BODY="${PR_BODY?usage: PR_BODY=<pull-request body> check-pull-request-ready.sh}"

PHRASES='not ready|do not merge yet'
PATTERN="(^|[^[:alnum:]])(${PHRASES})([^[:alnum:]]|$)"

first_line=''
while IFS= read -r line || [[ -n "${line}" ]]; do
  line="${line//$'\r'/}"
  if [[ -n "${line//[[:blank:]]/}" ]]; then
    first_line="${line}"
    break
  fi
done <<< "${BODY}"

echo "Checking the pull-request body's first line for a not-ready marker"

if [[ -z "${first_line}" ]]; then
  echo "  The body is empty, so it says nothing about readiness."
  exit 0
fi

# Printed after a label, never at the start of a line. The runner reads a line
# that begins `::` as a workflow command, and this text is the author's.
echo "  line    ${first_line}"
echo

normalised="${first_line//[*_]/}"
normalised="${normalised,,}"
normalised="$(tr -s '[:blank:]' ' ' <<< "${normalised}")"

if [[ "${normalised}" =~ ${PATTERN} ]]; then
  echo "  FAIL  the first line says the pull request is not ready ('${BASH_REMATCH[2]}')"
  echo
  echo "The body's first line marks this pull request as not ready to merge."
  echo "Remove that line once the gate it names has finished and its section is"
  echo "in the body; see docs/standards/05. Git Workflow.md §4."
  echo "Edit the body and this job will re-run. Re-running the job from the"
  echo "Actions page does not: it reads the body as it was when the run began."
  exit 1
fi

echo "The first line does not mark the pull request as not ready."
