#!/usr/bin/env bash
#
# Probes encryption at rest (NFR-SEC-009, #778, ADR-0049 §5). Each check prints
# `ok` or `FAIL`, and any FAIL exits non-zero. Every check was seen red first;
# the pull request that added this records the runs.
#
#   encryption-at-rest.probe.sh objects <marker>...
#       Reads every file under the bucket's directory in MinIO's data volume
#       raw, from outside MinIO, and finds no marker. Asks the store whether
#       every object is SSE-S3. Store the markers through the API first: the
#       probe tests the write the product makes, not one `mc` makes.
#   encryption-at-rest.probe.sh backup <marker>
#       Runs kelir-backup as the unit runs it, reads both archives raw, checks
#       modes and command lines, restores both halves into scratch targets,
#       and checks retention, the upgrade's conversion and the refusals.
#   encryption-at-rest.probe.sh deploy
#       Runs deploy.sh against stubs and checks what it refuses, and that it
#       refuses before any pull, build or deploy.
#
# **It runs as root in a Linux container, never on the host it protects and
# never against the kelir-staging stack.** The container needs age, jq,
# pg_dump, pg_restore, psql, runuser, ps, strace, a Docker CLI and the Docker
# socket. `postgres:16` with `apt-get install age jq docker-cli procps strace`
# is one. `backup` also needs `--cap-add SYS_PTRACE`, so strace can name the
# file each write goes to:
#
#   docker run --rm --cap-add SYS_PTRACE -v /var/run/docker.sock:/var/run/docker.sock \
#     -v "$PWD:/work:ro" --volumes-from <project>-minio-1:ro \
#     --network <project>_default -e KELIR_BACKUP_NETWORK=<project>_default ... \
#     <that image> /work/deploy/staging/encryption-at-rest.probe.sh objects <marker>
#
# Settings:
#   KELIR_PROBE_ENV_FILE      the stack's .env, for MinIO's pair (objects, backup)
#   KELIR_BACKUP_NETWORK      the stack's network (objects, backup)
#   KELIR_BACKUP_MC_IMAGE     the pinned mc image (objects, backup)
#   KELIR_PROBE_MINIO_IMAGE   the pinned minio image, for the scratch store (backup)
#   KELIR_BACKUP_BUCKET       kelir
#   KELIR_PROBE_DATA_DIR      /data, where --volumes-from mounts MinIO's volume
#   PGHOST PGPORT PGUSER PGPASSWORD KELIR_DB_NAME   the database (backup)
#   KELIR_PROBE_BACKUP        default: kelir-backup.sh beside this file
#   KELIR_PROBE_RESTORE       default: kelir-restore-objects.sh beside this file
#   KELIR_PROBE_DEPLOY        default: deploy.sh beside this file

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKUP="${KELIR_PROBE_BACKUP:-${HERE}/kelir-backup.sh}"
RESTORE="${KELIR_PROBE_RESTORE:-${HERE}/kelir-restore-objects.sh}"
DEPLOY="${KELIR_PROBE_DEPLOY:-${HERE}/deploy.sh}"
BUCKET="${KELIR_BACKUP_BUCKET:-kelir}"
AGE_HEADER='age-encryption.org/v1'

failures=0
check() {
    local name="$1"
    shift
    if "$@"; then
        echo "  ok    ${name}"
    else
        echo "  FAIL  ${name}"
        failures=$(( failures + 1 ))
    fi
}

work="$(mktemp -d)"
scratch_store=''
cleanup() {
    [[ -n "${scratch_store}" ]] && docker rm -f -v "${scratch_store}" >/dev/null 2>&1
    rm -rf "${work}"
}
trap cleanup EXIT

env_value() { grep -E "^[[:space:]]*$1=" "$2" | tail -1 | cut -d= -f2-; }

# `mc` in a one-off container on the stack's network, MinIO's pair in its
# environment. Reads `MC_HOST_kelir` from this shell.
in_mc() {
    docker run --rm -i --network "${KELIR_BACKUP_NETWORK}" -e MC_HOST_kelir \
        --entrypoint bash "${KELIR_BACKUP_MC_IMAGE}" -c "$1"
}

