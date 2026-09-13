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
        # `|| true`, and stderr discarded, because **the body is not always
        # JSON and that is a case this script has to survive rather than abort
        # on** (#440): `/version.json` answers with Caddy's single-page
        # fallback on every frontend image that predates #362, and a `jq` parse
        # error arriving as a non-zero exit under `set -euo pipefail` would end
        # the script here — before the branch below that tells *this image does
        # not carry the file* apart from *the wrong image is serving*. Absent
        # is what an unparseable body has, which is what empty already means to
        # every caller: each one compares the value rather than trusting it.
        #
        # The `sed` arm below has always behaved this way, so on a host with jq
        # the script used to fail differently from a host without it, reading
        # the same response. Only the hosts without jq reached the message
        # record 07 recorded.
        printf '%s' "${json}" | jq -r ".${field} // empty" 2>/dev/null || true
    else
        printf '%s' "${json}" \
            | sed -n "s/.*\"${field}\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"
    fi
}

# True when the first version is an earlier release than the second — the three
# numeric components compared in order, any pre-release suffix ignored, so
# `0.7.0-rc` counts as `0.7.0` and is built from the same source.
#
# **A version this cannot read is never earlier.** Returning false for an
# unrecognised string sends it to the strict branch of the caller below, which
# is the safe direction: an unparseable version is not *known* to predate
# anything, and the cost of being wrong is a deploy that refuses rather than a
# deploy that passes something through unchecked.
version_precedes() {
    local left="${1%%-*}" right="${2%%-*}"

    [[ "${left}"  =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1
    [[ "${right}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1

    local IFS='.'
    # Word splitting on the IFS above is the intent, so the expansions are
    # deliberately unquoted.
    # shellcheck disable=SC2206
    local left_parts=(${left}) right_parts=(${right})

    local index
    for index in 0 1 2; do
        # `10#` so a zero-padded component is read as decimal rather than as
        # an octal literal that would make `08` a syntax error.
        if (( 10#${left_parts[index]} < 10#${right_parts[index]} )); then
            return 0
        fi
        if (( 10#${left_parts[index]} > 10#${right_parts[index]} )); then
            return 1
        fi
    done

    # Equal is not earlier: the floor version itself carries the file.
    return 1
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

# `.env.staging.example` is the list, and this loop is what keeps the script and
# the backend in step.
#
# The `v0.4.0` rehearsal found the script asserting four variables while the
# backend read seven, so a missing bootstrap credential surfaced as a container
# restarting with the reason buried in `docker compose logs` — on a script whose
# whole design is to fail fast and name the value. Hard-coding the longer list
# would have fixed that deploy and drifted again at the next variable, because
# **a duplicated list is only ever correct on the day it is written**.
#
# The example file is not a third list: it is the file an operator copies to
# make `.env`, so it is already the thing both sides agree on. A variable added
# to the backend reaches a deployment by being documented here, and this check
# turns that document into an assertion.
example_env="${KELIR_APP_DIR}/.env.staging.example"

if [[ -f "${example_env}" ]]; then
    while read -r declared; do
        # `KELIR_VERSION` is declared in the example because an operator who
        # drives compose by hand has nowhere else to put it. This script is the
        # other caller and it takes the version as its argument — section 3
        # exports it over whatever `.env` holds. Requiring it here would refuse
        # a deployment for omitting the one value the command line supplied,
        # which is how CI found it: the browser job writes a `.env` for the
        # length of one run and passes the version on the command line.
        if [[ "${declared}" == "KELIR_VERSION" ]]; then
            continue
        fi

        # Declared-but-empty is fine and is the point of the distinction: the
        # bootstrap trio below is legitimately empty on a deployment that
        # already has users. What this catches is a `.env` copied from an older
        # release, which does not mention the variable at all.
        grep -Eq "^[[:space:]]*(export[[:space:]]+)?${declared}=" "${KELIR_APP_DIR}/.env" \
            || die "${declared} is not set in .env — it is new since this file was copied; see .env.staging.example"
    done < <(grep -Eo '^[[:space:]]*KELIR_[A-Z0-9_]+=' "${example_env}" | tr -d ' =' )
fi

for required in KELIR_DB_PASSWORD KELIR_JWT_SECRET KELIR_MINIO_USER KELIR_MINIO_PASSWORD; do
    value="${!required:-}"
    [[ -n "${value}" ]] || die "${required} is empty in .env"

    case "${value}" in
        change-me|changeme|secret|test-secret|password)
            die "${required} is a placeholder; generate one with: openssl rand -base64 36"
            ;;
    esac
done

# The bootstrap administrator is all-or-nothing, which is the backend's own rule
# (`config::bootstrap_admin`): username without password is a startup error, and
# both unset is a deployment that intends to create its first user another way.
# Stating it here costs one comparison and saves a container restart loop.
bootstrap_username="${KELIR_BOOTSTRAP_ADMIN_USERNAME:-}"
bootstrap_password="${KELIR_BOOTSTRAP_ADMIN_PASSWORD:-}"

if [[ -n "${bootstrap_username}" && -z "${bootstrap_password}" ]]; then
    die "KELIR_BOOTSTRAP_ADMIN_USERNAME is set and KELIR_BOOTSTRAP_ADMIN_PASSWORD is empty; the backend refuses to start on that pair"
fi

if [[ -z "${bootstrap_username}" && -n "${bootstrap_password}" ]]; then
    die "KELIR_BOOTSTRAP_ADMIN_PASSWORD is set and KELIR_BOOTSTRAP_ADMIN_USERNAME is empty; the backend refuses to start on that pair"
fi

if [[ -z "${bootstrap_username}" ]]; then
    # A warning rather than a refusal, because the backend treats it that way:
    # the bootstrap is a no-op once any user exists, so an established
    # deployment leaves these empty on purpose. On an empty database it is an
    # application nobody can enter, which is worth saying out loud here rather
    # than leaving to be discovered at the login page.
    printf '\033[1;33mwarning:\033[0m no KELIR_BOOTSTRAP_ADMIN_* in .env — a deployment with no users will have no way in\n' >&2
fi

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

    docker build         -f "${SOURCE_DIR}/deploy/docker/frontend.Dockerfile"         --build-arg "KELIR_BUILD_SHA=${BUILD_SHA}"         -t "kelir-frontend:${VERSION}"         "${SOURCE_DIR}/kelir-frontend"
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
# 3b. Every service the compose file declares is in the state it should be in
# ---------------------------------------------------------------------------
#
# **A deployment that cannot be reached must not exit 0** (#361). `v0.6.0`'s
# release ran this script on a host where another container held the published
# port; Caddy could not bind, the rest of the stack came up, and the run was
# reported as a success.
#
# `set -e` and the smoke test below both look like they cover that, and
# measured on 2026-09-07 they nearly do: `docker compose up -d` exits **1** when
# a published port is already allocated, so the line above aborts. What neither
# covers is the case that makes the smoke test lie — **something else answering
# on the address**. `KELIR_PUBLIC_URL` is an address, not a container, so a
# previous deployment of the same version still listening on that port satisfies
# every assertion below it: `/health/ready`, the version, the environment.
#
# So this asks the compose project about its own containers instead. A service
# that is not running — or a one-shot that exited non-zero — fails the deploy
# here, naming itself, rather than being inferred from a curl that reached
# somebody else.
#
# One-shot services are expected to exit: `minio-init` creates the bucket and
# ends by checking it. `exited (0)` is a pass for those and a failure for
# anything else, which is the same distinction `service_completed_successfully`
# makes one layer down (#359).

log "Checking every service came up"

unhealthy=""
while read -r service state exit_code; do
    [[ -n "${service}" ]] || continue

    case "${state}" in
        running)
            ;;
        exited)
            [[ "${exit_code}" == "0" ]] \
                || unhealthy="${unhealthy}
  ${service}: exited (${exit_code})"
            ;;
        *)
            unhealthy="${unhealthy}
  ${service}: ${state}"
            ;;
    esac
done < <(docker compose -f "${COMPOSE_FILE}" ps -a --format '{{.Service}} {{.State}} {{.ExitCode}}')

if [[ -n "${unhealthy}" ]]; then
    printf '\033[1;31merror:\033[0m the stack is not up. Services not in a good state:%s\n' \
        "${unhealthy}" >&2
    printf '\nRecent logs:\n' >&2
    docker compose -f "${COMPOSE_FILE}" logs --tail 20 >&2
    exit 1
fi


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

# **The other artefact** (#362). `/version` identifies the backend; until this
# existed nothing identified the frontend, so a step whose whole content is
# *the expected version and SHA* was run against one image of the two — and
# `kelir-frontend:0.6.0` shipped byte-identical to `0.6.0-rc` with nobody able
# to tell. `/version.json` is a static asset of the bundle, which Caddy serves
# ahead of the SPA fallback.
#
# **An image older than the assertion is *unknown*, not *wrong*** (#440). The
# path the Caddyfile gives an unmatched request is `/index.html`, so a frontend
# built before #362 answers `/version.json` with `200 text/html` and the whole
# single-page document. Read as JSON that carries no version, and compared as a
# version, it reported the right image as the wrong one — so the first operator
# to follow the rollback command this script prints at the bottom got
#
#     error: the frontend reports , expected 0.6.0 — the wrong image is serving
#
# on a rollback whose stack was up and answering ([record 07](../../projects/releases/07.%20Release%20v0.7.0.md),
# *The failure this row asks to be recorded*). A rollback is run when something
# is already wrong, and being told a working one failed is the worst possible
# moment to be told it.
#
# **The fix is a third outcome rather than a weaker assertion.** The check now
# separates *this bundle says it is a different release* from *this bundle
# cannot say which release it is*, and only the second is excused — for images
# below the floor named below, where it is the expected answer rather than a
# symptom. A version that is present and different still fails at any version,
# which is the whole of what #362 bought.
#
# The floor is the first release whose frontend emits the file. #367 added it
# on 2026-09-07 and `v0.7.0` is the first release tagged after that, so every
# frontend image before `0.7.0` serves the fallback and none of them can be
# told apart by this check — which is stated here rather than discovered again.
frontend_version_json_since='0.7.0'

printf '  %-16s ' "/version.json"

# `-f` is deliberately absent and the status code read instead: the three
# answers this has to tell apart are not distinguishable from a body alone, and
# `curl -f` collapses two of them into one exit code. The trailing `-w` line is
# stripped back off below.
frontend_response="$(curl -sS --max-time 5 -w '\n%{http_code} %{content_type}' \
    "${KELIR_PUBLIC_URL}/version.json")" \
    || die "/version.json could not be fetched from ${KELIR_PUBLIC_URL} — the frontend is not answering"

frontend_meta="$(printf '%s' "${frontend_response}" | tail -1)"
frontend_status="${frontend_meta%% *}"
frontend_type="${frontend_meta#* }"
frontend_body="$(printf '%s' "${frontend_response}" | sed '$d')"

printf '%s\n' "${frontend_body}"

frontend_version="$(json_field "${frontend_body}" version)"

if [[ "${frontend_status}" != 2[0-9][0-9] ]]; then
    # Not even the fallback answered. The Caddyfile sends every unmatched path
    # to `/index.html`, so there is no frontend image at all for which this is
    # the normal reply.
    die "/version.json answered ${frontend_status} — the frontend is not serving its bundle"

elif [[ -n "${frontend_version}" ]]; then
    # The bundle named itself. **This branch is not gated by the floor**: an
    # answer that is present and different is a wrong image whatever version
    # was asked for, and excusing it below the floor would drop the assertion
    # instead of narrowing it.
    [[ "${frontend_version}" == "${VERSION}" ]] \
        || die "the frontend reports ${frontend_version}, expected ${VERSION} — the wrong image is serving"

elif version_precedes "${VERSION}" "${frontend_version_json_since}"; then
    # Unknown, and expected to be: this release predates the file. Said out
    # loud rather than passed over, because what the deploy is proceeding
    # without is a real check — the backend's identity is confirmed above and
    # the frontend's is not.
    printf '\033[1;33mwarning:\033[0m the frontend does not identify itself and %s predates %s, so this is expected:\n' \
        "${VERSION}" "${frontend_version_json_since}" >&2
    printf '  /version.json answered %s %s — Caddy'"'"'s single-page fallback, not the asset (#362, #440).\n' \
        "${frontend_status}" "${frontend_type}" >&2
    printf '  The frontend image is unverified on this deploy. The backend is not: /version reports %s above.\n' \
        "${VERSION}" >&2

else
    # Unknown, and not expected to be: this release should emit the file. The
    # most likely cause is the one #362 was filed for — an older frontend image
    # serving under a newer version's name.
    die "/version.json carries no version, and ${VERSION} is not older than ${frontend_version_json_since}, whose bundle emits one — answered ${frontend_status} ${frontend_type}, which is the single-page fallback; an older frontend image is serving"
fi

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
