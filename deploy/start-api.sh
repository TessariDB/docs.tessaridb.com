#!/bin/bash
# Starts the store, then the API, in one container.
#
# The order matters and the waiting is not optional: `docs serve` asks the store
# what it holds before it binds, so starting both at once is a race, and a race
# that loses looks like a store that is unreachable rather than one that is not
# up yet.
#
# bash rather than sh for `wait -n` and `/dev/tcp`, both of which are used below
# and neither of which dash has.
set -euo pipefail

STORE="${TESSARIDB_STORE:?where the store lives}"
# No apostrophe in this message: the word after `:?` is expanded, so a lone
# quote there opens a string that never closes and the script will not parse.
ADDRESS="${TESSARIDB_ADDRESS:?the wire address of the store}"
NAMESPACE="${DOCS_NAMESPACE:?the documentation version}"

HOST="${ADDRESS%%:*}"
PORT="${ADDRESS##*:}"

log() { printf 'start-api: %s\n' "$*" >&2; }

# ── the store ───────────────────────────────────────────────────────────────

log "starting the node on ${ADDRESS} over ${STORE}"
tessaridb "${STORE}" --serve "${ADDRESS}" &
NODE=$!

# Stopping this container must stop the node, not orphan it. tini is PID 1 and
# forwards the signal here; this forwards it on and waits, so the node gets its
# chance to close the store rather than being killed with it half-written.
stop() {
  log "stopping"
  kill -TERM "${API:-}" 2>/dev/null || true
  kill -TERM "${NODE}" 2>/dev/null || true
  wait 2>/dev/null || true
  exit 0
}
trap stop TERM INT

waited=0
until (exec 3<>"/dev/tcp/${HOST}/${PORT}") 2>/dev/null; do
  kill -0 "${NODE}" 2>/dev/null || { log "the node exited while starting"; exit 1; }
  if [ "${waited}" -ge 60 ]; then
    log "the node did not accept connections in 60s"
    exit 1
  fi
  sleep 1
  waited=$((waited + 1))
done
log "the node is accepting connections after ${waited}s"

# ── the three accounts, once, on a fresh store ──────────────────────────────
#
# A store with no user is **open** and runs anything for anybody who reaches the
# port; declaring the first user is what closes it. An unattended deployment
# that never declares one is a database the network may rewrite.
#
# Three accounts, and each exists for a reason the other two cannot cover:
#
#   owner   declares the other two, and nothing else uses it. Only an owner may
#           declare a user, and the first user has to be declared by the
#           anonymous session that still can — so the owner must be first.
#   reader  a `viewer`, which the public read path runs as. A closed store has
#           no anonymous access at all, so without this the site serves nothing;
#           and because it is a viewer, a write on the read path is refused by
#           the store rather than by the server's routing.
#   editor  writes. Ingest and the API's PUT and DELETE run as this.
#
# This runs at most once **by construction** rather than by remembering: the
# owner's declaration closes the store, so a second start's anonymous attempt is
# refused and the block is skipped. There is no marker file to fall out of step
# with the store it describes.
#
# Keep the owner password. The store has no back door: an anonymous session
# cannot re-open a closed store by dropping its last user, so a lost owner
# password is a restore from backup and not a recovery.
if [ -n "${DOCS_OWNER_PASSWORD:-}" ] \
&& [ -n "${DOCS_READER_PASSWORD:-}" ] \
&& [ -n "${DOCS_EDITOR_PASSWORD:-}" ]; then
  # Exported, not just assigned. `docs serve` below reads `DOCS_READER_USER`
  # from the environment to decide who the public reads run as — so defaulting
  # the name here and not putting it back would declare an account called
  # `reader` and then serve anonymously, which a closed store refuses. The
  # symptom is a site that starts cleanly and 403s every page.
  export DOCS_OWNER_USER="${DOCS_OWNER_USER:-owner}"
  export DOCS_READER_USER="${DOCS_READER_USER:-reader}"
  export DOCS_EDITOR_USER="${DOCS_EDITOR_USER:-editor}"
  OWNER="${DOCS_OWNER_USER}"
  READER="${DOCS_READER_USER}"
  EDITOR="${DOCS_EDITOR_USER}"

  if tessaridb --at "${ADDRESS}" -e "
        DEFINE NAMESPACE IF NOT EXISTS ${NAMESPACE};
        USE NAMESPACE ${NAMESPACE};
        DEFINE DATABASE IF NOT EXISTS docs;
        USE DATABASE docs;
        DEFINE USER ${OWNER} ON ${NAMESPACE}.docs ROLE owner PASSWORD '${DOCS_OWNER_PASSWORD}';
      " >/dev/null 2>&1; then
    log "declared ${OWNER}; the store is now closed to anonymous sessions"
    # From here the owner must sign in, because the statement above took effect
    # the moment it committed.
    TESSARIDB_PASSWORD="${DOCS_OWNER_PASSWORD}" tessaridb \
      --at "${ADDRESS}" --user "${OWNER}" -e "
        USE NAMESPACE ${NAMESPACE};
        USE DATABASE docs;
        DEFINE USER ${READER} ON ${NAMESPACE}.docs ROLE viewer PASSWORD '${DOCS_READER_PASSWORD}';
        DEFINE USER ${EDITOR} ON ${NAMESPACE}.docs ROLE editor PASSWORD '${DOCS_EDITOR_PASSWORD}';
      " >/dev/null
    log "declared ${READER} (viewer) and ${EDITOR} (editor)"
  else
    log "the store already has users; leaving them alone"
  fi
else
  log "DOCS_OWNER_PASSWORD, DOCS_READER_PASSWORD and DOCS_EDITOR_PASSWORD are"
  log "not all set, so no users are declared and THE STORE STAYS OPEN — it will"
  log "run anything for anybody who reaches ${ADDRESS}. Development only."
fi

# ── the API ─────────────────────────────────────────────────────────────────
#
# It signs in as the editor, because on a fresh store it seeds the content, and
# it reads as `DOCS_READER_USER`, which is already in the environment. On every
# later start the store is populated and nothing is written.

log "starting the API"
DOCS_USER="${DOCS_EDITOR_USER:-editor}" \
DOCS_PASSWORD="${DOCS_EDITOR_PASSWORD:-}" \
  docs serve --at "${ADDRESS}" &
API=$!

# Whichever exits first takes the container with it. A container still running
# with half of itself dead is one whose health check may keep passing and that
# nobody thinks to look at.
#
# Polled rather than `wait -n`, which wants bash 4.3 — this script is worth
# being able to run outside the image, and a one-second poll on a process that
# is expected to run for weeks costs nothing.
while kill -0 "${NODE}" 2>/dev/null && kill -0 "${API}" 2>/dev/null; do
  sleep 1
done
log "a process exited; stopping the other"
stop
