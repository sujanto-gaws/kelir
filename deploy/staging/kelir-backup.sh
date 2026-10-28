#!/usr/bin/env bash
#
# kelir-backup: one backup of a Kelir host, a pair under one stamp
# (ADR-0049 §2, NFR-SEC-009, NFR-AVA-004).
#
#   kelir-<stamp>.dump.age      the database, pg_dump's custom format
#   kelir-<stamp>.objects.age   every object in the bucket, behind a manifest
#
# provision-ubuntu-24.sh installs this as /usr/local/bin/kelir-backup, and the
# kelir-backup unit runs it daily and before every deploy. It runs as root: it
# reads MinIO's root pair from .env (root, 600) and reaches MinIO, which is
# only on the stack's network, through Docker.
#
# **Both halves are encrypted to KELIR_BACKUP_AGE_RECIPIENTS before they reach
# the disk.** pg_dump and every object stream into `age`, so no plaintext dump
# and no plaintext object is ever written here, not even briefly. The host
# holds only the recipients, which are public; the identity that decrypts is
# kept off the host.
#
# **It fails closed.** No recipient, a store it cannot reach, or a stream that
# does not add up to its manifest exits non-zero, and a failed run leaves
# neither file of its pair.
#
#   kelir-backup                       take a backup
#   kelir-backup --encrypt-old-dumps   encrypt a pre-#778 plaintext *.dump to
#                                      the recipients and remove it (the
#                                      provisioner runs this on upgrade)
#
# Settings, each from the environment (the unit sets the first four):
#   KELIR_APP_DIR                 /opt/kelir; holds .env and the compose file
#   KELIR_BACKUP_DIR              /var/backups/kelir
#   KELIR_BACKUP_RETENTION_DAYS   14
#   KELIR_DB_NAME                 kelir
#   KELIR_ENV_FILE                ${KELIR_APP_DIR}/.env
#   KELIR_BACKUP_DB_OS_USER       postgres; pg_dump runs as this user, so peer
#                                 authentication still applies
#   KELIR_COMPOSE_PROJECT         kelir-staging
#   KELIR_BACKUP_NETWORK          ${KELIR_COMPOSE_PROJECT}_default
#   KELIR_BACKUP_MINIO_URL        http://minio:9000
#   KELIR_BACKUP_BUCKET           kelir
#   KELIR_BACKUP_MC_IMAGE         the `mc` image the compose file pins
#
# From the env file only: KELIR_BACKUP_AGE_RECIPIENTS, KELIR_MINIO_USER and
# KELIR_MINIO_PASSWORD. The credentials reach `mc` in its container's
# environment, never on a command line.
#
# The store archive's format, for an operator without kelir-restore-objects:
#   line 1      kelir-objects 1
#   line 2      <object count> <total bytes>
#   then        one line per object: <size in bytes> <key>
#   then        each object's bytes, in the manifest's order, with nothing
#               between them
#
# deploy.sh refuses an installed copy without the next line (#778, AC10).
# kelir-backup-format: 2

set -euo pipefail

KELIR_APP_DIR="${KELIR_APP_DIR:-/opt/kelir}"
KELIR_BACKUP_DIR="${KELIR_BACKUP_DIR:-/var/backups/kelir}"
KELIR_BACKUP_RETENTION_DAYS="${KELIR_BACKUP_RETENTION_DAYS:-14}"
KELIR_DB_NAME="${KELIR_DB_NAME:-kelir}"
KELIR_ENV_FILE="${KELIR_ENV_FILE:-${KELIR_APP_DIR}/.env}"
KELIR_BACKUP_DB_OS_USER="${KELIR_BACKUP_DB_OS_USER:-postgres}"
KELIR_COMPOSE_PROJECT="${KELIR_COMPOSE_PROJECT:-kelir-staging}"
KELIR_BACKUP_NETWORK="${KELIR_BACKUP_NETWORK:-${KELIR_COMPOSE_PROJECT}_default}"
KELIR_BACKUP_MINIO_URL="${KELIR_BACKUP_MINIO_URL:-http://minio:9000}"
KELIR_BACKUP_BUCKET="${KELIR_BACKUP_BUCKET:-kelir}"

die() { printf 'kelir-backup: %s\n' "$*" >&2; exit 1; }

