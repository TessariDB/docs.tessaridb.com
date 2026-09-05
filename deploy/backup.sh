#!/bin/bash
# Takes a backup of the running store, checks it, and keeps the last few.
#
# The store keeps no state that is not derived from its log, so a copy of the
# log is a complete backup and a restore is a replay. `BACKUP;` is a statement
# the running node answers, which is why nothing has to stop for this.
#
# # It needs a store-wide account, and for a week it did not have one
#
# `BACKUP` reads every record in every namespace, so it has no tenancy to check
# and its subject is the **store**. An owner scoped to one database is refused
# with `"owner" holds one database, and this statement's subject is the whole
# store`. That is exactly what this deployment had until 2026-09-02: the store
# had been bootstrapped by a script that declared a database-scoped owner, and
# it could not be repaired in place — a scoped owner may not declare a wider
# user and a closed store has no anonymous session.
#
# It was repaired by rebuilding: a fresh store whose first user the engine
# itself declares from `TESSARIDB_INITIAL_USER` (which has no `ON`, so it owns
# the store), then a republish of `content/`. Q-DOCS-44.
#
# **If this script ever starts failing that way again, do not paper over it.**
# It means a store was bootstrapped without a store-wide owner, and the same
# store also cannot declare a namespace or be repaired. The fallback that needs
# no account at all is to stop the node and open the store directly:
#
#   docker compose ... stop db
#   docker run --rm --user 0:0 -v "$STORE:/store" -v "$INTO:/out" \
#     tessaridb/tessaridb:0.0.3-alpha /store/store --backup /out/manual.tessalog
#   docker compose ... start db
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
# The published engine, and the same version `compose.yaml` runs the store with.
# A different one here would read the store with an engine the deployment has
# never used, which is the one moment you would not want to discover a format
# difference.
IMAGE="${DOCS_DB_IMAGE:-tessaridb/tessaridb:0.0.3-alpha}"

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
    TESSARIDB_PASSWORD="$TESSARIDB_INITIAL_PASSWORD" tessaridb \
      --at "127.0.0.1:${TESSARIDB_ADDRESS##*:}" \
      --user "${TESSARIDB_INITIAL_USER:-owner}" \
      -e "BACKUP;"' \
  | grep -o '^0x[0-9a-f]*' | cut -c3- | xxd -r -p > "${WORKING}"

if [ ! -s "${WORKING}" ]; then
  rm -f "${WORKING}"
  log "the node answered with nothing"
  exit 1
fi

# Read-only, because a verifier has no business writing to the thing it checks.
log "checking it"
if ! OUT="$(docker run --rm --user 0:0 -v "${INTO}:/b:ro" \
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
