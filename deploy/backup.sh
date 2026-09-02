#!/bin/bash
# Takes a backup of the store, checks it, and keeps the last few.
#
# The store keeps no state that is not derived from its log, so a copy of the
# log is a complete backup and a restore is a replay.
#
# # Why this stops the node for a few seconds
#
# `BACKUP;` is a statement a running node answers, and this script used to ask
# for it that way — no downtime, nothing to coordinate. It cannot any more, and
# the reason is worth stating rather than rediscovering: `BACKUP`'s subject is
# the **whole store**, so it has no tenancy to check and requires an account
# whose reach is the store. Every account in this deployment is scoped to
# `<namespace>.docs`, because the store was bootstrapped before the script that
# declares a store-wide owner first. A scoped owner cannot declare a wider user
# and a closed store has no anonymous session, so this is not repairable in
# place — it is repaired by a fresh store and a republish, which is Q-DOCS-44.
#
# Until then the backup is taken the other way: the node is stopped, the engine
# opens the store directly as a local process — which needs no credential,
# because there is no session to authorise — and the node is started again. That
# costs about ten seconds of the site being down, once per run. A backup that
# runs beats a backup that returns a permission error, and this way needs no
# account at all, which is the property that survives the next bootstrap
# accident.
#
# It is safe to run while the site is being edited: the node is asked to stop
# properly, so it drains and closes the store rather than being killed.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE=(docker compose -f "${HERE}/compose.yaml" --env-file "${HERE}/.env")
INTO="${DOCS_BACKUP_DIR:-/var/backups/docs}"
KEEP="${DOCS_BACKUP_KEEP:-14}"
# The published engine, and the same version `compose.yaml` runs the store with.
# A different one here would read the store with an engine the deployment has
# never used, which is the one moment you would not want to discover a format
# difference.
IMAGE="${DOCS_DB_IMAGE:-tessaridb/tessaridb:0.0.2-alpha}"
# Where the store lives on the host, as `compose.yaml` binds it.
STORE="${DOCS_STORE_PATH:-${HERE}/store}"

log() { printf 'backup: %s\n' "$*" >&2; }

mkdir -p "${INTO}"
chmod 700 "${INTO}"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
FILE="${INTO}/${STAMP}.tessalog"
# Written aside and moved into place only once it has been checked, so the
# directory never holds a file that has not been read back.
WORKING="${FILE}.partial"

# However this exits, the site comes back. Without this an interrupted run
# leaves the store down and nothing says so until somebody loads the site.
started=no
restore() {
  if [ "${started}" = yes ]; then
    log "starting the node again"
    "${COMPOSE[@]}" start db >/dev/null || log "the node did not start — check it"
  fi
}
trap restore EXIT

log "stopping the node so the store is closed and consistent"
"${COMPOSE[@]}" stop db >/dev/null
started=yes

log "reading the log out of the store"
# `--user 0:0` because the store's directory is owned by the image's uid 10001
# and this runs as root on the host; the mount is read-write only because the
# backup is written into a subdirectory of it and then moved out.
docker run --rm --user 0:0 \
  -v "${STORE}:/store" -v "${INTO}:/out" \
  "${IMAGE}" /store/store --backup "/out/$(basename "${WORKING}")" >&2

if [ ! -s "${WORKING}" ]; then
  rm -f "${WORKING}"
  log "the engine wrote nothing"
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
