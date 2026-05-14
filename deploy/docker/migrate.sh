#!/usr/bin/env bash
# feedbackmonk migrations runner — invoked as the `migrate` service entrypoint
# in deploy/docker/docker-compose.yml. Idempotent: sqlx migrate run skips
# migrations that have already been applied (tracked in the
# `_sqlx_migrations` table). Forward-only — there is no rollback.
#
# Inside the api image, migrations live at /app/migrations and sqlx-cli is
# at /usr/local/bin/sqlx.
#
# Standalone invocation (e.g., from an operator shell against an existing db):
#   docker compose run --rm migrate
#   # OR with an arbitrary DATABASE_URL:
#   DATABASE_URL=postgres://... ./migrate.sh

set -euo pipefail

if [ -z "${DATABASE_URL:-}" ]; then
    echo "[migrate] ERROR: DATABASE_URL is not set." >&2
    echo "[migrate] Set it in .env or export before invoking." >&2
    exit 1
fi

MIGRATIONS_DIR="${MIGRATIONS_DIR:-/app/migrations}"
if [ ! -d "${MIGRATIONS_DIR}" ]; then
    echo "[migrate] ERROR: migrations directory not found at ${MIGRATIONS_DIR}." >&2
    exit 1
fi

# Redact DATABASE_URL for logging (don't print passwords).
DB_REDACTED=$(printf '%s' "${DATABASE_URL}" | sed -E 's#(://[^:]+:)[^@]+(@)#\1***\2#')

echo "[migrate] Running sqlx migrations against ${DB_REDACTED}"
echo "[migrate] Source: ${MIGRATIONS_DIR}"

# sqlx-cli migrate run is idempotent. It applies any migration with a higher
# version than what's recorded in `_sqlx_migrations` and exits 0 if nothing
# is pending.
exec sqlx migrate run --source "${MIGRATIONS_DIR}" --database-url "${DATABASE_URL}"
