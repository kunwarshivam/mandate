#!/usr/bin/env bash
# Proves a backup restores: loads a pg_dump into a scratch database, exports every journal stream
# from it, and runs `mandate journal verify` (journal spec §11) on each against the backed-up
# artifact store. Run as root, monthly and after any restore:
#
#   bash restore-check.sh /var/backups/owlhead/owlhead-<stamp>.dump /var/backups/owlhead/artifacts-<stamp>.tar.gz
#
# Needs the `mandate` CLI at /usr/local/bin/mandate. The scratch database is dropped at the end;
# the live database is never touched.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
if [ "$#" -ne 2 ]; then
  echo "usage: bash restore-check.sh <owlhead-*.dump> <artifacts-*.tar.gz>" >&2
  exit 2
fi
DUMP="$1"
ARTIFACTS_TAR="$2"
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
tar -C "$work" -xzf "$ARTIFACTS_TAR"
chown -R postgres:postgres "$work"

say "Export and verify every stream"
streams="$(runuser -u postgres -- psql -XtA -v ON_ERROR_STOP=1 -d "$CHECK_DB" \
  -c "SELECT stream_id FROM journal.stream_heads ORDER BY stream_id")"
if [ -z "$streams" ]; then
  echo "the restored journal has no streams; nothing to verify"
  exit 0
fi
count=0
while IFS= read -r stream; do
  out="$work/segment-$count.jsonl"
  runuser -u postgres -- "$MANDATE" journal export "$stream" "$out" --journal "$DSN"
  runuser -u postgres -- "$MANDATE" journal verify "$out" --store "$work/artifacts"
  count=$((count + 1))
done <<<"$streams"
echo "restore check passed: $count streams verified"
