#!/usr/bin/env bash
#
# The N−1 rehearsal ADR-0051 §4 asks of #842: can the previous release's
# binary run as the serving role against a database this release migrated?
#
# After the upgrade, the release compose file names kelir_app in
# KELIR_DATABASE_URL, and `./deploy.sh <previous>` keeps that file. So a
# rollback starts the previous binary as the serving role, and that binary
# migrates at startup: sqlx takes its advisory lock, runs `CREATE TABLE IF NOT
# EXISTS _sqlx_migrations`, and reads the table. The serving role has no CREATE
# on schema public. Whether PostgreSQL refuses that statement when the table
# exists is what this decides (Release Process §6: rehearse the rollback,
# don't reason about it).
#
# What it does, in throwaway containers on a private network:
#   1. PostgreSQL; the owner `kelir` and the serving role `kelir_app`, granted
#      as Installation guide §5 and CHANGELOG.md's upgrade note say.
#   2. This tree's backend, in staging, migrates through
#      KELIR_DATABASE_MIGRATION_URL and serves as kelir_app; then it stops.
#   3. The previous release's backend, built from its tag, starts with the same
#      environment, so KELIR_DATABASE_URL names kelir_app. It must reach
#      /health/ready.
#   4. Through that binary's API: sign in, create a party, which audits. The
#      row must be in audit_events, and every connection the backend holds must
#      be kelir_app's.
#
# Usage, from anywhere, as a user who can run docker:
#   deploy/staging/n-minus-one.rehearsal.sh            # previous = v0.9.0
#   KELIR_REHEARSAL_PREVIOUS=v1.0.0 deploy/staging/n-minus-one.rehearsal.sh
#
# Settings:
#   KELIR_REHEARSAL_PREVIOUS   the tag to roll back to (default v0.9.0)
#   KELIR_REHEARSAL_PG_IMAGE   the server (default postgres:16; run it again
#                              with postgres:17, D-153's other supported one)
#   KELIR_REHEARSAL_PORT       the host port the backends publish (18080)
#
# Exit 0 when every step passed. A failure at step 3 is a design finding, not a
# script bug: ADR-0051 §4 then has the rollback run as the owner, and the
# serving role is NOT given CREATE. The script prints that fallback.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${HERE}/../.." && pwd)"
PREVIOUS="${KELIR_REHEARSAL_PREVIOUS:-v0.9.0}"
PG_IMAGE="${KELIR_REHEARSAL_PG_IMAGE:-postgres:16}"
PORT="${KELIR_REHEARSAL_PORT:-18080}"

RUN="kelir-n1-$$"
NETWORK="${RUN}-net"
DB="${RUN}-db"
CURRENT_IMAGE="kelir-backend:${RUN}-current"
PREVIOUS_IMAGE="kelir-backend:${RUN}-previous"
WORK="$(mktemp -d)"

OWNER_PASSWORD="rehearsal-owner-password"
APP_PASSWORD="rehearsal-app-password"
ADMIN_PASSWORD="rehearsal-administrator-password"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
pass() { printf '\033[1;32mPASS\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31mFAIL\033[0m %s\n' "$*" >&2; exit 1; }

cleanup() {
    docker rm -f "${RUN}-current" "${RUN}-previous" "${DB}" > /dev/null 2>&1 || true
    docker network rm "${NETWORK}" > /dev/null 2>&1 || true
    docker image rm "${CURRENT_IMAGE}" "${PREVIOUS_IMAGE}" > /dev/null 2>&1 || true
    git -C "${REPO}" worktree remove --force "${WORK}/previous" > /dev/null 2>&1 || true
    rm -rf "${WORK}"
}
trap cleanup EXIT

for command in docker git curl; do
    command -v "${command}" > /dev/null || fail "${command} is required"
done
git -C "${REPO}" rev-parse --verify --quiet "${PREVIOUS}^{commit}" > /dev/null \
    || fail "no tag ${PREVIOUS} in ${REPO}; fetch tags first"

# As the superuser, in the container, on the kelir database.
psql_as_superuser() {
    docker exec -i "${DB}" psql -X -q -v ON_ERROR_STOP=1 -U postgres -d kelir -tA "$@"
}

# ---------------------------------------------------------------------------
# 1. The server and the two roles
# ---------------------------------------------------------------------------

log "Starting ${PG_IMAGE}"
docker network create "${NETWORK}" > /dev/null
# Aliased `postgres`, a host the backend treats as local, so it dials without
# TLS as the development stack does (db.rs, LOCAL_DATABASE_HOSTS).
docker run -d --name "${DB}" --network "${NETWORK}" --network-alias postgres \
    -e POSTGRES_PASSWORD=rehearsal-superuser-password -e POSTGRES_DB=kelir \
    "${PG_IMAGE}" > /dev/null

for _ in $(seq 60); do
    docker exec "${DB}" pg_isready -U postgres -d kelir > /dev/null 2>&1 && break
    sleep 1
done
docker exec "${DB}" pg_isready -U postgres -d kelir > /dev/null || fail "PostgreSQL did not start"
sleep 2 # the image's entrypoint restarts the server once after initdb

log "Creating kelir, the owner, and kelir_app, the serving role (ADR-0051 §2)"
psql_as_superuser <<SQL
CREATE ROLE kelir WITH LOGIN PASSWORD '${OWNER_PASSWORD}';
ALTER DATABASE kelir OWNER TO kelir;
CREATE ROLE kelir_app WITH LOGIN PASSWORD '${APP_PASSWORD}';
GRANT CONNECT ON DATABASE kelir TO kelir_app;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO kelir_app;
ALTER DEFAULT PRIVILEGES FOR ROLE kelir IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO kelir_app;
ALTER DEFAULT PRIVILEGES FOR ROLE kelir IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO kelir_app;
SQL
pass "step 1: both roles provisioned"

