#!/usr/bin/env bash
#
# One-time provisioning for kelir-staging-01 — Ubuntu Server 24.04 LTS (noble).
#
# Installs Docker Engine and PostgreSQL, with PostgreSQL running natively on the
# host rather than in a container. The application containers reach it over the
# Docker bridge gateway.
#
# Native PostgreSQL means the data directory is an ordinary path on the host, so
# the backup and restore drill that NFR-AVA-004 requires (RPO 24 h, RTO 4 h) is a
# real exercise rather than a volume copy. This script installs the daily dump
# timer that drill depends on.
#
# Run once, as root, on a fresh host:
#   sudo ./provision-ubuntu-24.sh
#
# Idempotent: safe to re-run after a failure or to pick up a changed setting.

set -euo pipefail

KELIR_DB_NAME="${KELIR_DB_NAME:-kelir}"
KELIR_DB_USER="${KELIR_DB_USER:-kelir}"
# The role the backend serves as (ADR-0051): owns nothing, may not change
# audit_events. KELIR_DB_USER owns the schema and only migrates.
KELIR_DB_APP_USER="${KELIR_DB_APP_USER:-kelir_app}"
KELIR_APP_DIR="${KELIR_APP_DIR:-/opt/kelir}"
KELIR_BACKUP_DIR="${KELIR_BACKUP_DIR:-/var/backups/kelir}"
KELIR_BACKUP_RETENTION_DAYS="${KELIR_BACKUP_RETENTION_DAYS:-14}"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ ${EUID} -eq 0 ]] || die "run as root: sudo $0"

# ---------------------------------------------------------------------------
# 0. Preconditions
# ---------------------------------------------------------------------------

log "Checking the host"

if [[ -r /etc/os-release ]]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    [[ "${ID:-}" == "ubuntu" ]] || warn "expected Ubuntu, found '${ID:-unknown}' — continuing anyway"
    [[ "${VERSION_ID:-}" == "24.04" ]] \
        || warn "expected Ubuntu 24.04, found '${VERSION_ID:-unknown}' — package names may differ"
else
    warn "/etc/os-release is missing; cannot confirm the distribution"
fi

export DEBIAN_FRONTEND=noninteractive

# ---------------------------------------------------------------------------
# 1. Base packages
# ---------------------------------------------------------------------------

log "Installing base packages"
apt-get update -qq
apt-get install -y -qq ca-certificates curl gnupg ufw jq age

# ---------------------------------------------------------------------------
# 2. Docker Engine, from Docker's own repository
# ---------------------------------------------------------------------------

if command -v docker >/dev/null 2>&1; then
    log "Docker is already installed ($(docker --version))"
else
    log "Installing Docker Engine"

    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
        -o /etc/apt/keyrings/docker.asc
    chmod a+r /etc/apt/keyrings/docker.asc

    cat > /etc/apt/sources.list.d/docker.list <<EOF
deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu ${VERSION_CODENAME} stable
EOF

    apt-get update -qq
    apt-get install -y -qq \
        docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

    systemctl enable --now docker
fi

# ---------------------------------------------------------------------------
# 3. PostgreSQL, natively on the host
# ---------------------------------------------------------------------------

log "Installing PostgreSQL"
apt-get install -y -qq postgresql postgresql-contrib

PG_VERSION="$(psql --version | grep -oE '[0-9]+' | head -1)"
PG_CONF_DIR="/etc/postgresql/${PG_VERSION}/main"
[[ -d "${PG_CONF_DIR}" ]] || die "PostgreSQL config directory not found at ${PG_CONF_DIR}"

log "PostgreSQL ${PG_VERSION} configuration in ${PG_CONF_DIR}"

systemctl enable --now postgresql

# The application containers connect over the Docker bridge, so PostgreSQL has
# to accept connections on more than the loopback interface. Exposure is
# constrained by pg_hba (below) and by the firewall (§5) — not by the bind
# address alone, which is why '*' here is not the risk it looks like.
if ! grep -qE "^listen_addresses\s*=\s*'\*'" "${PG_CONF_DIR}/postgresql.conf"; then
    log "Setting listen_addresses"
    sed -i "s/^#\?listen_addresses.*/listen_addresses = '*'\t\t# kelir: reachable from the docker bridge/" \
        "${PG_CONF_DIR}/postgresql.conf"
    PG_NEEDS_RESTART=1
fi

