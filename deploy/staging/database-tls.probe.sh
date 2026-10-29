#!/usr/bin/env bash
#
# Probes the database hop's TLS rule (NFR-SEC-010, D-119 C, #803 AC7 and AC8).
# Each case prints `PASS` or `FAIL`, and any FAIL exits non-zero. Every case was
# seen red first; the pull request that added this records the runs.
#
#   database-tls.probe.sh
#       Generates a throwaway CA, a server certificate it signs and a second,
#       unrelated CA. Starts two PostgreSQL servers on a network of its own,
#       one with `ssl=on` and `ssl_min_protocol_version='TLSv1.2'`, one with
#       `ssl=off`, and runs the real backend against them, once per case:
#
#       (a) the TLS server, no setting: starts, migrates, and every connection
#           it holds is TLS (`pg_stat_ssl`);
#       (b) the TLS server, `verify-full` and the CA: starts, over TLS;
#       (c) the TLS server, `verify-full` and the other CA: refused;
#       (d) the TLS server by a second name its certificate does not carry,
#           `verify-full` and the CA: refused;
#       (e) the server without TLS, no setting: refused at startup naming
#           KELIR_DATABASE_SSL_MODE, and no migration table is created there;
#       (f) the server without TLS, `?sslmode=prefer` in the URL: refused
#           naming KELIR_DATABASE_SSL_MODE, and the server saw no connection;
#       (g) the server without TLS as `localhost`, from inside its network
#           namespace: starts, because the hop is local.
#
# **Both servers are reached by a name that is not local**, `db-tls.probe` and
# `db-plain.probe`, network aliases. `localhost`, `postgres` and
# `host.docker.internal` are local under the rule and would test nothing.
#
# **It runs by hand at the row's gate, not in CI**: CI's `services:` cannot
# mount certificates. It needs bash and a Docker CLI with its daemon, never the
# kelir-staging stack: every container, the network and the certificate volume
# are its own, named kelir-dbtls-probe-<pid>, and removed on exit. openssl runs
# in the PostgreSQL image, so no certificate touches the host or the tree. The
# backend needs neither MinIO nor ClamAV to reach `listening`.
#
#   KELIR_PROBE_BACKEND_IMAGE=kelir-backend:<sha> deploy/staging/database-tls.probe.sh
#
# or, with a debug binary built in the rust-tools container's target volume:
#
#   KELIR_PROBE_BACKEND_IMAGE=kelir-rust-tools:1.89 \
#   KELIR_PROBE_BACKEND_MOUNT=<target volume>:/target:ro \
#   KELIR_PROBE_BACKEND_ENTRYPOINT=/target/debug/kelir-backend \
#     deploy/staging/database-tls.probe.sh
#
# Settings:
#   KELIR_PROBE_BACKEND_IMAGE       the image the backend runs in (required);
#                                   one built from deploy/docker/backend.Dockerfile
#                                   runs it as it ships
#   KELIR_PROBE_BACKEND_ENTRYPOINT  the binary in that image; default: its entrypoint
#   KELIR_PROBE_BACKEND_MOUNT       a `docker run -v` value, for a binary in a volume
#   KELIR_PROBE_POSTGRES_IMAGE      postgres:16
#   KELIR_PROBE_START_SECONDS       how long a start may take; 120

set -euo pipefail

# Git Bash would rewrite /etc/kelir/... in the arguments below into Windows paths.
export MSYS_NO_PATHCONV=1

BACKEND_IMAGE="${KELIR_PROBE_BACKEND_IMAGE:?set KELIR_PROBE_BACKEND_IMAGE to the backend image}"
POSTGRES_IMAGE="${KELIR_PROBE_POSTGRES_IMAGE:-postgres:16}"
START_SECONDS="${KELIR_PROBE_START_SECONDS:-120}"

failures=0
check() {
    local name="$1"
    shift
    if "$@"; then
        echo "  PASS  ${name}"
    else
        echo "  FAIL  ${name}"
        failures=$(( failures + 1 ))
    fi
}

prefix="kelir-dbtls-probe-$$"
network="${prefix}"
certs="${prefix}-certs"
tls_db="${prefix}-tls"
plain_db="${prefix}-plain"
password="probe-$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')"
jwt_secret="probe-$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"

cleanup() {
    local containers
    containers="$(docker ps -aq --filter "name=^${prefix}-")"
    [[ -n "${containers}" ]] && docker rm -f -v ${containers} > /dev/null 2>&1
    docker network rm "${network}" > /dev/null 2>&1
    docker volume rm "${certs}" > /dev/null 2>&1
    true
}
trap cleanup EXIT

# Nothing the probe prints carries the database password.
mask() { sed "s/${password}/***/g"; }

# ---------------------------------------------------------------------------
# Certificates and servers
# ---------------------------------------------------------------------------

