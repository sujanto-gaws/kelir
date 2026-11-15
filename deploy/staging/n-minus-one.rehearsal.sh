#!/usr/bin/env bash
#
# The N−1 rehearsal ADR-0051 §4 asks of #842, over the real upgrade path.
#
# After the upgrade, the release compose file names kelir_app in
# KELIR_DATABASE_URL, and `./deploy.sh <previous>` keeps that file. So a
# rollback starts the previous binary as the serving role, and that binary
# migrates at startup: sqlx runs `CREATE TABLE IF NOT EXISTS _sqlx_migrations`,
# which the serving role, holding no CREATE on schema public, may be refused.
# Release Process §6: rehearse the rollback, don't reason about it.
#
# What it does, in throwaway containers on a private network:
#   1. PostgreSQL, with the owner `kelir` and its database: a v0.9.0 host.
#   2. The previous release migrates and serves as the owner, as it does today,
#      and writes an audit row.
#   3. The upgrade: CHANGELOG.md's "any other host" statements, as a superuser.
#   4. This release, in staging, migrates as kelir and serves as kelir_app,
#      and writes an audit row as kelir_app alone.
#   5. The rollback: the previous release with this release's environment, so
#      KELIR_DATABASE_URL names kelir_app. Its outcome is recorded either way.
#   6. If 5 was refused, the documented fallback: the previous release with
#      KELIR_DATABASE_URL pointed at the owner, and an audited write.
#   7. The roll forward: this release again, whose narrowing and posture check
#      run over what the rollback left, and an audited write as kelir_app.
#
# Every line it prints also goes to the result file, with the commits, the
# image ids, the server's version and the time, so a run is its own record.
#
# Exit codes:
#   0   the previous release served as kelir_app; the rollback needs no step
#   10  the previous release was refused as kelir_app, and the owner fallback
#       and the roll forward passed: ADR-0051 §4's fallback is the rollback
#   1   a step failed, or the script did
#   2   a precondition is missing (docker, the tag)
#
# Settings:
#   KELIR_REHEARSAL_PREVIOUS        the tag to roll back to (default v0.9.0)
#   KELIR_REHEARSAL_PG_IMAGE        the server (default postgres:16)
#   KELIR_REHEARSAL_PORT            the host port the backends publish (18080)
#   KELIR_REHEARSAL_RESULT          the result file (n-minus-one-rehearsal.txt)
#   KELIR_REHEARSAL_CURRENT_IMAGE   use this image for this release, unbuilt
#   KELIR_REHEARSAL_PREVIOUS_IMAGE  use this image for the previous one, unbuilt
#
# CI's `N−1 rehearsal` job runs it against PostgreSQL 16 and 17 and keeps the
# result file as an artifact. By hand, as a user who can run docker:
#   deploy/staging/n-minus-one.rehearsal.sh

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${HERE}/../.." && pwd)"
PREVIOUS="${KELIR_REHEARSAL_PREVIOUS:-v0.9.0}"
PG_IMAGE="${KELIR_REHEARSAL_PG_IMAGE:-postgres:16}"
PORT="${KELIR_REHEARSAL_PORT:-18080}"
RESULT="${KELIR_REHEARSAL_RESULT:-n-minus-one-rehearsal.txt}"

RUN="kelir-n1-$$"
NETWORK="${RUN}-net"
DB="${RUN}-db"
BACKEND="${RUN}-backend"
CURRENT_IMAGE="${KELIR_REHEARSAL_CURRENT_IMAGE:-}"
PREVIOUS_IMAGE="${KELIR_REHEARSAL_PREVIOUS_IMAGE:-}"
BUILT_IMAGES=()
WORK="$(mktemp -d)"

# Throwaway values for containers this script creates and removes; not a
# secret of any deployment. `gitleaks:allow` keeps a secret scan off them.
OWNER_PASSWORD="rehearsal-owner-password" # gitleaks:allow
APP_PASSWORD="rehearsal-app-password" # gitleaks:allow
ADMIN_PASSWORD="rehearsal-administrator-password" # gitleaks:allow
OWNER_URL="postgres://kelir:${OWNER_PASSWORD}@postgres:5432/kelir"
APP_URL="postgres://kelir_app:${APP_PASSWORD}@postgres:5432/kelir"

log() { printf '\n==> %s\n' "$*"; }
pass() { printf 'PASS %s\n' "$*"; }
fail() { printf 'FAIL %s\n' "$*" >&2; exit 1; }
precondition() { printf 'PRECONDITION %s\n' "$*" >&2; exit 2; }

