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
# The published engine, read out of `compose.yaml` rather than repeated here.
#
# It has to be the same version the store runs, because a backup is verified by
# an engine and an older one refuses a newer file rather than guessing at it —
# correctly. This was a literal until 2026-09-08, and it drifted: the pin moved
# to 0.0.4-alpha and then 0.0.5-alpha while this line stayed at 0.0.3-alpha, so
# for two nights the node wrote a good log, the check refused to read it, and
# the script discarded it and exited 0. Nothing was in an error state and the
# site had no backup:
#
#   this backup was written by version 0.0.5; this build is 0.0.3 and will not
#   guess at what a newer one meant
#
# A copy of a value that must equal another value is a defect waiting for
# somebody to move one of them, so it is derived. `DOCS_DB_IMAGE` still
# overrides, which is what makes reading an older store possible on purpose.
IMAGE="${DOCS_DB_IMAGE:-}"
if [[ -z "${IMAGE}" ]]; then
  IMAGE="$(sed -n 's|^[[:space:]]*image:[[:space:]]*\(tessaridb/tessaridb:[^[:space:]]*\).*|\1|p' "${HERE}/compose.yaml" | head -1)"
fi
if [[ -z "${IMAGE}" ]]; then
  log "no engine image found in compose.yaml and DOCS_DB_IMAGE is unset"
  exit 1
fi

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