# Only the loopback and Docker's private address pool may authenticate, and only
# with scram-sha-256. Docker allocates compose networks from 172.16.0.0/12.
# Two roles, each on its own marked line, so a host provisioned before ADR-0051
# gains the serving role's line on a re-run.
HBA_MARKER="# kelir: application containers over the docker bridge"
if ! grep -qF "${HBA_MARKER}" "${PG_CONF_DIR}/pg_hba.conf"; then
    log "Allowing the Docker bridge in pg_hba.conf"
    cat >> "${PG_CONF_DIR}/pg_hba.conf" <<EOF

${HBA_MARKER}
host    ${KELIR_DB_NAME}    ${KELIR_DB_USER}    172.16.0.0/12    scram-sha-256
EOF
    PG_NEEDS_RESTART=1
fi

HBA_APP_MARKER="# kelir: the serving role over the docker bridge (ADR-0051)"
if ! grep -qF "${HBA_APP_MARKER}" "${PG_CONF_DIR}/pg_hba.conf"; then
    log "Allowing the serving role in pg_hba.conf"
    cat >> "${PG_CONF_DIR}/pg_hba.conf" <<EOF

${HBA_APP_MARKER}
host    ${KELIR_DB_NAME}    ${KELIR_DB_APP_USER}    172.16.0.0/12    scram-sha-256
EOF
    PG_NEEDS_RESTART=1
fi

if [[ "${PG_NEEDS_RESTART:-0}" == "1" ]]; then
    log "Restarting PostgreSQL to apply the configuration"
    systemctl restart postgresql
fi

# ---------------------------------------------------------------------------
# 4. Role and database
# ---------------------------------------------------------------------------

# The password is read from the deployment env file so it exists in exactly one
# place. Generated here on first run if that file has not been written yet.
ENV_FILE="${KELIR_APP_DIR}/.env"
mkdir -p "${KELIR_APP_DIR}"

# Reads a password from the env file, or generates one and records it there.
# The log goes to stderr, because the value is this function's output.
env_password() {
    local key="$1" value
    if [[ -f "${ENV_FILE}" ]] && grep -qE "^${key}=.+" "${ENV_FILE}"; then
        value="$(grep -E "^${key}=" "${ENV_FILE}" | head -1 | cut -d= -f2-)"
        log "Using ${key} already in ${ENV_FILE}" >&2
    else
        value="$(openssl rand -base64 36 | tr -d '\n/+=' | head -c 40)"
        log "Generated ${key} and recorded it in ${ENV_FILE}" >&2
        touch "${ENV_FILE}"
        chmod 600 "${ENV_FILE}"
        if grep -qE "^${key}=" "${ENV_FILE}"; then
            sed -i "s|^${key}=.*|${key}=${value}|" "${ENV_FILE}"
        else
            printf '%s=%s\n' "${key}" "${value}" >> "${ENV_FILE}"
        fi
    fi
    printf '%s' "${value}"
}

DB_PASSWORD="$(env_password KELIR_DB_PASSWORD)"
DB_APP_PASSWORD="$(env_password KELIR_DB_APP_PASSWORD)"

log "Ensuring the ${KELIR_DB_USER} role and ${KELIR_DB_NAME} database exist"

role_exists="$(sudo -u postgres psql -tAc \
    "SELECT 1 FROM pg_roles WHERE rolname = '${KELIR_DB_USER}'")"

if [[ "${role_exists}" == "1" ]]; then
    sudo -u postgres psql -q -c \
        "ALTER ROLE ${KELIR_DB_USER} WITH LOGIN PASSWORD '${DB_PASSWORD}'"
else
    sudo -u postgres psql -q -c \
        "CREATE ROLE ${KELIR_DB_USER} WITH LOGIN PASSWORD '${DB_PASSWORD}'"
fi

db_exists="$(sudo -u postgres psql -tAc \
    "SELECT 1 FROM pg_database WHERE datname = '${KELIR_DB_NAME}'")"

if [[ "${db_exists}" != "1" ]]; then
    sudo -u postgres createdb -O "${KELIR_DB_USER}" "${KELIR_DB_NAME}"
fi

# The owner owns the schema: it creates every table through migrations.
sudo -u postgres psql -q -d "${KELIR_DB_NAME}" -c \
    "GRANT ALL ON SCHEMA public TO ${KELIR_DB_USER}"

