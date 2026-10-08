#!/usr/bin/env bash
# The nightly local backup, installed as /usr/local/sbin/owlhead-backup and run by
# owlhead-backup.timer. It writes a consistent pg_dump of the owlhead database and a tarball of the
# artifact store into /var/backups/owlhead (root only), and keeps 14 days of them.
#
# These copies sit on the VM's own disk: they protect against a bad migration, a mistaken delete,
# or a corrupted table, not against losing the VM. Hetzner's backups (whole-disk images, taken while
# running) cover losing the VM, and include this directory.
set -euo pipefail

DB=owlhead
DEST=/var/backups/owlhead
ARTIFACTS=/var/lib/owlhead/artifacts
KEEP_DAYS=14

umask 077
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
install -d -o root -g root -m 0700 "$DEST"

runuser -u postgres -- pg_dump --format=custom "$DB" >"$DEST/$DB-$stamp.dump.partial"
mv "$DEST/$DB-$stamp.dump.partial" "$DEST/$DB-$stamp.dump"

if [ -d "$ARTIFACTS" ]; then
  tar -C "$(dirname "$ARTIFACTS")" -czf "$DEST/artifacts-$stamp.tar.gz.partial" "$(basename "$ARTIFACTS")"
  mv "$DEST/artifacts-$stamp.tar.gz.partial" "$DEST/artifacts-$stamp.tar.gz"
fi

find "$DEST" -maxdepth 1 -type f \( -name '*.dump' -o -name '*.tar.gz' -o -name '*.partial' \) \
  -mtime +"$KEEP_DAYS" -delete
echo "backup written: $DEST/$DB-$stamp.dump"
