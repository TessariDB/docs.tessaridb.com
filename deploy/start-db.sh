#!/bin/bash
# Starts the node, and declares the store's three accounts on a fresh store.
#
# The declaration lives here rather than with the API because it is the store's
# own business: it needs the `tessaridb` CLI, and keeping that in the API image
# would keep the API coupled to the engine build the split exists to separate.
#
# bash rather than sh for `/dev/tcp`, which dash does not have.
set -euo pipefail

STORE="${TESSARIDB_STORE:?where the store lives}"
# No apostrophe in this message: the word after `:?` is expanded, so a lone
# quote there opens a string that never closes and the script will not parse.
ADDRESS="${TESSARIDB_ADDRESS:?the wire address of the store}"
NAMESPACE="${DOCS_NAMESPACE:?the documentation version}"

PORT="${ADDRESS##*:}"
# The node binds whatever it was told, usually every interface so the API's
# container can reach it. This script talks to it over loopback regardless.
LOCAL="127.0.0.1:${PORT}"

# What the health check looks for. Deliberately not on the volume: it says "this
# container has finished starting", not "this store has been bootstrapped", so
# it must disappear when the container does. The idempotency of the declaration
# below is a property of the store, not of any file.
READY=/tmp/store-ready

log() { printf 'start-db: %s\n' "$*" >&2; }

rm -f "${READY}"

log "starting the node on ${ADDRESS} over ${STORE}"
tessaridb "${STORE}" --serve "${ADDRESS}" &
NODE=$!

# Stopping this container must close the store, not kill it half-written. tini
# is PID 1 and forwards the signal here; this forwards it on and waits.
stop() {
  log "stopping"
  kill -TERM "${NODE}" 2>/dev/null || true
  wait 2>/dev/null || true
  exit 0
}
trap stop TERM INT

waited=0
until (exec 3<>"/dev/tcp/127.0.0.1/${PORT}") 2>/dev/null; do
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
#           the store rather than by the API's routing.
#   editor  writes. The seed and the API's PUT and DELETE run as this.
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
  OWNER="${DOCS_OWNER_USER:-owner}"
  READER="${DOCS_READER_USER:-reader}"
  EDITOR="${DOCS_EDITOR_USER:-editor}"

  # The store-wide owner FIRST, and it is not a nicety. Some statements have no
  # tenancy to check because their subject is the store itself — `BACKUP` reads
  # every namespace, and `DEFINE NAMESPACE` adds a sibling to every other one —
  # so they need an owner holding no tenancy of their own. A deployment whose
  # widest account is scoped to one database cannot back itself up, and cannot
  # be repaired afterwards: a scoped owner may not declare a wider user, and a
  # closed store has no anonymous session that could.
  #
  # The namespace is declared here too, by the same account and for the same
  # reason, so the API's own narrow account never has to ask for it.
  if tessaridb --at "${LOCAL}" -e "
        DEFINE USER ${OWNER} ROLE owner PASSWORD '${DOCS_OWNER_PASSWORD}';
      " >/dev/null 2>&1; then
    log "declared ${OWNER} over the whole store; it is now closed to anonymous sessions"
    # From here the owner must sign in, because the statement above took effect
    # the moment it committed.
    TESSARIDB_PASSWORD="${DOCS_OWNER_PASSWORD}" tessaridb \
      --at "${LOCAL}" --user "${OWNER}" -e "
        DEFINE NAMESPACE IF NOT EXISTS ${NAMESPACE};
        USE NAMESPACE ${NAMESPACE};
        DEFINE DATABASE IF NOT EXISTS docs;
        USE DATABASE docs;
        DEFINE USER ${READER} ON ${NAMESPACE}.docs ROLE viewer PASSWORD '${DOCS_READER_PASSWORD}';
        DEFINE USER ${EDITOR} ON ${NAMESPACE}.docs ROLE editor PASSWORD '${DOCS_EDITOR_PASSWORD}';
      " >/dev/null
    log "declared ${NAMESPACE}.docs, ${READER} (viewer) and ${EDITOR} (editor)"
  else
    log "the store already has users; leaving them alone"
  fi
else
  log "DOCS_OWNER_PASSWORD, DOCS_READER_PASSWORD and DOCS_EDITOR_PASSWORD are"
  log "not all set, so no users are declared and THE STORE STAYS OPEN — it will"
  log "run anything for anybody who reaches ${ADDRESS}. Development only."
fi

# Only now. The API waits on this container's health, and what it is really
# waiting for is an account to sign in as — a store that is listening but has no
# users yet would let it connect anonymously and then refuse it a moment later.
touch "${READY}"
log "ready"

wait "${NODE}"
