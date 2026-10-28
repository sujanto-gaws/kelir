#!/usr/bin/env bash
#
# kelir-restore-objects: writes a store archive back into an object store
# (ADR-0049 §2, #778). The database half is restored with pg_restore; the
# Installation guide §8 gives both.
#
#   KELIR_RESTORE_IDENTITY=/path/to/identity.txt \
#       kelir-restore-objects /var/backups/kelir/kelir-<stamp>.objects.age
#
# provision-ubuntu-24.sh installs it beside kelir-backup. **It needs the
# backup identity and not the store key**: it decrypts the archive with the
# identity and writes each object through the S3 API, asking the target store
# to encrypt it (SSE-S3) under whatever key that store holds. A store without
# a key refuses the first write.
#
# **The manifest is checked again as it is read**: its sizes must add up to
# its header, each object must arrive whole, and nothing may follow the last.
# A failure stops the restore with a non-zero exit. Objects written before it
# stay in the target; run it again over the same target once the cause is
# fixed, since each write replaces the object of that key.
#
# Settings, as kelir-backup takes them: KELIR_APP_DIR, KELIR_ENV_FILE (the
# target store's KELIR_MINIO_USER and KELIR_MINIO_PASSWORD), KELIR_COMPOSE_PROJECT,
# KELIR_BACKUP_NETWORK, KELIR_BACKUP_MINIO_URL, KELIR_BACKUP_BUCKET and
# KELIR_BACKUP_MC_IMAGE. The bucket must exist; the stack's minio-init makes it.

set -euo pipefail

KELIR_APP_DIR="${KELIR_APP_DIR:-/opt/kelir}"
KELIR_ENV_FILE="${KELIR_ENV_FILE:-${KELIR_APP_DIR}/.env}"
KELIR_COMPOSE_PROJECT="${KELIR_COMPOSE_PROJECT:-kelir-staging}"
KELIR_BACKUP_NETWORK="${KELIR_BACKUP_NETWORK:-${KELIR_COMPOSE_PROJECT}_default}"
KELIR_BACKUP_MINIO_URL="${KELIR_BACKUP_MINIO_URL:-http://minio:9000}"
KELIR_BACKUP_BUCKET="${KELIR_BACKUP_BUCKET:-kelir}"

die() { printf 'kelir-restore-objects: %s\n' "$*" >&2; exit 1; }

archive="${1:-}"
[[ $# -eq 1 && -r "${archive}" ]] || die "usage: KELIR_RESTORE_IDENTITY=<file> $0 <kelir-STAMP.objects.age>"
[[ -r "${KELIR_RESTORE_IDENTITY:-}" ]] || die "KELIR_RESTORE_IDENTITY must name the age identity file"
[[ -r "${KELIR_ENV_FILE}" ]] || die "cannot read ${KELIR_ENV_FILE}"

for required in age docker; do
    command -v "${required}" >/dev/null 2>&1 || die "${required} is required but not installed"
done

# As kelir-backup reads them: text, never sourced.
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

minio_user="$(env_value KELIR_MINIO_USER)"
minio_password="$(env_value KELIR_MINIO_PASSWORD)"
[[ -n "${minio_user}" && -n "${minio_password}" ]] \
    || die "KELIR_MINIO_USER or KELIR_MINIO_PASSWORD is empty in ${KELIR_ENV_FILE}"
# As kelir-backup: `mc` reads the pair as written, so neither may hold the
# characters it splits at.
[[ "${minio_user}" != *[:@]* && "${minio_password}" != *@* ]] \
    || die "KELIR_MINIO_USER holds ':' or '@', or KELIR_MINIO_PASSWORD holds '@'; mc cannot be given that pair"

mc_image="${KELIR_BACKUP_MC_IMAGE:-}"
if [[ -z "${mc_image}" && -r "${KELIR_APP_DIR}/docker-compose.staging.yml" ]]; then
    mc_image="$(grep -oE 'ghcr\.io/sujanto-gaws/mc:[^[:space:]]+' \
        "${KELIR_APP_DIR}/docker-compose.staging.yml" | head -1)"
fi
[[ -n "${mc_image}" ]] || die "no mc image: set KELIR_BACKUP_MC_IMAGE, or keep docker-compose.staging.yml in ${KELIR_APP_DIR}"

scheme="${KELIR_BACKUP_MINIO_URL%%://*}"
export MC_HOST_kelir="${scheme}://${minio_user}:${minio_password}@${KELIR_BACKUP_MINIO_URL#*://}"

# The reader runs inside one `mc` container, so the archive costs one container
# start however many objects it holds. GNU `head -c` reads no further than it
# is asked, which is what lets one stream be cut into objects.
# shellcheck disable=SC2016
reader='set -euo pipefail
fail() { printf "kelir-restore-objects: %s\n" "$*" >&2; exit 1; }

IFS= read -r magic || fail "the archive is empty"
[[ "${magic}" == "kelir-objects 1" ]] || fail "not a store archive: it starts \"${magic}\""
read -r count total || fail "the archive has no counts"
[[ "${count}" =~ ^[0-9]+$ && "${total}" =~ ^[0-9]+$ ]] || fail "unreadable counts: ${count} ${total}"

sizes=()
keys=()
sum=0
for (( index = 0; index < count; index++ )); do
    IFS= read -r line || fail "the manifest ends after ${index} of ${count} entries"
    size="${line%% *}"
    key="${line#* }"
    [[ "${size}" =~ ^[0-9]+$ && -n "${key}" && "${key}" != "${line}" ]] \
        || fail "manifest entry $(( index + 1 )) is unreadable: ${line}"
    sizes+=("${size}")
    keys+=("${key}")
    sum=$(( sum + size ))
done
(( sum == total )) || fail "the manifest adds up to ${sum} bytes and its header says ${total}"

counted="$(mktemp -u)"
mkfifo "${counted}"
for (( index = 0; index < count; index++ )); do
    wc -c < "${counted}" > "${counted}.n" &
    head -c "${sizes[index]}" | tee "${counted}" \
        | mc pipe --quiet --enc-s3 "kelir/${KELIR_BACKUP_BUCKET}" \
            "kelir/${KELIR_BACKUP_BUCKET}/${keys[index]}" > /dev/null
    wait $!
    [[ "$(< "${counted}.n")" == "${sizes[index]}" ]] \
        || fail "${keys[index]} arrived with $(< "${counted}.n") of ${sizes[index]} bytes; the archive is cut short"
done

[[ "$(head -c 1 | wc -c)" == "0" ]] || fail "bytes follow the last object in the manifest"
echo "kelir-restore-objects: ${count} objects, ${total} bytes, written to ${KELIR_BACKUP_BUCKET}"'

age --decrypt -i "${KELIR_RESTORE_IDENTITY}" "${archive}" \
    | docker run --rm -i --network "${KELIR_BACKUP_NETWORK}" \
        -e MC_HOST_kelir -e KELIR_BACKUP_BUCKET="${KELIR_BACKUP_BUCKET}" \
        --entrypoint bash "${mc_image}" -c "${reader}"