# **The serving role** (ADR-0051 §2): LOGIN, not a superuser, owner of nothing,
# member of no role, no CREATE on the database or on schema public. It reads
# and writes every table but two, by grant now and by default privileges for
# the tables later migrations create. The two REVOKEs are the backend's on
# every start too; they are here so the host is right before the first one.
# Idempotent, so a v0.9.0 host gains the role on a re-run (CHANGELOG.md).
log "Ensuring the ${KELIR_DB_APP_USER} role, which the backend serves as"

app_role_exists="$(sudo -u postgres psql -tAc \
    "SELECT 1 FROM pg_roles WHERE rolname = '${KELIR_DB_APP_USER}'")"

if [[ "${app_role_exists}" == "1" ]]; then
    sudo -u postgres psql -q -c \
        "ALTER ROLE ${KELIR_DB_APP_USER} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD '${DB_APP_PASSWORD}'"
else
    sudo -u postgres psql -q -c \
        "CREATE ROLE ${KELIR_DB_APP_USER} WITH LOGIN PASSWORD '${DB_APP_PASSWORD}'"
fi

# One transaction: a running kelir_app never sees the moment between the
# GRANT ON ALL TABLES and the two REVOKEs, when it could write _sqlx_migrations.
sudo -u postgres psql -q -v ON_ERROR_STOP=1 -d "${KELIR_DB_NAME}" <<SQL
BEGIN;
GRANT CONNECT ON DATABASE ${KELIR_DB_NAME} TO ${KELIR_DB_APP_USER};
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO ${KELIR_DB_APP_USER};
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO ${KELIR_DB_APP_USER};
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO ${KELIR_DB_APP_USER};
DO \$\$
BEGIN
    IF to_regclass('public.audit_events') IS NOT NULL THEN
        REVOKE UPDATE, DELETE, TRUNCATE ON audit_events FROM ${KELIR_DB_APP_USER};
    END IF;
    IF to_regclass('public._sqlx_migrations') IS NOT NULL THEN
        REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON _sqlx_migrations FROM ${KELIR_DB_APP_USER};
    END IF;
END
\$\$;
ALTER DEFAULT PRIVILEGES FOR ROLE ${KELIR_DB_USER} IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ${KELIR_DB_APP_USER};
ALTER DEFAULT PRIVILEGES FOR ROLE ${KELIR_DB_USER} IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO ${KELIR_DB_APP_USER};
COMMIT;
SQL

# ---------------------------------------------------------------------------
# 5. Firewall
# ---------------------------------------------------------------------------

log "Configuring the firewall"

ufw allow 22/tcp   >/dev/null   # SSH
ufw allow 80/tcp   >/dev/null   # HTTP — required for the ACME challenge
ufw allow 443/tcp  >/dev/null   # HTTPS

# PostgreSQL is deliberately absent: it must never be reachable from outside the
# host. Container traffic arrives over the Docker bridge, which UFW does not
# filter, so no rule is needed for it either.
ufw --force enable >/dev/null

# ---------------------------------------------------------------------------
# 6. Daily backup (NFR-AVA-004: daily automated backups, RPO <= 24 h)
# ---------------------------------------------------------------------------

#
# **Each backup is a pair, encrypted to the operator's age recipients** (#778,
# ADR-0049 §2): `kelir-<stamp>.dump.age`, the database, and
# `kelir-<stamp>.objects.age`, the object store. The commands are files beside
# this script, installed rather than written from a heredoc, so they can be
# run and probed elsewhere (`encryption-at-rest.probe.sh`).
#
# **The unit runs as root.** It reads MinIO's pair from .env (root, 600) and
# reaches MinIO through Docker; `postgres` can do neither, and putting it in
# the `docker` group would make it root-equivalent without letting it read
# .env. `pg_dump` still runs as `postgres`, through `runuser`, so peer
# authentication is unchanged.

log "Installing the daily backup timer"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for command_file in kelir-backup.sh kelir-restore-objects.sh; do
    [[ -f "${SCRIPT_DIR}/${command_file}" ]] \
        || die "${command_file} must sit beside this script (copy deploy/staging/ whole)"
done

install -m 0755 -o root -g root "${SCRIPT_DIR}/kelir-backup.sh" /usr/local/bin/kelir-backup
install -m 0755 -o root -g root "${SCRIPT_DIR}/kelir-restore-objects.sh" /usr/local/bin/kelir-restore-objects

