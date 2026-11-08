#!/usr/bin/env bash
#
# Seeds what encryption-at-rest.probe.sh's `backup` mode reads, and checks
# that it did (#807, AC-41-4). CI runs it before the probe; so can anyone
# running the probe by hand against a scratch database and store.
#
#   encryption-at-rest.seed.sh <marker>
#
# **Why the probe needs it.** The probe compares: the restored database holds
# the same rows as the source, every restored attachment hashes to its row's
# checksum, the store archive holds no marker. Over an empty database and an
# empty bucket every one of those is true with encryption off, so a probe run
# against nothing proves nothing. This puts one of each in place:
#
#   - an object in the bucket whose bytes carry <marker>, written through the
#     S3 API with `--enc-s3`, as the backend writes (ADR-0049 §2);
#   - a tenant, a document type, a document and an attachment row whose
#     `checksum` and `storage_reference` are that object's, in the shapes the
#     backend records them (`sha256:<hex>`, `tenants/<t>/documents/<d>/...`).
#
# **And it reads them back before it exits**: the store reports the object
# SSE-S3 and serves its bytes, the bucket encrypts by default, the listing is
# not empty, and the attachment row joins its document. Any of those missing
# exits non-zero, so the probe never starts on a seed that did not land.
#
# Settings, as the probe and kelir-backup take them:
#   PGHOST PGPORT PGUSER PGPASSWORD KELIR_DB_NAME   a migrated, scratch database
#   KELIR_PROBE_ENV_FILE      KELIR_MINIO_USER and KELIR_MINIO_PASSWORD
#   KELIR_BACKUP_NETWORK      the store's Docker network
#   KELIR_BACKUP_MC_IMAGE     the pinned mc image
#   KELIR_BACKUP_MINIO_URL    http://minio:9000
#   KELIR_BACKUP_BUCKET       kelir
#
# **Never against a database or store anybody uses**: it inserts a tenant.

set -euo pipefail

marker="${1:?usage: $0 <marker>}"
db="${KELIR_DB_NAME:-kelir}"
bucket="${KELIR_BACKUP_BUCKET:-kelir}"
url="${KELIR_BACKUP_MINIO_URL:-http://minio:9000}"

die() { printf 'encryption-at-rest.seed: %s\n' "$*" >&2; exit 1; }
env_value() { grep -E "^[[:space:]]*$1=" "${KELIR_PROBE_ENV_FILE}" | tail -1 | cut -d= -f2-; }

[[ -r "${KELIR_PROBE_ENV_FILE:-}" ]] || die "KELIR_PROBE_ENV_FILE must name the store's env file"
[[ -n "${KELIR_BACKUP_NETWORK:-}" && -n "${KELIR_BACKUP_MC_IMAGE:-}" ]] \
    || die "KELIR_BACKUP_NETWORK and KELIR_BACKUP_MC_IMAGE must be set"

export MC_HOST_kelir
MC_HOST_kelir="${url%%://*}://$(env_value KELIR_MINIO_USER):$(env_value KELIR_MINIO_PASSWORD)@${url#*://}"

# The pair travels in the environment, never on a command line, as in
# kelir-backup.
in_mc() {
    docker run --rm -i --network "${KELIR_BACKUP_NETWORK}" -e MC_HOST_kelir \
        -e BUCKET="${bucket}" --entrypoint bash "${KELIR_BACKUP_MC_IMAGE}" -c "$1"
}

sql() { psql -X -v ON_ERROR_STOP=1 -tA -d "${db}" "$@"; }

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

read -r tenant type document attachment < <(sql -c \
    "SELECT gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), gen_random_uuid()" | tr '|' ' ')
[[ -n "${attachment}" ]] || die "the database gave no ids; is ${db} reachable?"

name='encryption-at-rest-probe.txt'
key="tenants/${tenant}/documents/${document}/attachments/${attachment}/${name}"
printf 'Kelir encryption-at-rest probe object.\nmarker: %s\n' "${marker}" > "${work}/object"
size="$(wc -c < "${work}/object" | tr -d ' ')"
checksum="sha256:$(sha256sum < "${work}/object" | cut -c1-64)"

echo "Storing ${key} (${size} bytes, ${checksum}) with --enc-s3"
in_mc "mc pipe --quiet --enc-s3 \"kelir/\${BUCKET}\" \"kelir/\${BUCKET}/${key}\" > /dev/null" < "${work}/object"

echo "Recording tenant TNT-PROBE, DOC-2026-000001 and its attachment in ${db}"
sql -q <<SQL
BEGIN;
INSERT INTO tenants (id, tenant_code, name)
    VALUES ('${tenant}', 'TNT-PROBE', 'Encryption-at-rest probe');
INSERT INTO document_types (id, tenant_id, type_code, name)
    VALUES ('${type}', '${tenant}', 'PROBE_DOCUMENT', 'Probe document');
INSERT INTO documents (id, tenant_id, document_ref, document_type_id, title)
    VALUES ('${document}', '${tenant}', 'DOC-2026-000001', '${type}', 'A document with an attachment');
INSERT INTO attachments (id, tenant_id, document_id, file_name, original_file_name, mime_type,
                         file_size, checksum, storage_reference, virus_scan_status)
    VALUES ('${attachment}', '${tenant}', '${document}', '${name}', '${name}', 'text/plain',
            ${size}, '${checksum}', '${key}', 'CLEAN');
COMMIT;
SQL

# ---------------------------------------------------------------------------
# What was seeded, read back
# ---------------------------------------------------------------------------

failures=0
check() {
    local name="$1"
    shift
    if "$@"; then echo "  ok    ${name}"; else echo "  FAIL  ${name}"; failures=$(( failures + 1 )); fi
}

stat_json="$(in_mc "mc stat --json \"kelir/\${BUCKET}/${key}\"" < /dev/null)" || stat_json=''
check "the store holds the object" test -n "${stat_json}"
check "the store reports it SSE-S3" \
    bash -c '[[ "$1" == *"\"X-Amz-Server-Side-Encryption\":\"AES256\""* ]]' _ "${stat_json}"
served="$(in_mc "mc cat \"kelir/\${BUCKET}/${key}\"" < /dev/null | sha256sum | cut -c1-64)" || served=''
check "the store serves bytes that hash to the row's checksum" test "sha256:${served}" == "${checksum}"
check "the object carries the marker" grep -q -F -e "${marker}" "${work}/object"
listed="$(in_mc 'mc ls --recursive --json "kelir/${BUCKET}"' < /dev/null | grep -c '"type":"file"')" || listed=0
check "the bucket lists ${listed} object(s), more than none" test "${listed}" -gt 0
default="$(in_mc 'mc encrypt info --json "kelir/${BUCKET}"' < /dev/null 2>&1)" || default=''
check "the bucket encrypts by default (SSE-S3)" \
    bash -c '[[ "$1" == *"\"algorithm\":\"AES256\""* ]]' _ "${default}"
rows="$(sql -c "SELECT count(*) FROM attachments a JOIN documents d ON d.id = a.document_id
                 WHERE a.checksum = '${checksum}' AND a.storage_reference = '${key}'")"
check "one attachment row names the object, under a document" test "${rows}" -eq 1
check "the database holds documents and attachments" \
    test "$(sql -c 'SELECT least((SELECT count(*) FROM documents), (SELECT count(*) FROM attachments))')" -gt 0

if (( failures > 0 )); then
    echo "${failures} seed check(s) failed"
    exit 1
fi
echo "seeded"
