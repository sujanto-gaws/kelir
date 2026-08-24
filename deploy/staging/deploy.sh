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
# Where the stack is reachable once deployed. Overridden by deploy-local.sh to
# an http://<ip>:<port> address; the smoke test below uses it verbatim.
KELIR_PUBLIC_URL="${KELIR_PUBLIC_URL:-https://staging.kelir.gawshub.com}"
COMPOSE_FILE="${KELIR_APP_DIR}/docker-compose.staging.yml"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ -n "${VERSION}" ]] || die "usage: $0 <version>   e.g. $0 0.1.0"

for required_command in docker curl; do
    command -v "${required_command}" >/dev/null 2>&1 \
        || die "${required_command} is required but not installed"
done

# Reads one top-level string field out of a JSON object. jq when it is present,
# otherwise a plain-text fallback: this script also runs on developer machines
# that never had jq installed, and a missing tool should not stop a deploy the
# rest of which works.
json_field() {
    local json="$1" field="$2"

    if command -v jq >/dev/null 2>&1; then
        printf '%s' "${json}" | jq -r ".${field} // empty"
    else
        printf '%s' "${json}" \
            | sed -n "s/.*\"${field}\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"
    fi
}
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

elif [[ -z "${KELIR_FORCE_BUILD:-}" ]]     && docker image inspect "kelir-backend:${VERSION}" >/dev/null 2>&1     && docker image inspect "kelir-frontend:${VERSION}" >/dev/null 2>&1; then

    # Both images are already present at this tag. Rebuilding them would produce
    # a different artifact from the one that may already have been tested, which
    # is the opposite of what a release deploy should do. KELIR_FORCE_BUILD=1
    # overrides when the tag is being reused deliberately, as during local
    # iteration.
    log "Using the ${VERSION} images already on this host"

else
    # Build from a source tree. KELIR_SOURCE_DIR points at an existing checkout —
    # how local testing builds the working tree before any tag exists. Without
    # it, the tag is fetched, which is the release path.
    if [[ -n "${KELIR_SOURCE_DIR:-}" ]]; then
        [[ -d "${KELIR_SOURCE_DIR}" ]] || die "KELIR_SOURCE_DIR does not exist: ${KELIR_SOURCE_DIR}"
        SOURCE_DIR="${KELIR_SOURCE_DIR}"
        BUILD_SHA="$(git -C "${SOURCE_DIR}" rev-parse --short HEAD 2>/dev/null || echo 'unknown')"
        log "Building ${VERSION} from the checkout at ${SOURCE_DIR} (${BUILD_SHA})"
    else
        log "No registry configured — building ${VERSION} from tag v${VERSION}"

        if [[ -d "${KELIR_BUILD_DIR}/.git" ]]; then
            git -C "${KELIR_BUILD_DIR}" fetch --tags --quiet origin
        else
            git clone --quiet "${KELIR_REPO_URL}" "${KELIR_BUILD_DIR}"
        fi

        git -C "${KELIR_BUILD_DIR}" checkout --quiet "v${VERSION}"             || die "tag v${VERSION} not found in ${KELIR_REPO_URL}"

        SOURCE_DIR="${KELIR_BUILD_DIR}"
        BUILD_SHA="$(git -C "${SOURCE_DIR}" rev-parse --short HEAD)"
        log "Building from v${VERSION} (${BUILD_SHA})"
    fi

    docker build         -f "${SOURCE_DIR}/deploy/docker/backend.Dockerfile"         --build-arg "KELIR_BUILD_SHA=${BUILD_SHA}"         -t "kelir-backend:${VERSION}"         "${SOURCE_DIR}/kelir-backend"

    docker build         -f "${SOURCE_DIR}/deploy/docker/frontend.Dockerfile"         -t "kelir-frontend:${VERSION}"         "${SOURCE_DIR}/kelir-frontend"
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

# Exported, not scoped to the up command: every later compose invocation
# interpolates it too, including the log dump on the failure path below. A
# diagnostic that fails when it is needed is worse than no diagnostic.
export KELIR_VERSION="${VERSION}"

docker compose -f "${COMPOSE_FILE}" up -d --remove-orphans

# ---------------------------------------------------------------------------
# 4. Smoke test (release process §4 step 7)
# ---------------------------------------------------------------------------

log "Waiting for the backend to report ready"

ready=""
for _ in $(seq 1 30); do
    if curl -fsS --max-time 5 "${KELIR_PUBLIC_URL}/health/ready" >/dev/null 2>&1; then
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
    curl -fsS --max-time 5 "${KELIR_PUBLIC_URL}${path}" || die "${path} failed"
    printf '\n'
done

printf '  %-16s ' "/version"
version_body="$(curl -fsS --max-time 5 "${KELIR_PUBLIC_URL}/version")"
printf '%s\n' "${version_body}"

reported="$(json_field "${version_body}" version)"
[[ "${reported}" == "${VERSION}" ]] \
    || die "/version reports ${reported}, expected ${VERSION} — the wrong image is running"

expected_env="${KELIR_EXPECTED_ENV:-staging}"
environment="$(json_field "${version_body}" environment)"
[[ "${environment}" == "${expected_env}" ]] \
    || die "/version reports environment ${environment}, expected ${expected_env}"

cat <<EOF

$(printf '\033[1;32m==> %s is live at %s\033[0m' "${VERSION}" "${KELIR_PUBLIC_URL}")

Sign-in is covered by the browser harness — run it against this address rather
than repeating the flow yourself (release process §4 step 7):

  cd e2e && npm ci
  KELIR_E2E_BASE_URL=${KELIR_PUBLIC_URL} KELIR_E2E_PASSWORD=... npm test

Still to verify by hand, as each phase delivers it:
  document submission · one workflow approval

Rollback:
  ./deploy.sh <previous-version>

EOF