# Root's now, where it was postgres's: the archives are written by root.
mkdir -p "${KELIR_BACKUP_DIR}"
chown -R root:root "${KELIR_BACKUP_DIR}"
chmod 700 "${KELIR_BACKUP_DIR}"

# **The upgrade leaves no plaintext dump** (#778, AC6). A pre-#778 unit wrote
# `<db>-<stamp>.dump` in the clear; each becomes `<name>.age`, with its date
# kept for retention, once the recipients are in .env. Removing the plaintext
# file does not scrub its blocks (Installation guide §8).
if compgen -G "${KELIR_BACKUP_DIR}/*.dump" >/dev/null; then
    if grep -qE '^[[:space:]]*KELIR_BACKUP_AGE_RECIPIENTS=.*age1' "${ENV_FILE}"; then
        log "Encrypting the plaintext dumps an earlier unit left"
        KELIR_APP_DIR="${KELIR_APP_DIR}" KELIR_BACKUP_DIR="${KELIR_BACKUP_DIR}" \
            /usr/local/bin/kelir-backup --encrypt-old-dumps
    else
        die "${KELIR_BACKUP_DIR} holds plaintext dumps; set KELIR_BACKUP_AGE_RECIPIENTS in ${ENV_FILE} and run this again to encrypt them"
    fi
fi

grep -qE '^[[:space:]]*KELIR_BACKUP_AGE_RECIPIENTS=.*age1' "${ENV_FILE}" \
    || warn "KELIR_BACKUP_AGE_RECIPIENTS is not set in ${ENV_FILE}; every backup fails, and so does every deploy, until it is"

cat > /etc/systemd/system/kelir-backup.service <<EOF
[Unit]
Description=Kelir backup: the database and the object store, encrypted
After=postgresql.service docker.service
Requires=postgresql.service
Wants=docker.service

[Service]
Type=oneshot
User=root
Environment=KELIR_APP_DIR=${KELIR_APP_DIR}
Environment=KELIR_BACKUP_DIR=${KELIR_BACKUP_DIR}
Environment=KELIR_BACKUP_RETENTION_DAYS=${KELIR_BACKUP_RETENTION_DAYS}
Environment=KELIR_DB_NAME=${KELIR_DB_NAME}
ExecStart=/usr/local/bin/kelir-backup
EOF

cat > /etc/systemd/system/kelir-backup.timer <<'EOF'
[Unit]
Description=Daily Kelir backup

[Timer]
OnCalendar=*-*-* 02:30:00
Persistent=true
RandomizedDelaySec=15m

[Install]
WantedBy=timers.target
EOF

systemctl daemon-reload
systemctl enable --now kelir-backup.timer

# ---------------------------------------------------------------------------
# 7. Summary
# ---------------------------------------------------------------------------

DOCKER_GATEWAY="$(ip -4 addr show docker0 | grep -oP '(?<=inet\s)\d+(\.\d+){3}' || echo '172.17.0.1')"

cat <<EOF

$(printf '\033[1;32m==> kelir-staging-01 provisioned\033[0m')

  Docker            $(docker --version)
  PostgreSQL        ${PG_VERSION}, listening for the docker bridge (${DOCKER_GATEWAY})
  Database          ${KELIR_DB_NAME}, owned by ${KELIR_DB_USER}, served as ${KELIR_DB_APP_USER}
  Application dir   ${KELIR_APP_DIR}
  Backups           ${KELIR_BACKUP_DIR}, daily 02:30 UTC, kept ${KELIR_BACKUP_RETENTION_DAYS} days,
                    the database and the object store, encrypted with age
  Firewall          22, 80, 443 open; PostgreSQL not exposed

Next:

  1. Copy deploy/staging/{docker-compose.staging.yml,Caddyfile,deploy.sh,.env.staging.example} to ${KELIR_APP_DIR}
  2. Fill in the remaining secrets in ${ENV_FILE}
     (KELIR_DB_PASSWORD and KELIR_DB_APP_PASSWORD are already set; generate KELIR_JWT_SECRET, the MinIO
     pair and KELIR_MINIO_KMS_SECRET_KEY, and add KELIR_BACKUP_AGE_RECIPIENTS
     from an identity made on another machine: Installation guide §6.3)
  3. Point staging.kelir.gawshub.com at this host, then:
       cd ${KELIR_APP_DIR} && ./deploy.sh 0.1.0

Verify the backup path before relying on it:

  sudo systemctl start kelir-backup && ls -l ${KELIR_BACKUP_DIR}

EOF