# One value from the env file, read as text and never sourced: the file is
# root's, and this runs as root, so executing it would run whatever it holds.
env_value() {
    local key="$1" line value=''

    while IFS= read -r line || [[ -n "${line}" ]]; do
        line="${line#"${line%%[![:space:]]*}"}"
        line="${line#export }"
        [[ "${line}" == "${key}="* ]] && value="${line#"${key}="}"
    done < "${KELIR_ENV_FILE}"

    if [[ "${value}" == \"*\" || "${value}" == \'*\' ]]; then
        value="${value:1:${#value}-2}"
    fi

    printf '%s' "${value}"
}

[[ -r "${KELIR_ENV_FILE}" ]] || die "cannot read ${KELIR_ENV_FILE}"
command -v age >/dev/null 2>&1 || die "age is not installed (apt-get install age)"

# Every recipient is an X25519 one. A typo here would otherwise surface as
# `age` refusing it, after pg_dump had started; checking first names the value.
read -r -a recipients <<< "$(env_value KELIR_BACKUP_AGE_RECIPIENTS | tr ',' ' ')"
(( ${#recipients[@]} > 0 )) \
    || die "KELIR_BACKUP_AGE_RECIPIENTS is empty in ${KELIR_ENV_FILE}; nothing was written"

recipient_args=()
for recipient in "${recipients[@]}"; do
    [[ "${recipient}" =~ ^age1[02-9ac-hj-np-z]{58}$ ]] \
        || die "'${recipient}' in KELIR_BACKUP_AGE_RECIPIENTS is not an age X25519 recipient (age1...); nothing was written"
    recipient_args+=(-r "${recipient}")
done

# The one place either half is encrypted.
encrypt() { age "${recipient_args[@]}"; }

umask 077

# ---------------------------------------------------------------------------
# --encrypt-old-dumps: the upgrade from an unencrypted unit (#778, AC6)
# ---------------------------------------------------------------------------
#
# Each plaintext dump becomes <name>.age with the same modification time, so
# retention still counts from the day it was taken. **Removing the plaintext
# does not scrub its blocks**; the Installation guide §8 says so.

if [[ "${1:-}" == "--encrypt-old-dumps" ]]; then
    shopt -s nullglob
    converted=0

    for plaintext in "${KELIR_BACKUP_DIR}"/*.dump; do
        encrypt < "${plaintext}" > "${plaintext}.age.partial"
        touch -r "${plaintext}" "${plaintext}.age.partial"
        mv "${plaintext}.age.partial" "${plaintext}.age"
        rm -f "${plaintext}"
        converted=$(( converted + 1 ))
    done

    echo "kelir-backup: encrypted ${converted} plaintext dump(s) in ${KELIR_BACKUP_DIR}"
    exit 0
fi

[[ $# -eq 0 ]] || die "usage: kelir-backup [--encrypt-old-dumps]"

for required in pg_dump runuser docker jq; do
    command -v "${required}" >/dev/null 2>&1 || die "${required} is required but not installed"
done

minio_user="$(env_value KELIR_MINIO_USER)"
minio_password="$(env_value KELIR_MINIO_PASSWORD)"
[[ -n "${minio_user}" && -n "${minio_password}" ]] \
    || die "KELIR_MINIO_USER or KELIR_MINIO_PASSWORD is empty in ${KELIR_ENV_FILE}"
# `mc` takes the pair in `MC_HOST_kelir` as written, not percent-decoded
# (measured 2026-10-28), so it splits at the first `:` and the `@`. A generated
# value (`openssl rand -base64`) holds neither.
[[ "${minio_user}" != *[:@]* && "${minio_password}" != *@* ]] \
    || die "KELIR_MINIO_USER holds ':' or '@', or KELIR_MINIO_PASSWORD holds '@'; mc cannot be given that pair"

# The `mc` the stack pins (D-62), read from its compose file, so a bump there
# is a bump here.
mc_image="${KELIR_BACKUP_MC_IMAGE:-}"
if [[ -z "${mc_image}" && -r "${KELIR_APP_DIR}/docker-compose.staging.yml" ]]; then
    mc_image="$(grep -oE 'ghcr\.io/sujanto-gaws/mc:[^[:space:]]+' \
        "${KELIR_APP_DIR}/docker-compose.staging.yml" | head -1)"
fi
[[ -n "${mc_image}" ]] || die "no mc image: set KELIR_BACKUP_MC_IMAGE, or keep docker-compose.staging.yml in ${KELIR_APP_DIR}"

scheme="${KELIR_BACKUP_MINIO_URL%%://*}"
export MC_HOST_kelir="${scheme}://${minio_user}:${minio_password}@${KELIR_BACKUP_MINIO_URL#*://}"

# `--log-driver none`: the store half's plaintext leaves `mc` on stdout, and
# Docker's default json-file driver writes a container's stdout to a file under
# /var/lib/docker before `--rm` removes it. With no driver, nothing is kept.
in_mc() {
    docker run --rm -i --log-driver none --network "${KELIR_BACKUP_NETWORK}" \
        -e MC_HOST_kelir -e KELIR_BACKUP_BUCKET="${KELIR_BACKUP_BUCKET}" \
        --entrypoint bash "${mc_image}" "$@"
}

install -d -m 700 "${KELIR_BACKUP_DIR}"
chmod 700 "${KELIR_BACKUP_DIR}"

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
dump="${KELIR_BACKUP_DIR}/kelir-${stamp}.dump.age"
objects="${KELIR_BACKUP_DIR}/kelir-${stamp}.objects.age"
work="$(mktemp -d)"
completed=''
counter=''

# A failed run leaves neither file of its pair: a restore needs both.
cleanup() {
    [[ -n "${counter}" ]] && kill "${counter}" 2>/dev/null
    rm -rf "${work}"
    [[ -n "${completed}" ]] || rm -f "${dump}" "${dump}.partial" "${objects}" "${objects}.partial"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# 1. The database
# ---------------------------------------------------------------------------
#
# First, because an object is stored before its row commits and nothing
# deletes one: every attachment this dump names is in the store copy taken
# after it.

runuser -u "${KELIR_BACKUP_DB_OS_USER}" -- pg_dump --format=custom "${KELIR_DB_NAME}" \
    | encrypt > "${dump}.partial"

# ---------------------------------------------------------------------------
# 2. The object store
# ---------------------------------------------------------------------------

listing="$(in_mc -c 'mc ls --recursive --json "kelir/${KELIR_BACKUP_BUCKET}"' < /dev/null)" \
    || die "the object store could not be listed; no backup kept"

manifest="$(jq -r '
    if .status != "success" then error("the listing reported \(.error.message // .)")
    elif .type != "file" then empty
    elif (.key | test("[\\n\\r]")) then error("a key holds a line break: \(.key)")
    else "\(.size) \(.key)" end' <<< "${listing}")" \
    || die "the object store's listing could not be read; no backup kept"

count=0
total=0
if [[ -n "${manifest}" ]]; then
    while read -r size _; do
        count=$(( count + 1 ))
        total=$(( total + size ))
    done <<< "${manifest}"
fi

# The bytes streamed are counted on their way into `age`, through a FIFO, so
# the count is of what was encrypted and nothing is written to count it.
mkfifo "${work}/streamed"
wc -c < "${work}/streamed" > "${work}/count" &
counter=$!

{
    printf 'kelir-objects 1\n%s %s\n' "${count}" "${total}"
    if (( count > 0 )); then
        printf '%s\n' "${manifest}"
        cut -d' ' -f2- <<< "${manifest}" \
            | in_mc -c 'set -euo pipefail
                     while IFS= read -r key; do
                         mc cat "kelir/${KELIR_BACKUP_BUCKET}/${key}" < /dev/null
                     done' \
            | tee "${work}/streamed"
    else
        : > "${work}/streamed"
    fi
} | encrypt > "${objects}.partial"

wait "${counter}"
counter=''
streamed="$(tr -d '[:space:]' < "${work}/count")"

[[ "${streamed}" == "${total}" ]] \
    || die "the store half streamed ${streamed} bytes and its manifest says ${total}; no backup kept"

chmod 600 "${dump}.partial" "${objects}.partial"
mv "${dump}.partial" "${dump}"
mv "${objects}.partial" "${objects}"
completed=1

# ---------------------------------------------------------------------------
# 3. Retention, by file age, over both halves
# ---------------------------------------------------------------------------

find "${KELIR_BACKUP_DIR}" -maxdepth 1 -type f \
    \( -name '*.dump.age' -o -name '*.objects.age' \) \
    -mtime "+${KELIR_BACKUP_RETENTION_DAYS}" -delete

echo "kelir-backup: ${dump} and ${objects} written (${count} objects, ${total} bytes)"
