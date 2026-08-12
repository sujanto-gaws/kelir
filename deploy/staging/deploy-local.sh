#!/usr/bin/env bash
#
# Deploy the release stack for local testing, reachable by IP rather than by
# hostname.
#
#   ./deploy-local.sh 0.1.0            # auto-detect the host IP, port 8080
#   ./deploy-local.sh 0.1.0 9090       # a different port
#   KELIR_HOST_IP=192.168.1.50 ./deploy-local.sh 0.1.0
#   KELIR_DB_PORT=5433 ./deploy-local.sh 0.1.0     # PostgreSQL on a spare port
#
# Same images, same compose file and same smoke test as deploy.sh — only the
# address changes. That is the point: testing a deployment that differs from the
# real one mostly proves the difference works.
#
# Two things necessarily differ from a hostname deployment:
#
#   * **Plain HTTP.** Certificates cannot be issued for an IP address, so Caddy
#     serves the site without TLS. NFR-SEC-010 (TLS 1.2+) is therefore not
#     satisfied by this mode, which is why it is for testing only.
#   * **KELIR_APP_ENV is `development`.** Staging refuses placeholder secrets,
#     which is right for an internet-facing host and needless friction for a
#     laptop. Override with KELIR_EXPECTED_ENV=staging to exercise the stricter
#     path deliberately.

set -euo pipefail

VERSION="${1:-}"
PORT="${2:-${KELIR_HTTP_PORT:-8080}}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ -n "${VERSION}" ]] || die "usage: $0 <version> [port]   e.g. $0 0.1.0 8080"

# ---------------------------------------------------------------------------
# Determine the address the stack will answer on
# ---------------------------------------------------------------------------

detect_host_ip() {
    # The address of the interface that carries the default route — the one
    # another machine on the network would use to reach this host. Loopback
    # would work from this machine only, which defeats the purpose.
    local ip=''

    if command -v ip >/dev/null 2>&1; then
        ip="$(ip -4 route get 1.1.1.1 2>/dev/null | grep -oP '(?<=src\s)\d+(\.\d+){3}' | head -1)"
    fi

    if [[ -z "${ip}" ]] && command -v hostname >/dev/null 2>&1; then
        ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
    fi

    if [[ -z "${ip}" ]] && command -v ipconfig >/dev/null 2>&1; then
        ip="$(ipconfig | grep -A4 -iE 'ethernet|wireless' \
            | grep -iE 'IPv4' | grep -oE '[0-9]+(\.[0-9]+){3}' | head -1)"
    fi

    printf '%s' "${ip}"
}

HOST_IP="${KELIR_HOST_IP:-$(detect_host_ip)}"

[[ -n "${HOST_IP}" ]] \
    || die "could not determine this host's IP address; set it explicitly:
    KELIR_HOST_IP=192.168.1.50 $0 ${VERSION} ${PORT}"

PUBLIC_URL="http://${HOST_IP}:${PORT}"

log "Deploying ${VERSION} for local testing at ${PUBLIC_URL}"

cat <<EOF

  Host IP        ${HOST_IP}       (override with KELIR_HOST_IP)
  Port           ${PORT}
  Scheme         http — no certificate can be issued for an IP, so this mode
                 does not satisfy NFR-SEC-010 and is for testing only
  Environment    ${KELIR_EXPECTED_ENV:-development}

EOF

# ---------------------------------------------------------------------------
# Hand off to the real deploy script
# ---------------------------------------------------------------------------
#
# Everything below — secret checks, pre-deploy backup, image build or pull,
# compose up, smoke test — is deploy.sh unchanged. Only the address, the
# published port and the expected environment are supplied differently.

export KELIR_SITE_ADDRESS=":80"          # inside the container; Caddy serves HTTP
export KELIR_HTTP_PORT="${PORT}"          # published on the host
export KELIR_HTTPS_PORT="${KELIR_HTTPS_PORT:-8443}"
export KELIR_PUBLIC_URL="${PUBLIC_URL}"
export KELIR_EXPECTED_ENV="${KELIR_EXPECTED_ENV:-development}"
export KELIR_APP_ENV="${KELIR_EXPECTED_ENV}"

# Local testing builds the working tree, not a tag: the whole point is to try a
# release candidate before it is tagged. Images already present at this tag are
# reused unless KELIR_FORCE_BUILD=1.
export KELIR_SOURCE_DIR="${KELIR_SOURCE_DIR:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"

exec "${SCRIPT_DIR}/deploy.sh" "${VERSION}"
