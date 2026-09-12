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
# go stale the first time an image renamed a file, and on Ubuntu 24.04 the
# distribution's own sources live in `sources.list.d/ubuntu.sources` alongside
# the third parties rather than in `sources.list`. So a file is kept when every
# URI it carries is on `ubuntu.com`, and disabled otherwise. Comments are
# stripped first, so a third party someone already commented out is not counted
# as one and its file is left alone.
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

kept=0

for source in "${apt_root}/sources.list" "${apt_root}/sources.list.d/"*; do
  [ -f "${source}" ] || continue

  # Both formats in one sweep: the `deb http://…` one-liners third parties
  # still ship, and the deb822 `URIs:` lines Ubuntu 24.04 uses for its own.
  uris="$(sed 's/#.*//' "${source}" | grep -oE 'https?://[^ ]+' || true)"

  # Nothing to fetch — the 24.04 `sources.list` stub, or a file whose every
  # entry is commented out. Neither can fail an update, so neither is touched.
  [ -n "${uris}" ] || continue

  if printf '%s\n' "${uris}" | grep -qvE "${ubuntu_uri}"; then
    echo "disabling ${source}"
    printf '%s\n' "${uris}" | sort -u | sed 's/^/    /'
    mv "${source}" "${source}.disabled-by-ci"
  else
    kept=$((kept + 1))
  fi
done

# **Should a runner image ever move Ubuntu's own sources off ubuntu.com, the
# loop above would disable all of them.** apt would then update cleanly against
# nothing and the failure would surface two steps later as a package it cannot
# locate — the same shape of misdirection #408's second defect is about. Say it
# here instead.
if [ "${kept}" -eq 0 ]; then
  echo "::error::no apt source survived the narrowing under ${apt_root} — Ubuntu's own sources are not on ubuntu.com on this image"
  exit 1
fi

echo "kept ${kept} Ubuntu source file(s) under ${apt_root}"
