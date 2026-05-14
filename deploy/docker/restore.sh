#!/usr/bin/env bash
# feedbackmonk operator-side restore script.
#
# Reads a gzipped pg_dump from stdin and pipes it into psql in the `db`
# service container. Idempotent if the dump was created with --clean
# --if-exists (which backup.sh emits): DROP existing objects + recreate
# from the dump.
#
# USAGE:
#   cd deploy/docker
#   gunzip -c backups/feedbackmonk-20260514.sql.gz | ./restore.sh
#
# SAFETY: --clean --if-exists in the dump means this WILL drop and
# recreate every table, sequence, function, etc. in the database. The
# `--force` flag is required to acknowledge this destructive intent.
# Without --force, restore.sh prompts interactively.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

if [ ! -f .env ]; then
    echo "[restore] ERROR: .env not found in $(pwd)." >&2
    exit 1
fi

FORCE=0
for arg in "$@"; do
    case "${arg}" in
        --force) FORCE=1 ;;
        -h|--help)
            cat <<EOF
Usage: gunzip -c BACKUP.sql.gz | $0 [--force]

Restores a feedbackmonk database dump into the running 'db' service.

  --force     Skip the interactive confirmation. Required for non-TTY
              (cron / systemd-timer) invocation.

The backup is read from stdin (use gunzip -c to decompress on the fly).
EOF
            exit 0
            ;;
    esac
done

if [ "${FORCE}" -ne 1 ]; then
    if [ ! -t 0 ]; then
        echo "[restore] ERROR: stdin is not a TTY but --force was not passed." >&2
        echo "[restore]   Refusing to silently drop and recreate the live database." >&2
        echo "[restore]   Re-run with --force to acknowledge destructive intent." >&2
        exit 1
    fi
    echo "[restore] WARNING: restoring will DROP and recreate every object in the live"
    echo "[restore]          database. This is destructive and irreversible."
    echo -n "[restore]          Type 'yes' to continue: "
    read -r reply
    if [ "${reply}" != "yes" ]; then
        echo "[restore] Aborted."
        exit 1
    fi
fi

# Source the .env to get POSTGRES_* and database name.
# shellcheck disable=SC1091
set -a; . ./.env; set +a

POSTGRES_USER="${POSTGRES_USER:-feedbackmonk}"
POSTGRES_DB="${POSTGRES_DB:-feedbackmonk}"

if [ -z "${POSTGRES_PASSWORD:-}" ]; then
    echo "[restore] ERROR: POSTGRES_PASSWORD not set in .env." >&2
    exit 1
fi

echo "[restore] Restoring into ${POSTGRES_DB} as ${POSTGRES_USER} via docker compose exec db psql ..."

exec docker compose exec -T \
    -e PGPASSWORD="${POSTGRES_PASSWORD}" \
    db \
    psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" --quiet --single-transaction
