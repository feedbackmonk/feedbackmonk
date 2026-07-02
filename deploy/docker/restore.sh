#!/usr/bin/env bash
# feedbackmonk operator-side restore script.
#
# Restores BOTH halves of feedbackmonk's durable state:
#   1. the Postgres database — reads a gzipped pg_dump from stdin and pipes
#      it into psql in the `db` service container. Idempotent if the dump was
#      created with --clean --if-exists (which backup.sh emits): DROP existing
#      objects + recreate from the dump.
#   2. the feedback ATTACHMENT OBJECTS — optional `--attachments TAR` restores
#      the attachment archive backup.sh wrote (local backend). The objects live
#      in the object store, NOT in Postgres.
#
# ⚠️ ORDERING / COMPLETENESS: the `attachments` table only stores metadata +
# a `storage_key`; the bytes are in the object store. A DB-ONLY restore
# resurrects rows that point at OBJECTS THAT ARE GONE — dangling references,
# broken screenshot/log links. For a full restore you MUST restore BOTH the
# DB and the attachment objects. Restore order does not matter (the api reads
# objects lazily), but SKIPPING the attachment leg does. For a DB-and-objects
# restore, pass --attachments alongside the piped SQL dump.
#
# USAGE:
#   cd deploy/docker
#   # DB + attachments (full restore):
#   gunzip -c backups/feedbackmonk-20260514.sql.gz | \
#       ./restore.sh --attachments backups/feedbackmonk-attachments-20260514.tar.gz
#   # DB only (leaves dangling attachment refs — see warning above):
#   gunzip -c backups/feedbackmonk-20260514.sql.gz | ./restore.sh
#
# SAFETY: --clean --if-exists in the dump means this WILL drop and recreate
# every table, sequence, function, etc. in the database; restoring
# attachments REPLACES the object-store contents. The `--force` flag is
# required to acknowledge this destructive intent. Without --force, restore.sh
# prompts interactively.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

if [ ! -f .env ]; then
    echo "[restore] ERROR: .env not found in $(pwd)." >&2
    exit 1
fi

FORCE=0
ATTACH_TAR=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --force) FORCE=1 ;;
        --attachments)
            ATTACH_TAR="${2:?--attachments requires a tar.gz path}"; shift ;;
        --attachments=*)
            ATTACH_TAR="${1#*=}" ;;
        -h|--help)
            cat <<EOF
Usage: gunzip -c BACKUP.sql.gz | $0 [--force] [--attachments TAR.gz]

Restores a feedbackmonk database dump into the running 'db' service, and
(optionally) the feedback attachment objects.

  --force              Skip the interactive confirmation. Required for non-TTY
                       (cron / systemd-timer) invocation.
  --attachments TAR    Also restore feedback attachment objects from TAR
                       (the archive backup.sh wrote). Local backend only;
                       for S3, restore via bucket versioning / CRR.

The database dump is read from stdin (use gunzip -c to decompress on the fly).

WARNING: a DB-only restore (no --attachments) leaves DANGLING attachment
references — the rows point at objects that no longer exist. Restore BOTH.
EOF
            exit 0
            ;;
    esac
    shift
done

# Source the .env to get POSTGRES_* / storage config.
# shellcheck disable=SC1091
set -a; . ./.env; set +a

BACKEND="${FEEDBACKMONK_STORAGE_BACKEND:-local}"
STORAGE_DIR="${FEEDBACKMONK_STORAGE_LOCAL_DIR:-./data/attachments}"

if [ "${FORCE}" -ne 1 ]; then
    if [ ! -t 0 ]; then
        echo "[restore] ERROR: stdin is not a TTY but --force was not passed." >&2
        echo "[restore]   Refusing to silently drop and recreate the live database." >&2
        echo "[restore]   Re-run with --force to acknowledge destructive intent." >&2
        exit 1
    fi
    echo "[restore] WARNING: restoring will DROP and recreate every object in the live"
    echo "[restore]          database. This is destructive and irreversible."
    if [ -n "${ATTACH_TAR}" ]; then
        echo "[restore]          It will ALSO REPLACE the attachment object store from"
        echo "[restore]          ${ATTACH_TAR}."
    fi
    echo -n "[restore]          Type 'yes' to continue: "
    read -r reply
    if [ "${reply}" != "yes" ]; then
        echo "[restore] Aborted."
        exit 1
    fi
