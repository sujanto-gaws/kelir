#!/usr/bin/env bash
#
# Deploy a released version to kelir-staging-01.
#
#   ./deploy.sh 0.1.0
#
# Runs on the staging host, from ${KELIR_APP_DIR} (default /opt/kelir), after
# provision-ubuntu-24.sh has set the host up.
#
# Images: pulled from ${KELIR_IMAGE_REGISTRY} when that is set, otherwise built
# on this host from a checkout of the tag. Building on the host is the fallback
# for a project with no registry yet; once images are published, set the
# registry and this becomes a pull, which is what the release process means by
# deploying the same artifacts rather than rebuilding them (§4 step 8).

set -euo pipefail

VERSION="${1:-}"
KELIR_APP_DIR="${KELIR_APP_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
KELIR_REPO_URL="${KELIR_REPO_URL:-https://github.com/sujanto-gaws/kelir.git}"
KELIR_BUILD_DIR="${KELIR_BUILD_DIR:-/opt/kelir-build}"
KELIR_HOSTNAME="${KELIR_HOSTNAME:-staging.kelir.gawshub.com}"
COMPOSE_FILE="${KELIR_APP_DIR}/docker-compose.staging.yml"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ -n "${VERSION}" ]] || die "usage: $0 <version>   e.g. $0 0.1.0"
[[ -f "${COMPOSE_FILE}" ]] || die "compose file not found: ${COMPOSE_FILE}"
[[ -f "${KELIR_APP_DIR}/.env" ]] || die "${KELIR_APP_DIR}/.env not found — copy .env.staging.example and fill it in"

# ---------------------------------------------------------------------------
# 1. Refuse to deploy on unset or placeholder secrets
# ---------------------------------------------------------------------------
#
# The backend refuses placeholder secrets at startup, but failing here is
# cheaper: it happens before the running version is replaced.

log "Checking ${KELIR_APP_DIR}/.env"

# shellcheck disable=SC1091
set -a; . "${KELIR_APP_DIR}/.env"; set +a

for required in KELIR_DB_PASSWORD KELIR_JWT_SECRET KELIR_MINIO_USER KELIR_MINIO_PASSWORD; do
    value="${!required:-}"
    [[ -n "${value}" ]] || die "${required} is empty in .env"

    case "${value}" in
        change-me|changeme|secret|test-secret|password)
            die "${required} is a placeholder; generate one with: openssl rand -base64 36"
            ;;
    esac
done

# ---------------------------------------------------------------------------
# 2. Obtain the images
# ---------------------------------------------------------------------------

if [[ -n "${KELIR_IMAGE_REGISTRY:-}" ]]; then
    log "Pulling images for ${VERSION} from ${KELIR_IMAGE_REGISTRY}"

    docker pull "${KELIR_IMAGE_REGISTRY}/kelir-backend:${VERSION}"
    docker pull "${KELIR_IMAGE_REGISTRY}/kelir-frontend:${VERSION}"
    docker tag "${KELIR_IMAGE_REGISTRY}/kelir-backend:${VERSION}" "kelir-backend:${VERSION}"
    docker tag "${KELIR_IMAGE_REGISTRY}/kelir-frontend:${VERSION}" "kelir-frontend:${VERSION}"
else
    log "No registry configured — building ${VERSION} on this host from v${VERSION}"

    if [[ -d "${KELIR_BUILD_DIR}/.git" ]]; then
        git -C "${KELIR_BUILD_DIR}" fetch --tags --quiet origin
    else
        git clone --quiet "${KELIR_REPO_URL}" "${KELIR_BUILD_DIR}"
    fi

    git -C "${KELIR_BUILD_DIR}" checkout --quiet "v${VERSION}" \
        || die "tag v${VERSION} not found in ${KELIR_REPO_URL}"

    BUILD_SHA="$(git -C "${KELIR_BUILD_DIR}" rev-parse --short HEAD)"
    log "Building from v${VERSION} (${BUILD_SHA})"

    docker build \
        -f "${KELIR_BUILD_DIR}/deploy/docker/backend.Dockerfile" \
        --build-arg "KELIR_BUILD_SHA=${BUILD_SHA}" \
        -t "kelir-backend:${VERSION}" \
        "${KELIR_BUILD_DIR}/kelir-backend"

    docker build \
        -f "${KELIR_BUILD_DIR}/deploy/docker/frontend.Dockerfile" \
        -t "kelir-frontend:${VERSION}" \
        "${KELIR_BUILD_DIR}/kelir-frontend"
fi

# ---------------------------------------------------------------------------
# 3. Deploy
# ---------------------------------------------------------------------------
#
# Migrations run inside the backend on start, against the host's PostgreSQL.
# Back up first: a migration that fails halfway is exactly when yesterday's dump
# is not good enough.

if systemctl list-unit-files kelir-backup.service >/dev/null 2>&1; then
    log "Taking a pre-deploy backup"
    sudo systemctl start kelir-backup || die "pre-deploy backup failed — not deploying"
fi

log "Starting ${VERSION}"
cd "${KELIR_APP_DIR}"
KELIR_VERSION="${VERSION}" docker compose -f "${COMPOSE_FILE}" up -d --remove-orphans

# ---------------------------------------------------------------------------
# 4. Smoke test (release process §4 step 7)
# ---------------------------------------------------------------------------

log "Waiting for the backend to report ready"

ready=""
for _ in $(seq 1 30); do
    if curl -fsS --max-time 5 "https://${KELIR_HOSTNAME}/health/ready" >/dev/null 2>&1; then
        ready=1
        break
    fi
    sleep 5
done

[[ -n "${ready}" ]] || {
    printf '\033[1;31merror:\033[0m readiness never came up. Recent backend logs:\n' >&2
    docker compose -f "${COMPOSE_FILE}" logs --tail 40 backend >&2
    exit 1
}

log "Smoke test"

for path in /health /health/live /health/ready; do
    printf '  %-16s ' "${path}"
    curl -fsS --max-time 5 "https://${KELIR_HOSTNAME}${path}" || die "${path} failed"
    printf '\n'
done

printf '  %-16s ' "/version"
version_body="$(curl -fsS --max-time 5 "https://${KELIR_HOSTNAME}/version")"
printf '%s\n' "${version_body}"

reported="$(printf '%s' "${version_body}" | jq -r '.version')"
[[ "${reported}" == "${VERSION}" ]] \
    || die "/version reports ${reported}, expected ${VERSION} — the wrong image is running"

environment="$(printf '%s' "${version_body}" | jq -r '.environment')"
[[ "${environment}" == "staging" ]] \
    || die "/version reports environment ${environment}, expected staging"

cat <<EOF

$(printf '\033[1;32m==> %s is live at https://%s\033[0m' "${VERSION}" "${KELIR_HOSTNAME}")

Still to verify by hand, as each phase delivers it (release process §4 step 7):
  login · document submission · one workflow approval

Rollback:
  ./deploy.sh <previous-version>

EOF