# ---------------------------------------------------------------------------
# 2. This release migrates as the owner and serves as kelir_app
# ---------------------------------------------------------------------------

log "Building this tree's backend"
docker build -q -f "${REPO}/deploy/docker/backend.Dockerfile" \
    -t "${CURRENT_IMAGE}" "${REPO}/kelir-backend" > /dev/null

log "Building ${PREVIOUS}'s backend from its tag"
git -C "${REPO}" worktree add --detach "${WORK}/previous" "${PREVIOUS}" > /dev/null 2>&1
docker build -q -f "${WORK}/previous/deploy/docker/backend.Dockerfile" \
    -t "${PREVIOUS_IMAGE}" "${WORK}/previous/kelir-backend" > /dev/null

# The release compose file's environment, as `./deploy.sh <previous>` keeps it.
backend_env=(
    -e KELIR_APP_ENV=staging
    -e "KELIR_DATABASE_URL=postgres://kelir_app:${APP_PASSWORD}@postgres:5432/kelir"
    -e "KELIR_DATABASE_MIGRATION_URL=postgres://kelir:${OWNER_PASSWORD}@postgres:5432/kelir"
    -e KELIR_JWT_SECRET=rehearsal-jwt-secret-0123456789abcdef
    -e KELIR_STORAGE_ACCESS_KEY=rehearsal-access-key
    -e KELIR_STORAGE_SECRET_KEY=rehearsal-secret-key
    -e KELIR_BOOTSTRAP_ADMIN_USERNAME=admin
    -e "KELIR_BOOTSTRAP_ADMIN_PASSWORD=${ADMIN_PASSWORD}"
    -e KELIR_BOOTSTRAP_ADMIN_EMAIL=admin@localhost
)

# start_backend <name> <image>; waits for /health/ready, or fails with its log.
start_backend() {
    docker run -d --name "$1" --network "${NETWORK}" -p "127.0.0.1:${PORT}:8080" \
        "${backend_env[@]}" "$2" > /dev/null

    for _ in $(seq 120); do
        if curl -fsS "http://127.0.0.1:${PORT}/health/ready" > /dev/null 2>&1; then
            return 0
        fi
        if [[ "$(docker inspect -f '{{.State.Running}}' "$1")" != "true" ]]; then
            break
        fi
        sleep 1
    done

    docker logs "$1" 2>&1 | tail -40 >&2
    return 1
}

log "Starting this release in staging"
start_backend "${RUN}-current" "${CURRENT_IMAGE}" \
    || fail "step 2: this release did not reach /health/ready"
docker rm -f "${RUN}-current" > /dev/null
pass "step 2: this release migrated as kelir and served as kelir_app"

# ---------------------------------------------------------------------------
# 3. The previous release, as kelir_app
# ---------------------------------------------------------------------------

log "Starting ${PREVIOUS} with KELIR_DATABASE_URL naming kelir_app"
if ! start_backend "${RUN}-previous" "${PREVIOUS_IMAGE}"; then
    cat >&2 <<TEXT

step 3 FAILED: ${PREVIOUS}'s binary cannot start as the serving role.

By ADR-0051 §4 the serving role is NOT given CREATE. The rollback to
${PREVIOUS} runs in ${PREVIOUS}'s posture instead: for as long as it lasts, the
operator points KELIR_DATABASE_URL at the owner in docker-compose.staging.yml:

  KELIR_DATABASE_URL: postgres://kelir:\${KELIR_DB_PASSWORD}@...

CHANGELOG.md's upgrade note and the release notes must say so.
TEXT
    fail "step 3: ${PREVIOUS} did not reach /health/ready as kelir_app"
fi
pass "step 3: ${PREVIOUS} reached /health/ready as kelir_app"

# ---------------------------------------------------------------------------
# 4. A write that audits, through the previous binary
# ---------------------------------------------------------------------------

api="http://127.0.0.1:${PORT}/api/v1"
token="$(curl -fsS -H 'Content-Type: application/json' \
    -d "{\"username\":\"admin\",\"password\":\"${ADMIN_PASSWORD}\"}" \
    "${api}/auth/login" | sed -n 's/.*"accessToken":"\([^"]*\)".*/\1/p')"
[[ -n "${token}" ]] || fail "step 4: could not sign in through ${PREVIOUS}"

before="$(psql_as_superuser -c "SELECT count(*) FROM audit_events")"
status="$(curl -sS -o "${WORK}/party.json" -w '%{http_code}' \
    -H "Authorization: Bearer ${token}" -H 'Content-Type: application/json' \
    -d '{"partyId":"N1-REHEARSAL","partyTypeId":"PARTY_GROUP","partyGroup":{"groupName":"Rehearsal"}}' \
    "${api}/master-data/parties")"
[[ "${status}" == "201" ]] || fail "step 4: creating a party answered ${status}: $(cat "${WORK}/party.json")"
after="$(psql_as_superuser -c "SELECT count(*) FROM audit_events")"
(( after > before )) || fail "step 4: the write left no audit row (${before} before, ${after} after)"

roles="$(psql_as_superuser -c "SELECT string_agg(DISTINCT usename, ',')
    FROM pg_stat_activity
    WHERE datname = 'kelir' AND backend_type = 'client backend' AND pid <> pg_backend_pid()")"
[[ "${roles}" == "kelir_app" ]] || fail "step 4: the backend holds connections as '${roles}', not kelir_app alone"

pass "step 4: ${PREVIOUS} wrote audit rows (${before} -> ${after}) as kelir_app alone"
echo
echo "The rollback to ${PREVIOUS} needs no step: it serves as kelir_app (ADR-0051 §4)."
