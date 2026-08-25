#!/bin/bash
# Takes a backup of the running store, checks it, and keeps the last few.
#
# The store keeps no state that is not derived from its log, so a copy of the
# log is a complete backup and a restore is a replay. `BACKUP;` is a statement
# the running node answers, which is why nothing has to stop for this.
#
# Two halves in two places, and not by preference: `xxd` is not in the database
# image and the engine binary is not on the host. So the node is asked inside the
# container, the hex it answers with is decoded here, and the file is checked by
# the engine again over a read-only mount.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE=(docker compose -f "${HERE}/compose.yaml" --env-file "${HERE}/.env")
INTO="${DOCS_BACKUP_DIR:-/var/backups/docs}"
KEEP="${DOCS_BACKUP_KEEP:-14}"
IMAGE="${DOCS_DB_IMAGE:-tessaridb-docs-db:latest}"

log() { printf 'backup: %s\n' "$*" >&2; }

mkdir -p "${INTO}"
chmod 700 "${INTO}"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
FILE="${INTO}/${STAMP}.tessalog"
# Written aside and moved into place only once it has been checked, so the
# directory never holds a file that has not been read back.
WORKING="${FILE}.partial"

log "asking the node for its log"
# The password is read from the container's own environment. An argument would
# be in the process table for anybody on the host to see.
"${COMPOSE[@]}" exec -T db sh -c '
    TESSARIDB_PASSWORD="$DOCS_OWNER_PASSWORD" tessaridb \
      --at "127.0.0.1:${TESSARIDB_ADDRESS##*:}" \
      --user "${DOCS_OWNER_USER:-owner}" \
      -e "USE NAMESPACE $DOCS_NAMESPACE; USE DATABASE docs; BACKUP;"' \
  | grep -o '^0x[0-9a-f]*' | cut -c3- | xxd -r -p > "${WORKING}"

if [ ! -s "${WORKING}" ]; then
  rm -f "${WORKING}"
  log "the node answered with nothing"
  exit 1
fi

# Root, because the directory is 700 and the image's own user is uid 10001.
# Read-only, because a verifier has no business writing to the thing it checks.
log "checking it"
if ! OUT="$(docker run --rm --user 0:0 -v "${INTO}:/b:ro" --entrypoint tessaridb \
              "${IMAGE}" --verify "/b/$(basename "${WORKING}")" 2>&1)"; then
  log "the file did not verify, so it is not kept: ${OUT}"
  rm -f "${WORKING}"
  exit 1
fi
log "${OUT}"

mv "${WORKING}" "${FILE}"
log "kept ${FILE} ($(stat -c %s "${FILE}") bytes)"

# Oldest first, and only whole files — a `.partial` left by a failed run is
# removed above, never rotated.
mapfile -t OLD < <(ls -1t "${INTO}"/*.tessalog 2>/dev/null | tail -n "+$((KEEP + 1))")
for stale in "${OLD[@]:-}"; do
  [ -n "${stale}" ] || continue
  rm -f "${stale}"
  log "removed ${stale}"
done