cleanup() {
    docker rm -f "${BACKEND}" "${DB}" > /dev/null 2>&1 || true
    docker network rm "${NETWORK}" > /dev/null 2>&1 || true
    if (( ${#BUILT_IMAGES[@]} > 0 )); then
        docker image rm "${BUILT_IMAGES[@]}" > /dev/null 2>&1 || true
    fi
    git -C "${REPO}" worktree remove --force "${WORK}/previous" > /dev/null 2>&1 || true
    rm -rf "${WORK}"
}
trap cleanup EXIT
trap 'printf "FAIL the script itself, at line %s\n" "${LINENO}" >&2; exit 1' ERR

for command in docker git curl; do
    command -v "${command}" > /dev/null || precondition "${command} is required"
done
git -C "${REPO}" rev-parse --verify --quiet "${PREVIOUS}^{commit}" > /dev/null \
    || precondition "no tag ${PREVIOUS} in ${REPO}; fetch tags first"

# Everything from here is the record.
: > "${RESULT}"
exec > >(tee -a "${RESULT}") 2>&1

# As the superuser, in the container, on the kelir database.
psql_as_superuser() {
    docker exec -i "${DB}" psql -X -q -v ON_ERROR_STOP=1 -U postgres -d kelir -tA "$@"
}

# ---------------------------------------------------------------------------
# The images
# ---------------------------------------------------------------------------

if [[ -z "${CURRENT_IMAGE}" ]]; then
    log "Building this tree's backend"
    CURRENT_IMAGE="kelir-backend:${RUN}-current"
    docker build -q -f "${REPO}/deploy/docker/backend.Dockerfile" \
        -t "${CURRENT_IMAGE}" "${REPO}/kelir-backend" > /dev/null
    BUILT_IMAGES+=("${CURRENT_IMAGE}")
fi

if [[ -z "${PREVIOUS_IMAGE}" ]]; then
    log "Building ${PREVIOUS}'s backend from its tag"
    PREVIOUS_IMAGE="kelir-backend:${RUN}-previous"
    git -C "${REPO}" worktree add --detach "${WORK}/previous" "${PREVIOUS}" > /dev/null 2>&1
    docker build -q -f "${WORK}/previous/deploy/docker/backend.Dockerfile" \
        -t "${PREVIOUS_IMAGE}" "${WORK}/previous/kelir-backend" > /dev/null
    BUILT_IMAGES+=("${PREVIOUS_IMAGE}")
fi

# ---------------------------------------------------------------------------
# 1. A v0.9.0 host's database
# ---------------------------------------------------------------------------

log "Starting ${PG_IMAGE}"
docker network create "${NETWORK}" > /dev/null
# Aliased `postgres`, a host the backend treats as local, so it dials without
# TLS as the development stack does (db.rs, LOCAL_DATABASE_HOSTS).
docker run -d --name "${DB}" --network "${NETWORK}" --network-alias postgres \
    -e POSTGRES_PASSWORD=rehearsal-superuser-password -e POSTGRES_DB=kelir \
    "${PG_IMAGE}" > /dev/null

ready=''
for _ in $(seq 60); do
    # The image's entrypoint starts a server for initdb and restarts it, so a
    # query has to answer twice in a row.
    if psql_as_superuser -c 'SELECT 1' > /dev/null 2>&1; then
        sleep 2
        if psql_as_superuser -c 'SELECT 1' > /dev/null 2>&1; then
            ready=1
            break
        fi
    fi
    sleep 1
done
[[ -n "${ready}" ]] || fail "PostgreSQL did not start"

cat <<RECORD
N−1 rehearsal, ADR-0051 §4 (#842)
  at          $(date -u +%Y-%m-%dT%H:%M:%SZ)
  this tree   $(git -C "${REPO}" rev-parse HEAD)
  previous    ${PREVIOUS} = $(git -C "${REPO}" rev-parse "${PREVIOUS}^{commit}")
  images      this: ${CURRENT_IMAGE} $(docker image inspect -f '{{.Id}}' "${CURRENT_IMAGE}")
              previous: ${PREVIOUS_IMAGE} $(docker image inspect -f '{{.Id}}' "${PREVIOUS_IMAGE}")
  server      ${PG_IMAGE} $(docker image inspect -f '{{index .RepoDigests 0}}' "${PG_IMAGE}" 2> /dev/null || true)
              $(psql_as_superuser -c 'SELECT version()')
RECORD

psql_as_superuser <<SQL
CREATE ROLE kelir WITH LOGIN PASSWORD '${OWNER_PASSWORD}';
ALTER DATABASE kelir OWNER TO kelir;
GRANT ALL ON SCHEMA public TO kelir;
SQL
pass "step 1: a v0.9.0 host's owner and database"

# ---------------------------------------------------------------------------
# The backend, and the write that audits
# ---------------------------------------------------------------------------

common_env=(
    -e KELIR_APP_ENV=staging
    -e KELIR_JWT_SECRET=rehearsal-jwt-secret-0123456789abcdef # gitleaks:allow
    -e KELIR_STORAGE_ACCESS_KEY=rehearsal-access-key
    -e KELIR_STORAGE_SECRET_KEY=rehearsal-secret-key # gitleaks:allow
    -e KELIR_BOOTSTRAP_ADMIN_USERNAME=admin
    -e "KELIR_BOOTSTRAP_ADMIN_PASSWORD=${ADMIN_PASSWORD}"
    -e KELIR_BOOTSTRAP_ADMIN_EMAIL=admin@localhost
)

# settle_connections: waits until no client holds a connection to kelir, so
# that audited_write counts only the backend started next. A force-removed
# container's connections can outlive it, and #987's CI once read the
# rolled-back v0.9.0's `kelir` connection as the next backend's. What is
# still open after 15 seconds belonged to a container that is gone; it is
# ended, and said so. A connection the next backend opens comes after this,
# so a real leak is still caught.
settle_connections() {
    local query="FROM pg_stat_activity WHERE datname = 'kelir'
        AND backend_type = 'client backend' AND pid <> pg_backend_pid()"
    local left
    for _ in $(seq 15); do
        left="$(psql_as_superuser -c "SELECT count(*) ${query}")"
        [[ "${left}" == "0" ]] && return 0
        sleep 1
    done
    echo "ending ${left} connection(s) a removed backend left: $(psql_as_superuser \
        -c "SELECT string_agg(usename || '@' || coalesce(host(client_addr), 'local'), ', ') ${query}")"
    psql_as_superuser -c "SELECT count(pg_terminate_backend(pid)) ${query}" > /dev/null
}

# start_backend <image> <env...>: runs it and waits for /health/ready. Returns
# non-zero with the container's log printed when it never gets there.
start_backend() {
    local image="$1"
    shift
    docker rm -f "${BACKEND}" > /dev/null 2>&1 || true
    settle_connections
    docker run -d --name "${BACKEND}" --network "${NETWORK}" -p "127.0.0.1:${PORT}:8080" \
        "${common_env[@]}" "$@" "${image}" > /dev/null

    for _ in $(seq 120); do
        if curl -fsS "http://127.0.0.1:${PORT}/health/ready" > /dev/null 2>&1; then
            return 0
        fi
        if [[ "$(docker inspect -f '{{.State.Running}}' "${BACKEND}")" != "true" ]]; then
            break
        fi
        sleep 1
    done

    echo "--- the backend's log, last 30 lines"
    docker logs "${BACKEND}" 2>&1 | tail -30
    echo "---"
    docker rm -f "${BACKEND}" > /dev/null 2>&1 || true
    return 1
}

writes=0

# audited_write <step> <role>: signs in and creates a party through the running
# backend; requires new audit rows, and every connection it holds to be <role>'s.
audited_write() {
    local step="$1" role="$2" api="http://127.0.0.1:${PORT}/api/v1" token before after status held
    writes=$((writes + 1))

    before="$(psql_as_superuser -c 'SELECT count(*) FROM audit_events')"
    token="$(curl -fsS -H 'Content-Type: application/json' \
        -d "{\"username\":\"admin\",\"password\":\"${ADMIN_PASSWORD}\"}" \
        "${api}/auth/login" | sed -n 's/.*"accessToken":"\([^"]*\)".*/\1/p')" || true
    [[ -n "${token}" ]] || fail "${step}: could not sign in"

    status="$(curl -sS -o "${WORK}/party.json" -w '%{http_code}' \
        -H "Authorization: Bearer ${token}" -H 'Content-Type: application/json' \
        -d "{\"partyId\":\"N1-${writes}\",\"partyTypeId\":\"PARTY_GROUP\",\"partyGroup\":{\"groupName\":\"Rehearsal ${writes}\"}}" \
        "${api}/master-data/parties")"
    [[ "${status}" == "201" ]] || fail "${step}: creating a party answered ${status}: $(cat "${WORK}/party.json")"

    after="$(psql_as_superuser -c 'SELECT count(*) FROM audit_events')"
    (( after > before )) || fail "${step}: no audit row was written (${before} before, ${after} after)"

    held="$(psql_as_superuser -c "SELECT coalesce(string_agg(DISTINCT usename, ','), '')
        FROM pg_stat_activity
        WHERE datname = 'kelir' AND backend_type = 'client backend' AND pid <> pg_backend_pid()")"
    [[ "${held}" == "${role}" ]] || fail "${step}: the backend holds connections as '${held}', not ${role} alone"

    pass "${step}: audit rows ${before} -> ${after}, every connection ${role}'s"
}

# ---------------------------------------------------------------------------
# 2. The previous release, as it runs on a v0.9.0 host
# ---------------------------------------------------------------------------

log "Starting ${PREVIOUS} as the owner, as a v0.9.0 host runs it"
start_backend "${PREVIOUS_IMAGE}" -e "KELIR_DATABASE_URL=${OWNER_URL}" \
    || fail "step 2: ${PREVIOUS} did not start as the owner on its own host"
audited_write "step 2: ${PREVIOUS} as the owner" kelir

# ---------------------------------------------------------------------------
# 3. The upgrade, as CHANGELOG.md's "any other host" note gives it
# ---------------------------------------------------------------------------

log "Applying the upgrade note's statements"
psql_as_superuser <<SQL
BEGIN;
CREATE ROLE kelir_app WITH LOGIN PASSWORD '${APP_PASSWORD}';
GRANT CONNECT ON DATABASE kelir TO kelir_app;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO kelir_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO kelir_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO kelir_app;
REVOKE UPDATE, DELETE, TRUNCATE ON audit_events FROM kelir_app;
REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON _sqlx_migrations FROM kelir_app;
ALTER DEFAULT PRIVILEGES FOR ROLE kelir IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO kelir_app;
ALTER DEFAULT PRIVILEGES FOR ROLE kelir IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO kelir_app;
COMMIT;
SQL
pass "step 3: kelir_app created and granted over ${PREVIOUS}'s schema"

# This release's environment, as the release compose file sets it.
this_release=(-e "KELIR_DATABASE_URL=${APP_URL}" -e "KELIR_DATABASE_MIGRATION_URL=${OWNER_URL}")

# ---------------------------------------------------------------------------
# 4. This release
# ---------------------------------------------------------------------------

log "Starting this release in staging, with both URLs"
start_backend "${CURRENT_IMAGE}" "${this_release[@]}" \
    || fail "step 4: this release did not start over the upgraded host"
audited_write "step 4: this release" kelir_app

# ---------------------------------------------------------------------------
# 5 and 6. The rollback, and its fallback
# ---------------------------------------------------------------------------

log "Rolling back: ${PREVIOUS} with this release's environment, so as kelir_app"
if start_backend "${PREVIOUS_IMAGE}" "${this_release[@]}"; then
    audited_write "step 5: ${PREVIOUS} as kelir_app" kelir_app
    outcome=0
else
    echo "RECORDED step 5: ${PREVIOUS} was refused as kelir_app (log above)"
    log "The fallback (ADR-0051 §4): ${PREVIOUS} with KELIR_DATABASE_URL naming the owner"
    start_backend "${PREVIOUS_IMAGE}" -e "KELIR_DATABASE_URL=${OWNER_URL}" \
        -e "KELIR_DATABASE_MIGRATION_URL=${OWNER_URL}" \
        || fail "step 6: ${PREVIOUS} did not start as the owner either"
    audited_write "step 6: ${PREVIOUS} as the owner, the fallback" kelir
    outcome=10
fi

# ---------------------------------------------------------------------------
# 7. The roll forward
# ---------------------------------------------------------------------------

log "Rolling forward: this release again, over what the rollback left"
start_backend "${CURRENT_IMAGE}" "${this_release[@]}" \
    || fail "step 7: this release did not start again after the rollback"
audited_write "step 7: this release, rolled forward" kelir_app
docker rm -f "${BACKEND}" > /dev/null

echo
if (( outcome == 0 )); then
    echo "OUTCOME 0: ${PREVIOUS} serves as kelir_app; the rollback needs no step."
else
    echo "OUTCOME 10: ${PREVIOUS} is refused as kelir_app; the rollback runs as the owner,"
    echo "as CHANGELOG.md's upgrade note and Release Process §6 say, and rolls forward."
fi
exit "${outcome}"
