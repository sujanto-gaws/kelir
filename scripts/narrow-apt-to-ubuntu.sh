#!/usr/bin/env bash
#
# Disable every apt source that is not Ubuntu's own (issue #408 AC1).
#
# **For roughly forty minutes on 2026-09-09 nobody in this project could merge
# anything**, because Google regenerated the `Release` file of its Chrome apt
# repository eight hours ahead of the `Packages` file served beside it. apt
# checked the new index against the old file, refused the mismatch, and every
# `apt-get update` on the runner failed — whatever it was for. In the E2E job
# that was `playwright install --with-deps`, which refreshes *every* configured
# source before installing chromium's system libraries. `End-to-end (browser)`
# is a required context and bypassing is off (#335), so a red run there is a
# merge nobody can push through, including the merge that would have fixed it.
#
# **The job never needed Google's repository.** chromium's dependencies are
# Ubuntu packages; the Chrome repository is on the image because the image
# ships Chrome, not because anything here installs from it. So the remedy is
# to stop refreshing sources this job does not resolve packages from: a source
# apt never reads cannot hold a gate shut.
#
# **The predicate is the URI, not the filename.** An allowlist of names would
# go stale the first time an image renamed a file — on the hosted runner
# Chrome's is `google-chrome.sources`, in deb822, not the `.list` the issue's
# log implies. So a file is disabled when it carries an `http(s)` index that is
# not on `ubuntu.com`, and kept otherwise. Comments are stripped first, so a
# third party someone already commented out is not counted as one and its file
# is left alone.
#
# **Only `http(s)` URIs count, and that is the correction the first run of this
# script bought.** `/etc/apt/sources.list` on the hosted image does not name a
# host at all — it reads `deb mirror+file:/etc/apt/apt-mirrors.txt noble main`,
# the indirection GitHub uses to fail over between Azure's Ubuntu mirrors. A
# scan for `http://` therefore found *no* Ubuntu source, every file it could
# see was a third party, and the guard at the foot of this file stopped the job
# saying so. It was right to: judging a file by URIs it does not carry is how
# you disable the distribution's own archive by accident. A source reached
# through a mirror file is not an index a third party can hold hostage, so it
# is left alone and counted as kept.
#
# **Disabling is a rename, not a delete.** The suffix takes the file out of the
# `*.list` / `*.sources` patterns apt reads while leaving it in place to be
# read by a human debugging the job.
#
# Run it against a throwaway tree to see what it would do on an image:
#
#     KELIR_APT_ROOT=/tmp/apt-replica ./scripts/narrow-apt-to-ubuntu.sh
#
# Exits non-zero, naming the cause, if nothing survives — see the guard at the
# foot of the file for why that case is worth its own message.

set -euo pipefail

# Overridable so the script can be exercised against a replica tree without
# root and without touching the machine running it.
apt_root="${KELIR_APT_ROOT:-/etc/apt}"

# The distribution's own hosts. Every Ubuntu mirror GitHub's images have used
# is under this domain — `azure.archive.ubuntu.com` on the hosted runners,
# `security.ubuntu.com` for the security suite.
ubuntu_uri='^https?://([^/]*\.)?ubuntu\.com(/|$)'

# A line that configures a source at all, in either format: the `deb`/`deb-src`
# one-liners third parties still ship, and the deb822 `URIs:` stanzas.
source_line='^[[:space:]]*(deb(-src)?[[:space:]]|URIs:)'

kept=0

for source in "${apt_root}/sources.list" "${apt_root}/sources.list.d/"*; do
  [ -f "${source}" ] || continue

  body="$(sed 's/#.*//' "${source}")"

  # Nothing to fetch — a stub, or a file whose every entry is commented out.
  # Neither can fail an update, so neither is touched and neither is counted.
  printf '%s\n' "${body}" | grep -qE "${source_line}" || continue

  foreign="$(printf '%s\n' "${body}" \
    | grep -oE 'https?://[^ ]+' \
    | grep -vE "${ubuntu_uri}" || true)"

  if [ -n "${foreign}" ]; then
    echo "disabling ${source}"
    printf '%s\n' "${foreign}" | sort -u | sed 's/^/    /'
    mv "${source}" "${source}.disabled-by-ci"
  else
    kept=$((kept + 1))
  fi
done

# **Should a runner image ever move Ubuntu's own sources off ubuntu.com, the
# loop above would disable all of them.** apt would then update cleanly against
# nothing and the failure would surface two steps later as a package it cannot
# locate — the same shape of misdirection #408's second defect is about. Say it
# here instead. **This is not hypothetical: it fired on this script's first run
# and was how the `mirror+file:` indirection above was found.**
if [ "${kept}" -eq 0 ]; then
  echo "::error::no apt source survived the narrowing under ${apt_root} — nothing here is recognisably Ubuntu's own archive, so refusing to leave apt with no sources at all"
  exit 1
fi

echo "kept ${kept} Ubuntu source file(s) under ${apt_root}"
