#!/usr/bin/env bash
# The nightly local backup, installed as /usr/local/sbin/owlhead-backup and run by
# owlhead-backup.timer. It writes a consistent pg_dump of the owlhead database and a tarball of
# the API's state directory (the artifact store and the minimal vault) into /var/backups/owlhead
# (root only), and keeps 14 days of them. A failure runs owlhead-backup-failed.service.
#
# These copies sit on the VM's own disk: they protect against a bad migration, a mistaken delete,
# or a corrupted table, not against losing the VM. Hetzner's backups (whole-disk images, taken while
# running) cover losing the VM, and include this directory and /etc/owlhead/api.env, so an image
# holds both the vault and its key.
set -euo pipefail

DB=owlhead
DEST=/var/backups/owlhead
STATE=/var/lib/owlhead
KEEP_DAYS=14
# Head room kept free after the backup, on top of twice its estimated size.
SPARE_KB=$((512 * 1024))

umask 077
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
install -d -o root -g root -m 0700 "$DEST"

db_kb=$(($(runuser -u postgres -- psql -XtA -d "$DB" -c "SELECT pg_database_size('$DB')") / 1024))
state_kb="$(du -sk "$STATE" | cut -f1)"
free_kb="$(df -Pk "$DEST" | awk 'NR == 2 { print $4 }')"
need_kb=$((2 * (db_kb + state_kb) + SPARE_KB))
if [ "$free_kb" -lt "$need_kb" ]; then
  echo "not enough disk for a backup: ${free_kb} KB free, ${need_kb} KB needed" >&2
  exit 1
fi

runuser -u postgres -- pg_dump --format=custom "$DB" >"$DEST/$DB-$stamp.dump.partial"
mv "$DEST/$DB-$stamp.dump.partial" "$DEST/$DB-$stamp.dump"

tar -C "$STATE" -czf "$DEST/state-$stamp.tar.gz.partial" artifacts vault
mv "$DEST/state-$stamp.tar.gz.partial" "$DEST/state-$stamp.tar.gz"

find "$DEST" -maxdepth 1 -type f \( -name '*.dump' -o -name '*.tar.gz' -o -name '*.partial' \) \
  -mtime +"$KEEP_DAYS" -delete
echo "backup written: $DEST/$DB-$stamp.dump and $DEST/state-$stamp.tar.gz"
