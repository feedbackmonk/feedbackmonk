#!/usr/bin/env bash
# feedbackmonk operator-side backup script.
#
# Backs up BOTH halves of feedbackmonk's durable state:
#   1. the Postgres database (gzipped pg_dump → stdout), and
#   2. the feedback ATTACHMENT OBJECTS (screenshots + scrubbed logs) —
#      which live in the object store, NOT in Postgres. The `attachments`
#      table holds only metadata + a `storage_key`; the bytes are in the
#      configured backend (local FS volume, or S3). A DB-only backup that
#      omits the objects restores to DANGLING attachment references — the
#      rows point at objects that no longer exist. This is a P0 for a sole,
#      no-fallback backend (scrutiny P0-4), so the object leg runs first.
#
# The pg_dump still goes to stdout so existing cron redirects keep working:
#
#   cd deploy/docker
#   ./backup.sh > backups/feedbackmonk-$(date +%Y%m%d-%H%M%S).sql.gz
#
# Or in a cron / systemd-timer:
#   0 3 * * *  cd /opt/feedbackmonk/deploy/docker && ./backup.sh > /var/backups/feedbackmonk/$(date +\%Y\%m\%d).sql.gz
#
# The attachment archive is written to a FILE (it cannot share the SQL
# stdout stream), timestamped, next to the SQL dump. Default dir: ./backups
# (override with --attachments-dir DIR). All human output goes to stderr, so
# stdout carries ONLY the binary-safe SQL dump.
#
# Backend detection: reads FEEDBACKMONK_STORAGE_BACKEND from .env
# (default `local`). For `local` it tars the storage directory / docker
# volume; for `s3` it prints durability guidance (objects are not dumped
# locally — durability is delegated to the bucket).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

ATTACH_OUT="./backups"
DO_ATTACHMENTS=1

while [ "$#" -gt 0 ]; do
    case "$1" in
        --attachments-dir)
            ATTACH_OUT="${2:?--attachments-dir requires a directory}"; shift ;;
        --attachments-dir=*)
            ATTACH_OUT="${1#*=}" ;;
        --skip-attachments)
            DO_ATTACHMENTS=0 ;;
        -h|--help)
            cat <<EOF
Usage: $0 [--attachments-dir DIR] [--skip-attachments] > BACKUP.sql.gz

Backs up the feedbackmonk database (gzipped pg_dump to stdout) AND the
feedback attachment objects (to a timestamped tar.gz file).

  --attachments-dir DIR   Where to write the attachment archive.
                          Default: ./backups
  --skip-attachments      DB-only backup (DANGEROUS for a sole backend —
                          leaves attachment references undefended).

The SQL dump is emitted on stdout; redirect it to a file. The attachment
archive is written to a file under --attachments-dir. Human progress goes
to stderr.

Env (from .env):
  FEEDBACKMONK_STORAGE_BACKEND    local (default) | s3
  FEEDBACKMONK_STORAGE_LOCAL_DIR  local backend storage root (default ./data/attachments)
  FEEDBACKMONK_ATTACHMENTS_VOLUME docker volume name to archive when the storage
                                  dir is inside a container (default feedbackmonk_attachments)
EOF
            exit 0 ;;
    esac
    shift
done

if [ ! -f .env ]; then
    echo "[backup] ERROR: .env not found in $(pwd)." >&2
    echo "[backup] Run 'cp .env.example .env' and fill in DATABASE_URL + POSTGRES_PASSWORD first." >&2
    exit 1
fi

# Source .env (same pattern restore.sh uses) so we can read the storage
# backend + local dir. Values only inform the attachment leg; the pg_dump
# leg gets its config via `docker compose` reading .env itself.
# shellcheck disable=SC1091
set -a; . ./.env; set +a

BACKEND="${FEEDBACKMONK_STORAGE_BACKEND:-local}"
STORAGE_DIR="${FEEDBACKMONK_STORAGE_LOCAL_DIR:-./data/attachments}"
TS="$(date -u +%Y%m%d-%H%M%S)"

if [ "${DO_ATTACHMENTS}" -eq 1 ]; then
    case "${BACKEND}" in
        local)
            mkdir -p "${ATTACH_OUT}"
            OUT_ABS="$(cd "${ATTACH_OUT}" && pwd)"
            OUT_FILE="${OUT_ABS}/feedbackmonk-attachments-${TS}.tar.gz"
            echo "[backup] storage backend: local — archiving attachment objects..." >&2
            if [ -d "${STORAGE_DIR}" ]; then
                # Native / host-path storage dir: tar directly on the host.
                tar czf "${OUT_FILE}" -C "${STORAGE_DIR}" .
                echo "[backup]   wrote ${OUT_FILE} (from host dir ${STORAGE_DIR})" >&2
            else
                # docker-compose named volume: the storage dir lives inside the
                # api container. Archive the volume via a one-shot alpine
                # container (mirrors the pgdata volume-level pattern in
                # docs/operations/SELFHOST.md). Volume name defaults to
                # feedbackmonk_attachments; override via FEEDBACKMONK_ATTACHMENTS_VOLUME
                # if your compose project name differs.
                VOLUME="${FEEDBACKMONK_ATTACHMENTS_VOLUME:-feedbackmonk_attachments}"
                echo "[backup]   host dir ${STORAGE_DIR} not present; archiving docker volume ${VOLUME}" >&2
                docker run --rm \
                    -v "${VOLUME}":/data:ro \
                    -v "${OUT_ABS}":/backup \
                    alpine:latest \
                    tar czf "/backup/feedbackmonk-attachments-${TS}.tar.gz" -C /data . >&2
                echo "[backup]   wrote ${OUT_FILE} (from docker volume ${VOLUME})" >&2
            fi
            ;;
        s3)
            echo "[backup] storage backend: s3 — attachment objects are NOT dumped locally." >&2
            echo "[backup]   Object durability is delegated to the S3 bucket. Ensure:" >&2
            echo "[backup]     - bucket VERSIONING is enabled (recover overwrites/deletes)" >&2
            echo "[backup]     - CROSS-REGION REPLICATION (CRR) is configured for DR" >&2
            echo "[backup]     - lifecycle rules match your data-retention policy" >&2
            echo "[backup]   Bucket: ${FEEDBACKMONK_S3_BUCKET:-<unset>}  region: ${FEEDBACKMONK_S3_REGION:-us-east-1}" >&2
            echo "[backup]   (See docs/operations/SELFHOST.md § Backup and Restore.)" >&2
            ;;
        *)
            echo "[backup] WARNING: unknown FEEDBACKMONK_STORAGE_BACKEND='${BACKEND}' — skipping attachment leg." >&2
            ;;
    esac
else
    echo "[backup] --skip-attachments: DB-only backup. WARNING: a DB restore from this" >&2
    echo "[backup]   dump alone leaves DANGLING attachment references. Only safe if you" >&2
    echo "[backup]   back up attachment objects out-of-band (e.g. S3 versioning/CRR)." >&2
fi

# --- Database leg (unchanged behavior) -----------------------------------
# `docker compose --profile backup run --rm backup` invokes the `backup`
# service defined in docker-compose.yml. The service's command pipes
# pg_dump to gzip and emits to stdout; we forward that to whatever the
# operator redirected this script's stdout to.
#
# Note: -T (--no-TTY) ensures binary-safe stdout (no terminal translations).
echo "[backup] dumping database to stdout (gzipped pg_dump)..." >&2
exec docker compose --profile backup run --rm -T backup