echo "Generating a CA, a server certificate for db-tls.probe, and another CA"
docker volume create "${certs}" > /dev/null
docker run --rm -v "${certs}:/certs" --entrypoint bash "${POSTGRES_IMAGE}" -c '
    set -euo pipefail
    cd /certs
    ca() {
        openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 1 \
            -subj "/CN=$1" -keyout "$2.key" -out "$2.crt" \
            -addext basicConstraints=critical,CA:TRUE \
            -addext keyUsage=critical,keyCertSign,cRLSign 2> /dev/null
    }
    ca "Kelir probe CA" ca
    ca "Kelir probe other CA" other-ca
    openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
        -subj /CN=db-tls.probe -keyout server.key -out server.csr 2> /dev/null
    printf "subjectAltName=DNS:db-tls.probe\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n" > server.ext
    openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 1 \
        -extfile server.ext -out server.crt 2> /dev/null
    rm -f ca.key other-ca.key server.csr server.ext ca.srl
    chown postgres:postgres server.key
    chmod 600 server.key
    chmod 644 ca.crt other-ca.crt server.crt
'

docker network create "${network}" > /dev/null

server() {
    local name="$1"
    shift
    docker run -d --name "${name}" --network "${network}" -v "${certs}:/certs:ro" \
        -e POSTGRES_USER=kelir -e POSTGRES_PASSWORD="${password}" -e POSTGRES_DB=kelir \
        "$@" -c log_connections=on > /dev/null
}
# One server answering to two names; its certificate carries only the first.
server "${tls_db}" --network-alias db-tls.probe --network-alias db-tls-other.probe \
    "${POSTGRES_IMAGE}" -c ssl=on -c ssl_cert_file=/certs/server.crt \
    -c ssl_key_file=/certs/server.key -c ssl_min_protocol_version=TLSv1.2
server "${plain_db}" --network-alias db-plain.probe "${POSTGRES_IMAGE}" -c ssl=off

# psql over the server's own socket, as the superuser the image made.
in_db() { docker exec "$1" psql -X -q -tA -U kelir -d "$2" -c "$3"; }

for db in "${tls_db}" "${plain_db}"; do
    # Over TCP, so the entrypoint's socket-only init server does not count.
    for (( i = 0; i < 60; i++ )); do
        docker exec "${db}" pg_isready -q -h 127.0.0.1 -U kelir -d kelir && break
        sleep 1
    done
    docker exec "${db}" pg_isready -q -h 127.0.0.1 -U kelir -d kelir \
        || { echo "${db} did not come up"; docker logs --tail 20 "${db}" 2>&1 | mask; exit 2; }
done
in_db "${plain_db}" kelir 'CREATE DATABASE kelir_local'
echo "db-tls.probe: ssl $(in_db "${tls_db}" kelir 'SHOW ssl'), min $(in_db "${tls_db}" kelir 'SHOW ssl_min_protocol_version');" \
    "db-plain.probe: ssl $(in_db "${plain_db}" kelir 'SHOW ssl')"

# ---------------------------------------------------------------------------
# The backend, once per case
# ---------------------------------------------------------------------------

url_at() { echo "postgres://kelir:${password}@$1"; }

# run_backend <case> <network> <url> [<KELIR_DATABASE_SSL_MODE> [<..._ROOT_CERT>]]
# Starts the backend as the case's container and waits until it logs
# `listening` or exits. Leaves `started`, `exited <code>` or `stuck` in
# ${outcome}. Blank settings are what the staging compose file passes when
# .env says nothing.
run_backend() {
    local name="${prefix}-$1" net="$2" url="$3" mode="${4:-}" root_cert="${5:-}" extra=()
    [[ -n "${KELIR_PROBE_BACKEND_ENTRYPOINT:-}" ]] && extra+=(--entrypoint "${KELIR_PROBE_BACKEND_ENTRYPOINT}")
    [[ -n "${KELIR_PROBE_BACKEND_MOUNT:-}" ]] && extra+=(-v "${KELIR_PROBE_BACKEND_MOUNT}")

    docker run -d --name "${name}" --network "${net}" ${extra[@]+"${extra[@]}"} \
        -v "${certs}:/etc/kelir/db-tls:ro" \
        -e NO_COLOR=1 -e KELIR_APP_ENV=development -e KELIR_JWT_SECRET="${jwt_secret}" \
        -e KELIR_BIND_ADDRESS=127.0.0.1:18080 -e KELIR_STORAGE_DRIVER=local \
        -e KELIR_DATABASE_URL="${url}" \
        -e KELIR_DATABASE_SSL_MODE="${mode}" -e KELIR_DATABASE_SSL_ROOT_CERT="${root_cert}" \
        "${BACKEND_IMAGE}" > /dev/null

    outcome=stuck
    for (( i = 0; i < START_SECONDS; i++ )); do
        if docker logs "${name}" 2>&1 | grep -q ' listening'; then
            outcome=started
            return
        fi
        if [[ "$(docker inspect -f '{{.State.Running}}' "${name}")" == false ]]; then
            outcome="exited $(docker inspect -f '{{.State.ExitCode}}' "${name}")"
            return
        fi
        sleep 1
    done
}

stop_backend() { docker rm -f -v "${prefix}-$1" > /dev/null; }