store_host() {
    local env_file="$1" host="$2"
    export MC_HOST_kelir
    MC_HOST_kelir="http://$(env_value KELIR_MINIO_USER "${env_file}"):$(env_value KELIR_MINIO_PASSWORD "${env_file}")@${host}:9000"
}

# ---------------------------------------------------------------------------
# objects: AC4(a)
# ---------------------------------------------------------------------------

probe_objects() {
    (( $# > 0 )) || { echo "usage: $0 objects <marker>..."; exit 2; }
    local bucket_dir="${KELIR_PROBE_DATA_DIR:-/data}/${BUCKET}" marker files

    files="$(find "${bucket_dir}" -type f | wc -l)"
    echo "Reading ${files} files under ${bucket_dir} raw"
    check "the bucket's directory holds files to read" test "${files}" -gt 0

    for marker in "$@"; do
        check "no file under ${bucket_dir} holds '${marker}'" \
            bash -c '! grep -r -l -F -e "$1" "$2"' _ "${marker}" "${bucket_dir}"
    done

    store_host "${KELIR_PROBE_ENV_FILE}" minio
    local stats
    if ! stats="$(in_mc "mc stat --recursive --json kelir/${BUCKET}" < /dev/null)"; then
        check "the store answers mc stat" false
        echo "        ${stats:0:300}"
        return
    fi
    printf '%s\n' "${stats}" > "${work}/stats"
    echo "The store reports $(wc -l < "${work}/stats") objects"
    check "the store reports every object as SSE-S3" \
        bash -c '! grep -v -F "\"X-Amz-Server-Side-Encryption\":\"AES256\"" "$1"' _ "${work}/stats"

    # **The bucket encrypts what does not ask** (AC3). Every writer Kelir ships
    # asks, so no object above tells a bucket with the default from one
    # without it; the setting itself is the evidence.
    local default
    default="$(in_mc "mc encrypt info --json kelir/${BUCKET}" < /dev/null 2>&1 || true)"
    echo "        ${default:0:200}"
    check "the bucket encrypts by default (SSE-S3)" \
        bash -c '[[ "$1" == *"\"algorithm\":\"AES256\""* ]]' _ "${default}"
}

# ---------------------------------------------------------------------------
# backup: AC4(b), AC4(c), AC5's restore, AC6's conversion, AC9, AC11
# ---------------------------------------------------------------------------

run_backup() {
    local env_file="$1" dir="$2"
    shift 2
    KELIR_ENV_FILE="${env_file}" KELIR_BACKUP_DIR="${dir}" \
        KELIR_BACKUP_DB_OS_USER=postgres "${BACKUP}" "$@"
}

# Exits non-zero, and leaves the directory as it was: no file of the pair.
refused_and_wrote_nothing() {
    local dir="$1" before after
    shift
    before="$(ls -A "${dir}")"
    if "$@" > "${work}/refusal.log" 2>&1; then
        echo "        it exited 0:"; sed 's/^/        /' "${work}/refusal.log"
        return 1
    fi
    after="$(ls -A "${dir}")"
    [[ "${before}" == "${after}" ]] || { echo "        it left: $(comm -13 <(echo "${before}") <(echo "${after}"))"; return 1; }
    echo "        $(tail -1 "${work}/refusal.log")"
}

first_line_is_age() { [[ "$(head -c ${#AGE_HEADER} "$1")" == "${AGE_HEADER}" ]]; }
not_pgdmp() { [[ "$(head -c 5 "$1")" != "PGDMP" ]]; }
pg_restore_refuses() { ! pg_restore --list "$1" > /dev/null 2>&1; }
holds_no() { ! grep -q -F -e "$1" "$2"; }
mode_is() { [[ "$(stat -c '%U %a' "$2")" == "$1" ]]; }
count_rows() { psql -tA -d "$1" -c "SELECT (SELECT count(*) FROM documents) || ' documents, ' || (SELECT count(*) FROM attachments) || ' attachments'"; }

# **AC1's "not even briefly", read off the system calls** (#778). The backup
# runs under strace, and every regular file it wrote is listed with the first
# bytes written to it. A plaintext dump or object written to a file and removed
# again leaves nothing on disk for a later look to find; its write is in the
# trace. A FIFO the run made (`mknod S_IFIFO`) is a pipe, not the disk, so it
# is left out. A write whose file strace could not name is listed as one
# nobody can vouch for: without CAP_SYS_PTRACE that is every write `pg_dump`
# makes once it is `postgres`, and the check fails rather than pass blind.
first_writes() {
    awk '
        match($0, /mknod(at)?\([^"]*"[^"]+", S_IFIFO/) {
            path = substr($0, RSTART, RLENGTH)
            sub(/^[^"]*"/, "", path)
            sub(/", S_IFIFO$/, "", path)
            fifo[path] = 1
            next
        }
        match($0, /(write|pwrite64|writev)\([0-9]+<\/[^>]*>, (\[\{iov_base=)?"/) {
            call = substr($0, RSTART, RLENGTH)
            path = call
            sub(/^[a-z0-9]+\([0-9]+</, "", path)
            sub(/>, .*$/, "", path)
            if (path ~ /^\/(dev|proc|sys)\// || path in seen) next
            seen[path] = 1
            order[++n] = path
            bytes[path] = substr($0, RSTART + RLENGTH, 40)
        }
        /^[0-9]+ +(copy_file_range|sendfile|splice|tee)\(.*<\// { order[++n] = "unread:" $0 }
        match($0, /^[0-9]+ +(write|pwrite64|writev)\([0-9]+, /) {
            call = substr($0, 1, RLENGTH - 2)
            gsub(/ +/, " ", call)
            if (!(call in unnamed)) { unnamed[call] = 1; order[++n] = "a write strace could not name, by pid " call }
        }
        END {
            for (i = 1; i <= n; i++) {
                if (!(order[i] in fifo)) printf "%s\t%s\n", order[i], bytes[order[i]]
            }
        }' "$1"
}

# Every file the traced run wrote began as age ciphertext, except what AC1
# leaves readable: the store's listing and the manifest, which bash keeps in
# its here-string files (`sh-thd.*`), and the byte count. Those are names and
# sizes, never in the backup directory.
wrote_no_plaintext() {
    local trace="$1" dir="$2" path bytes name offenders=''
    while IFS=$'\t' read -r path bytes; do
        [[ -n "${path}" ]] || continue
        [[ "${bytes}" == "${AGE_HEADER}"* ]] && continue
        name="$(basename "${path}")"
        if [[ "${path}" != "${dir}/"* && ( "${name}" == sh-thd.* || "${name}" == count ) ]] \
            && [[ "${bytes}" == '{\"status\":'* || "${bytes}" =~ ^[0-9] ]]; then
            continue
        fi
        offenders+="        ${path} began \"${bytes}\""$'\n'
    done < <(first_writes "${trace}")
    [[ -z "${offenders}" ]] || { printf '%s' "${offenders}"; return 1; }
}

# AC11: the database first, then the store. Read off the same trace, by the
# first pg_dump and the first docker the run started.
dumped_before_listed() {
    local dumped listed
    dumped="$(grep -n -m1 -E 'execve\("[^"]*/pg_dump"' "$1" | cut -d: -f1)"
    listed="$(grep -n -m1 -E 'execve\("[^"]*/docker"' "$1" | cut -d: -f1)"
    echo "        first pg_dump at trace line ${dumped:-none}, first docker at ${listed:-none}"
    [[ -n "${dumped}" && -n "${listed}" ]] && (( dumped < listed ))
}

# AC9, without sampling: no program the run started was given MinIO's pair in
# its arguments. `-e MC_HOST_kelir` names the variable; a value after it would
# be the pair.
no_credential_in_argv() {
    ! grep -E '^[0-9]+ +execve\(' "$1" | grep -q -F -e 'MC_HOST_kelir=' -e "${2:0:12}"
}

# A restore that exits non-zero, given a store archive that is not whole.
restore_refuses() {
    if "$@" > "${work}/restore-refusal.log" 2>&1; then
        echo "        it exited 0:"; sed 's/^/        /' "${work}/restore-refusal.log"
        return 1
    fi
    echo "        $(tail -1 "${work}/restore-refusal.log")"
}

probe_backup() {
    local marker="${1:?usage: $0 backup <marker>}"
    local identity="${work}/identity" env_file="${work}/env" dir="${work}/backups"
    local db="${KELIR_DB_NAME:-kelir}" password

    age-keygen -o "${identity}" 2> /dev/null
    { cat "${KELIR_PROBE_ENV_FILE}"; echo; echo "KELIR_BACKUP_AGE_RECIPIENTS=$(age-keygen -y "${identity}")"; } > "${env_file}"
    password="$(env_value KELIR_MINIO_PASSWORD "${env_file}")"
    mkdir -m 755 "${dir}"

    # Retention, planted: a pair past 14 days and a pair inside them.
    for half in dump objects; do
        echo old > "${dir}/kelir-20200101T000000Z.${half}.age"
        touch -d '20 days ago' "${dir}/kelir-20200101T000000Z.${half}.age"
        echo recent > "${dir}/kelir-20200102T000000Z.${half}.age"
        touch -d '2 days ago' "${dir}/kelir-20200102T000000Z.${half}.age"
    done

    echo "Running ${BACKUP} as the unit runs it"
    # Every command line in this container and every container's, sampled
    # while the backup runs (AC9).
    ( while :; do ps -eo args; docker ps --no-trunc --format '{{.Command}}'; sleep 0.2; done ) \
        > "${work}/command-lines" 2>&1 &
    local sampler=$!
    strace -f -qq -y -s 64 -o "${work}/trace" \
        -e trace=execve,mknod,mknodat,write,pwrite64,writev,copy_file_range,sendfile,splice,tee \
        env KELIR_ENV_FILE="${env_file}" KELIR_BACKUP_DIR="${dir}" KELIR_BACKUP_DB_OS_USER=postgres "${BACKUP}" \
        | sed 's/^/        /'
    kill "${sampler}"

    local dumps objects stamp
    dumps="$(find "${dir}" -name 'kelir-*.dump.age' ! -name 'kelir-2020*')"
    stamp="$(basename "${dumps}" .dump.age)"
    objects="${dir}/${stamp}.objects.age"

    echo "AC4(b) the database half: $(basename "${dumps}")"
    check "one new pair under one stamp" test "$(wc -l <<< "${dumps}")" -eq 1 -a -f "${objects}"
    check "the dump archive's first line is ${AGE_HEADER}" first_line_is_age "${dumps}"
    check "the dump archive does not start PGDMP" not_pgdmp "${dumps}"
    check "pg_restore --list refuses the raw dump archive" pg_restore_refuses "${dumps}"

    echo "AC4(c) the store half: $(basename "${objects}")"
    check "the store archive's first line is ${AGE_HEADER}" first_line_is_age "${objects}"
    check "a raw read of the store archive finds no '${marker}'" holds_no "${marker}" "${objects}"

    echo "AC1 what the run wrote to a file, from its system calls ($(wc -l < "${work}/trace") traced)"
    check "every file the run wrote began as age ciphertext, but for names and sizes" \
        wrote_no_plaintext "${work}/trace" "${dir}"
    check "the database was dumped before the store was listed (AC11)" dumped_before_listed "${work}/trace"

    echo "AC9 who wrote it, and what a command line showed"
    check "the backup directory is root's, mode 700" mode_is "root 700" "${dir}"
    check "the dump archive is root's, mode 600" mode_is "root 600" "${dumps}"
    check "the store archive is root's, mode 600" mode_is "root 600" "${objects}"
    check "no command line showed MinIO's password ($(sort -u "${work}/command-lines" | wc -l) seen)" \
        holds_no "${password}" "${work}/command-lines"
    check "no program the run started had MinIO's pair in its arguments (traced)" \
        no_credential_in_argv "${work}/trace" "${password}"

    echo "AC11 retention, by file age, over both halves"
    check "the pair past 14 days is gone" test ! -e "${dir}/kelir-20200101T000000Z.dump.age" -a ! -e "${dir}/kelir-20200101T000000Z.objects.age"
    check "the pair inside 14 days is kept" test -f "${dir}/kelir-20200102T000000Z.dump.age" -a -f "${dir}/kelir-20200102T000000Z.objects.age"

    echo "AC5 both halves restore with the identity"
    psql -q -d postgres -c 'DROP DATABASE IF EXISTS kelir_probe_restore' -c 'CREATE DATABASE kelir_probe_restore'
    age --decrypt -i "${identity}" "${dumps}" | pg_restore --no-owner --dbname=kelir_probe_restore
    local source restored
    source="$(count_rows "${db}")"
    restored="$(count_rows kelir_probe_restore)"
    echo "        ${db}: ${source}; restored: ${restored}"
    check "the restored database holds the same rows" test "${source}" == "${restored}"

    # A scratch store under a key of its own: the restore needs the identity,
    # never the store key.
    scratch_store="kelir-probe-store-$$"
    docker run -d --name "${scratch_store}" --network "${KELIR_BACKUP_NETWORK}" \
        -e MINIO_ROOT_USER=probe -e MINIO_ROOT_PASSWORD="probe-$$-password" \
        -e MINIO_KMS_SECRET_KEY="probe:$(head -c 32 /dev/urandom | base64)" \
        "${KELIR_PROBE_MINIO_IMAGE}" server /data > /dev/null
    printf 'KELIR_MINIO_USER=probe\nKELIR_MINIO_PASSWORD=probe-%s-password\n' "$$" > "${work}/scratch-env"
    store_host "${work}/scratch-env" "${scratch_store}"
    in_mc "until mc ls kelir > /dev/null 2>&1; do sleep 1; done
           mc mb -q kelir/${BUCKET} && mc encrypt set sse-s3 kelir/${BUCKET} > /dev/null" < /dev/null

    KELIR_RESTORE_IDENTITY="${identity}" KELIR_ENV_FILE="${work}/scratch-env" \
        KELIR_BACKUP_MINIO_URL="http://${scratch_store}:9000" "${RESTORE}" "${objects}" | sed 's/^/        /'

    # Every attachment the restored rows name, read back and hashed inside one
    # mc container, against the row's own checksum.
    local mismatches
    mismatches="$(psql -tA -d kelir_probe_restore -c "SELECT substr(checksum, 8) || ' ' || storage_reference FROM attachments" \
        | in_mc "while read -r sum key; do
                     got=\$(mc cat \"kelir/${BUCKET}/\${key}\" | sha256sum | cut -c1-64)
                     [[ \"\${got}\" == \"\${sum}\" ]] || echo \"\${key}\"
                 done")"
    check "every restored attachment's sha256 equals its row's checksum" test -z "${mismatches}"

    # **The restore checks the manifest again as it reads** (settled point 5).
    # Each archive here is encrypted to the same recipient and differs from a
    # whole one in one way; the first is whole, so a refusal is the archive's.
    echo "Settled point 5 the restore refuses a store archive that is not whole"
    local recipient
    recipient="$(age-keygen -y "${identity}")"
    archive_of() { printf '%b' "$1" | age -r "${recipient}" > "${work}/crafted.age"; }
    restore_crafted() {
        KELIR_RESTORE_IDENTITY="${identity}" KELIR_ENV_FILE="${work}/scratch-env" \
            KELIR_BACKUP_MINIO_URL="http://${scratch_store}:9000" "${RESTORE}" "${work}/crafted.age"
    }
    archive_of 'kelir-objects 1\n2 10\n5 probe/a\n5 probe/b\nAAAAABBBBB'
    check "a whole archive restores" restore_crafted
    archive_of 'kelir-objects 1\n2 11\n5 probe/a\n5 probe/b\nAAAAABBBBB'
    check "sizes that do not add up to the header: refused" restore_refuses restore_crafted
    archive_of 'kelir-objects 1\n2 10\n5 probe/a\n5 probe/b\nAAAAABBB'
    check "an archive cut short inside an object: refused" restore_refuses restore_crafted
    archive_of 'kelir-objects 1\n2 10\n5 probe/a\n5 probe/b\nAAAAABBBBBX'
    check "bytes after the last object: refused" restore_refuses restore_crafted
    psql -q -d postgres -c 'DROP DATABASE kelir_probe_restore'

    echo "AC6 the upgrade's plaintext dumps"
    local convert="${work}/convert" plaintext_sum
    mkdir -m 700 "${convert}"
    runuser -u postgres -- pg_dump --format=custom "${db}" > "${convert}/kelir-20200103T000000Z.dump"
    touch -d '3 days ago' "${convert}/kelir-20200103T000000Z.dump"
    plaintext_sum="$(sha256sum < "${convert}/kelir-20200103T000000Z.dump")"
    run_backup "${env_file}" "${convert}" --encrypt-old-dumps | sed 's/^/        /'
    check "the plaintext dump is gone" test ! -e "${convert}/kelir-20200103T000000Z.dump"
    check "its .dump.age decrypts to the same bytes" \
        test "$(age --decrypt -i "${identity}" "${convert}/kelir-20200103T000000Z.dump.age" | sha256sum)" == "${plaintext_sum}"
    check "its date is kept, so retention counts from the day it was taken" \
        test "$(find "${convert}" -name '*.dump.age' -mtime +2 | wc -l)" -eq 1

    echo "AC11 it fails closed"
    grep -v '^KELIR_BACKUP_AGE_RECIPIENTS=' "${env_file}" > "${work}/no-recipient"
    echo 'KELIR_BACKUP_AGE_RECIPIENTS=' >> "${work}/no-recipient"
    check "no recipient: refused, and no file" \
        refused_and_wrote_nothing "${dir}" run_backup "${work}/no-recipient" "${dir}"
    check "a database it cannot dump: refused, and neither file of the pair" \
        refused_and_wrote_nothing "${dir}" env KELIR_DB_NAME=kelir_probe_no_such_database \
            KELIR_ENV_FILE="${env_file}" KELIR_BACKUP_DIR="${dir}" KELIR_BACKUP_DB_OS_USER=postgres "${BACKUP}"
    check "a store it cannot reach: refused, and neither file of the pair" \
        refused_and_wrote_nothing "${dir}" env KELIR_BACKUP_NETWORK=none \
            KELIR_ENV_FILE="${env_file}" KELIR_BACKUP_DIR="${dir}" KELIR_BACKUP_DB_OS_USER=postgres "${BACKUP}"

    # A stream one byte longer than its manifest: a stub `docker` in front of
    # the real one adds the byte to the `mc cat` container's output.
    mkdir "${work}/stub"
    printf '#!/usr/bin/env bash\n"%s" "$@"; status=$?\n[[ "$*" == *"mc cat"* ]] && printf x\nexit "${status}"\n' \
        "$(command -v docker)" > "${work}/stub/docker"
    chmod +x "${work}/stub/docker"
    check "bytes that do not add up to the manifest: refused, and neither file" \
        refused_and_wrote_nothing "${dir}" env PATH="${work}/stub:${PATH}" \
            KELIR_ENV_FILE="${env_file}" KELIR_BACKUP_DIR="${dir}" KELIR_BACKUP_DB_OS_USER=postgres "${BACKUP}"
}

# ---------------------------------------------------------------------------
# deploy: AC10, and AC11's pre-deploy backup
# ---------------------------------------------------------------------------

probe_deploy() {
    local app="${work}/app" bin="${work}/bin"
    mkdir -p "${app}" "${bin}"
    cp "${DEPLOY}" "${app}/deploy.sh"
    cp "$(dirname "${DEPLOY}")/.env.staging.example" "${app}/"
    touch "${app}/docker-compose.staging.yml"

    # Stubs. `docker` records every call and answers only `image inspect`, so
    # a deploy that gets past its checks reaches compose and stops there.
    cat > "${bin}/docker" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "${PROBE_DOCKER_LOG}"
[[ "$1 $2" == "image inspect" ]]
STUB
    cat > "${bin}/systemctl" <<'STUB'
#!/usr/bin/env bash
case "$1" in
    list-unit-files) [[ -n "${PROBE_UNIT_LISTED:-}" ]] ;;
    start) exec "${KELIR_BACKUP_COMMAND}" ;;
esac
STUB
    printf '#!/usr/bin/env bash\nexec "$@"\n' > "${bin}/sudo"
    printf '#!/usr/bin/env bash\nexit 1\n' > "${bin}/curl"
    printf '#!/usr/bin/env bash\nwhile [[ "$1" != -- ]]; do shift; done; shift; exec "$@"\n' > "${bin}/runuser"
    printf '#!/usr/bin/env bash\necho PGDMP-stub\n' > "${bin}/pg_dump"
    chmod +x "${bin}"/*

    local recipient
    recipient="$(age-keygen 2> /dev/null | grep -o 'age1[0-9a-z]*')"
    good_env() {
        cat <<ENV
KELIR_DB_PASSWORD=probe-database-password
KELIR_JWT_SECRET=probe-jwt-secret-0123456789
KELIR_MINIO_USER=probe-user
KELIR_MINIO_PASSWORD=probe-minio-password
KELIR_MINIO_KMS_SECRET_KEY=probe-store:$(head -c 32 /dev/urandom | base64)
KELIR_BACKUP_AGE_RECIPIENTS=${recipient}
KELIR_TRUSTED_PROXY_HOPS=1
KELIR_BOOTSTRAP_ADMIN_USERNAME=
KELIR_BOOTSTRAP_ADMIN_PASSWORD=
KELIR_BOOTSTRAP_ADMIN_EMAIL=
ENV
    }

    # deploy_with <env> <installed backup command or ''> <unit listed: 1 or ''>
    deploy_with() {
        printf '%s\n' "$1" > "${app}/.env"
        : > "${work}/docker.log"
        set +e
        PATH="${bin}:${PATH}" PROBE_DOCKER_LOG="${work}/docker.log" PROBE_UNIT_LISTED="$3" \
            KELIR_BACKUP_COMMAND="${2:-${work}/none}" KELIR_ENV_FILE="${app}/.env" \
            KELIR_BACKUP_DIR="${work}/deploy-backups" KELIR_BACKUP_NETWORK=none \
            KELIR_BACKUP_MC_IMAGE=probe-mc KELIR_APP_DIR="${app}" \
            bash "${app}/deploy.sh" 9.9.9 > "${work}/deploy.out" 2> "${work}/deploy.err"
        deploy_status=$?
        set -e
        echo "        exit ${deploy_status}: $({ grep -m1 'error' "${work}/deploy.err" || grep -m1 'backup' "${work}/deploy.err"; } | sed 's/\x1b\[[0-9;]*m//g')"
    }
    refused_before_docker() { (( deploy_status != 0 )) && [[ ! -s "${work}/docker.log" ]]; }

    printf '#!/usr/bin/env bash\n# the unit provision-ubuntu-24.sh wrote before #778\npg_dump --format=custom --file=/tmp/x.dump kelir\n' > "${work}/old-backup"
    chmod +x "${work}/old-backup"

    echo "AC10 deploy.sh refuses before any pull, build or deploy"
    deploy_with "$(good_env)" "${work}/old-backup" 1
    check "an installed kelir-backup from before #778" refused_before_docker
    deploy_with "$(good_env | grep -v '^KELIR_MINIO_KMS_SECRET_KEY=')" "${BACKUP}" 1
    check "a .env without KELIR_MINIO_KMS_SECRET_KEY" refused_before_docker
    deploy_with "$(good_env | sed 's/^KELIR_MINIO_KMS_SECRET_KEY=.*/KELIR_MINIO_KMS_SECRET_KEY=change-me/')" "${BACKUP}" 1
    check "a placeholder store key" refused_before_docker
    deploy_with "$(good_env | sed 's|^KELIR_MINIO_KMS_SECRET_KEY=.*|KELIR_MINIO_KMS_SECRET_KEY=kelir-development:w7GPk50m0UH2qkpB+MZbHBZD4Tcd0+nwJx3VzoopMkk=|')" "${BACKUP}" 1
    check "the published development store key" refused_before_docker

    echo "AC10 with no unit installed, it deploys without a backup and says so"
    deploy_with "$(good_env)" '' ''
    check "a warning on stderr names the missing pre-deploy backup" grep -q 'no pre-deploy backup is taken' "${work}/deploy.err"
    check "and it goes on to deploy" grep -q '^compose' "${work}/docker.log"

    echo "AC11 a pre-deploy backup whose store half fails stops the deploy"
    deploy_with "$(good_env)" "${BACKUP}" 1
    check "deploy.sh exits non-zero, before compose" \
        bash -c '(( $1 != 0 )) && grep -q "pre-deploy backup failed" "$2" && ! grep -q "^compose" "$3"' \
            _ "${deploy_status}" "${work}/deploy.err" "${work}/docker.log"
    check "and the failed run left no file" test -z "$(ls -A "${work}/deploy-backups" 2> /dev/null)"
}

case "${1:-}" in
    objects) shift; probe_objects "$@" ;;
    backup) shift; probe_backup "$@" ;;
    deploy) shift; probe_deploy "$@" ;;
    *) echo "usage: $0 objects <marker>... | backup <marker> | deploy"; exit 2 ;;
esac

if (( failures > 0 )); then
    echo "${failures} check(s) failed"
    exit 1
fi
echo "every check passed"
