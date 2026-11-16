#!/usr/bin/env bash
# The serving role of a load stack's database (ADR-0051, #984), made by the
# postgres image's entrypoint on the volume's first start, as POSTGRES_USER,
# which owns the schema and migrates. The same statements as the provisioner's
# and CI's `Provision the serving role`, with this database's owner and name.
#
# `race` drops and makes the database again, and default privileges are kept
# per database, so a race round does not have them.
set -eo pipefail

psql -X -q -v ON_ERROR_STOP=1 --username "${POSTGRES_USER}" --dbname "${POSTGRES_DB}" \
  -v app_password="${KELIR_DB_APP_PASSWORD}" -v owner="${POSTGRES_USER}" -v database="${POSTGRES_DB}" <<'SQL'
CREATE ROLE kelir_app WITH LOGIN PASSWORD :'app_password';
GRANT CONNECT ON DATABASE :"database" TO kelir_app;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO kelir_app;
ALTER DEFAULT PRIVILEGES FOR ROLE :"owner" IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO kelir_app;
ALTER DEFAULT PRIVILEGES FOR ROLE :"owner" IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO kelir_app;
SQL