# The backend's address on the probe's network, as the server sees it.
address_of() {
    docker inspect -f "{{(index .NetworkSettings.Networks \"${network}\").IPAddress}}" "${prefix}-$1"
}

# Started, and every connection the server holds from this backend is TLS 1.2
# or higher. Run while it is up: the pool keeps its connections idle.
started_over_tls() {
    local seen
    [[ "${outcome}" == started ]] || { echo "        it ${outcome}:"; last_error "$1"; return 1; }
    seen="$(in_db "${tls_db}" kelir "SELECT count(*) || ' connection(s), ssl ' || coalesce(bool_and(s.ssl)::text, 'none') || ', ' || coalesce(string_agg(DISTINCT s.version, ','), 'no version')
        FROM pg_stat_ssl s JOIN pg_stat_activity a USING (pid) WHERE a.client_addr = '$(address_of "$1")'")"
    echo "        started; the server sees ${seen}"
    [[ "${seen}" =~ ^[1-9][0-9]*\ connection\(s\),\ ssl\ true,\ TLSv1\.[23](,TLSv1\.[23])?$ ]]
}

started() {
    [[ "${outcome}" == started ]] || { echo "        it ${outcome}:"; last_error "$1"; return 1; }
    echo "        started"
}

last_error() {
    docker logs "${prefix}-$1" 2>&1 | grep -E 'ERROR|failed to start' | tail -1 \
        | sed -E 's/^.*failed to start //' | mask | cut -c1-400 | sed 's/^/        /'
}

# Exited non-zero without listening, and its error names every text given.
refused_naming() {
    local case="$1" line text
    shift
    [[ "${outcome}" == exited* && "${outcome}" != 'exited 0' ]] \
        || { echo "        it ${outcome}"; return 1; }
    line="$(last_error "${case}")"
    echo "        ${outcome}:"
    echo "${line}"
    for text in "$@"; do
        [[ "${line}" == *"${text}"* ]] || { echo "        it does not name ${text}"; return 1; }
    done
}

connections_to_plain() { docker logs "${plain_db}" 2>&1 | grep -c 'connection received' || true; }

echo "AC7 a server that offers TLS 1.2 or higher, reached as db-tls.probe"

run_backend a "${network}" "$(url_at db-tls.probe:5432/kelir)"
check "(a) no setting: starts, and every connection it holds is TLS" started_over_tls a
check "(a) and it migrated the TLS server" \
    test "$(in_db "${tls_db}" kelir 'SELECT count(*) > 0 FROM _sqlx_migrations' 2>&1)" == t
stop_backend a

run_backend b "${network}" "$(url_at db-tls.probe:5432/kelir)" verify-full /etc/kelir/db-tls/ca.crt
check "(b) verify-full with the CA: starts, over TLS" started_over_tls b
stop_backend b

run_backend c "${network}" "$(url_at db-tls.probe:5432/kelir)" verify-full /etc/kelir/db-tls/other-ca.crt
check "(c) verify-full with another CA: refused, naming both settings" \
    refused_naming c KELIR_DATABASE_SSL_MODE KELIR_DATABASE_SSL_ROOT_CERT
stop_backend c

run_backend d "${network}" "$(url_at db-tls-other.probe:5432/kelir)" verify-full /etc/kelir/db-tls/ca.crt
check "(d) verify-full by a name the certificate does not carry: refused, naming both settings" \
    refused_naming d KELIR_DATABASE_SSL_MODE KELIR_DATABASE_SSL_ROOT_CERT
stop_backend d

echo "AC8 a server without TLS, reached as db-plain.probe"

run_backend e "${network}" "$(url_at db-plain.probe:5432/kelir)"
check "(e) no setting: refused at startup, naming KELIR_DATABASE_SSL_MODE" \
    refused_naming e KELIR_DATABASE_SSL_MODE
check "(e) and no migration table was created there" \
    test "$(in_db "${plain_db}" kelir "SELECT to_regclass('public._sqlx_migrations') IS NULL")" == t
stop_backend e

before="$(connections_to_plain)"
run_backend f "${network}" "$(url_at 'db-plain.probe:5432/kelir?sslmode=prefer')"
check "(f) sslmode=prefer in the URL: refused, naming KELIR_DATABASE_SSL_MODE and the mode" \
    refused_naming f KELIR_DATABASE_SSL_MODE sslmode=prefer
after="$(connections_to_plain)"
echo "        the server logged ${before} connection(s) before (f) and ${after} after"
check "(f) and refused before any dial: the server saw no connection" test "${before}" -eq "${after}"
stop_backend f

echo "AC8 the same server as localhost, from inside its network namespace"

run_backend g "container:${plain_db}" "$(url_at localhost:5432/kelir_local)"
check "(g) no setting: starts, because the hop is local" started g
check "(g) and it migrated" \
    test "$(in_db "${plain_db}" kelir_local 'SELECT count(*) > 0 FROM _sqlx_migrations' 2>&1)" == t
stop_backend g

if (( failures > 0 )); then
    echo "${failures} check(s) failed"
    exit 1
fi
echo "every check passed"
