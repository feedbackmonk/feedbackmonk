#!/usr/bin/env bash
# feedbackmonk operator-side backup script.
#
# Pipes a gzipped pg_dump of the feedbackmonk database to stdout. Operators
# redirect to a timestamped file:
#
#   cd deploy/docker
#   ./backup.sh > backups/feedbackmonk-$(date +%Y%m%d-%H%M%S).sql.gz
#
# Or in a cron / systemd-timer:
#   0 3 * * *  cd /opt/feedbackmonk/deploy/docker && ./backup.sh > /var/backups/feedbackmonk/$(date +\%Y\%m\%d).sql.gz
#
# Uses `docker compose run --rm --profile backup backup`, which runs a
# one-shot postgres:16-alpine container that pg_dumps the db service.
# The container exits as soon as pg_dump completes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

if [ ! -f .env ]; then
    echo "[backup] ERROR: .env not found in $(pwd)." >&2
    echo "[backup] Run 'cp .env.example .env' and fill in DATABASE_URL + POSTGRES_PASSWORD first." >&2
    exit 1
fi

# `docker compose --profile backup run --rm backup` invokes the `backup`
# service defined in docker-compose.yml. The service's command pipes
# pg_dump to gzip and emits to stdout; we forward that to whatever the
# operator redirected this script's stdout to.
#
# Note: --no-TTY ensures binary-safe stdout (no terminal translations).
exec docker compose --profile backup run --rm -T backup