fi

# --- Attachment leg (optional; runs before the DB leg's exec) -------------
if [ -n "${ATTACH_TAR}" ]; then
    case "${BACKEND}" in
        local)
            if [ ! -f "${ATTACH_TAR}" ]; then
                echo "[restore] ERROR: attachment archive not found: ${ATTACH_TAR}" >&2
                exit 1
            fi
            echo "[restore] restoring attachment objects from ${ATTACH_TAR} (local backend)..." >&2
            if [ -d "${STORAGE_DIR}" ]; then
                # Native / host-path storage dir: replace its contents on the host.
                rm -rf "${STORAGE_DIR:?}/"* 2>/dev/null || true
                mkdir -p "${STORAGE_DIR}"
                tar xzf "${ATTACH_TAR}" -C "${STORAGE_DIR}"
                echo "[restore]   restored into host dir ${STORAGE_DIR}" >&2
            else
                # docker-compose named volume: restore into the volume via a
                # one-shot alpine container (mirrors backup.sh's volume path).
                VOLUME="${FEEDBACKMONK_ATTACHMENTS_VOLUME:-feedbackmonk_attachments}"
                TAR_DIR="$(cd "$(dirname "${ATTACH_TAR}")" && pwd)"
                TAR_BASE="$(basename "${ATTACH_TAR}")"
                echo "[restore]   host dir ${STORAGE_DIR} not present; restoring into docker volume ${VOLUME}" >&2
                docker run --rm \
                    -v "${VOLUME}":/data \
                    -v "${TAR_DIR}":/backup:ro \
                    alpine:latest \
                    sh -c "rm -rf /data/* && tar xzf '/backup/${TAR_BASE}' -C /data" >&2
                echo "[restore]   restored into docker volume ${VOLUME}" >&2
            fi
            ;;
        s3)
            echo "[restore] storage backend: s3 — attachment objects are NOT restored from a local tar." >&2
            echo "[restore]   Recover objects via bucket versioning / cross-region-replication failback." >&2
            echo "[restore]   See docs/operations/SELFHOST.md § Backup and Restore and" >&2
            echo "[restore]   docs/operations/RAILWAY_GITCELLAR.md § Backup & DR." >&2
            echo "[restore]   Ignoring --attachments=${ATTACH_TAR} for the s3 backend." >&2
            ;;
        *)
            echo "[restore] WARNING: unknown FEEDBACKMONK_STORAGE_BACKEND='${BACKEND}' — skipping attachment restore." >&2
            ;;
    esac
else
    echo "[restore] NOTE: no --attachments given — restoring DATABASE ONLY. If this dump's" >&2
    echo "[restore]   rows reference attachment objects, those references will DANGLE until" >&2
    echo "[restore]   the objects are also restored (local tar, or S3 versioning/CRR)." >&2
fi

# --- Database leg (unchanged behavior) -----------------------------------
POSTGRES_USER="${POSTGRES_USER:-feedbackmonk}"
POSTGRES_DB="${POSTGRES_DB:-feedbackmonk}"

if [ -z "${POSTGRES_PASSWORD:-}" ]; then
    echo "[restore] ERROR: POSTGRES_PASSWORD not set in .env." >&2
    exit 1
fi

echo "[restore] Restoring into ${POSTGRES_DB} as ${POSTGRES_USER} via docker compose exec db psql ..." >&2

exec docker compose exec -T \
    -e PGPASSWORD="${POSTGRES_PASSWORD}" \
    db \
    psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" --quiet --single-transaction
