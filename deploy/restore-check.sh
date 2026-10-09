#!/usr/bin/env bash
# Proves a backup restores: loads a pg_dump into a scratch database, exports every journal stream
# from it, runs `mandate journal verify` (journal spec §11) on each against the backed-up artifact
# store, and checks that each verified span ends at the head `stream_heads` records, so a backup
# that lost a stream's last events fails. Run as root, monthly and after any restore:
#
#   bash restore-check.sh /var/backups/owlhead/owlhead-<stamp>.dump /var/backups/owlhead/state-<stamp>.tar.gz
#
# Needs the `mandate` CLI at /usr/local/bin/mandate. The scratch database is dropped at the end;
# the live database is never touched. What it cannot see: a stream whose events and stream_heads
# row were both deleted leaves no trace in the database; only the cold segments and anchors (journal
# spec §6.2, §10, §11) show it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
if [ "$#" -ne 2 ]; then
  echo "usage: bash restore-check.sh <owlhead-*.dump> <state-*.tar.gz>" >&2
  exit 2
fi
DUMP="$1"
STATE_TAR="$2"
require_host

CHECK_DB=owlhead_restore_check
MANDATE=/usr/local/bin/mandate
DSN="postgresql://postgres@localhost/$CHECK_DB?host=/var/run/postgresql"

if [ ! -x "$MANDATE" ]; then
  echo "the mandate CLI is not installed at $MANDATE" >&2
  exit 1
fi
work="$(mktemp -d)"
chown postgres:postgres "$work"
cleanup() {
  runuser -u postgres -- dropdb --if-exists "$CHECK_DB" || true
  rm -rf "$work"
}
trap cleanup EXIT

say "Restore into $CHECK_DB"
runuser -u postgres -- dropdb --if-exists "$CHECK_DB"
runuser -u postgres -- createdb "$CHECK_DB"
runuser -u postgres -- pg_restore --exit-on-error -d "$CHECK_DB" <"$DUMP"
runuser -u postgres -- psql -XqA -v ON_ERROR_STOP=1 -d "$CHECK_DB" \
  -c "ALTER DATABASE $CHECK_DB SET search_path = journal"
tar -C "$work" -xzf "$STATE_TAR" artifacts
chown -R postgres:postgres "$work"

say "Export and verify every stream, each to its recorded head"
heads="$(runuser -u postgres -- psql -XtA -F ' ' -v ON_ERROR_STOP=1 -d "$CHECK_DB" \
  -c "SELECT stream_id, seq, encode(hash, 'hex') FROM journal.stream_heads ORDER BY stream_id")"
if [ -z "$heads" ]; then
  echo "restore check passed: the restored journal is empty (no stream has an event yet)"
  exit 0
fi
count=0
while read -r stream seq hash; do
  out="$work/segment-$count.jsonl"
  runuser -u postgres -- "$MANDATE" journal export "$stream" "$out" --journal "$DSN"
  verified="$(runuser -u postgres -- "$MANDATE" journal verify "$out" --store "$work/artifacts" | tee /dev/stderr | grep '^result: ')"
  check_head "$verified" "$stream" "$seq" "$hash"
  count=$((count + 1))
done <<<"$heads"
echo "restore check passed: $count streams verified to their recorded heads"
