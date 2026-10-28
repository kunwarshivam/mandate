#!/usr/bin/env bash
# The migration Job's body (infrastructure §7.2, OPS-13), as mandate_migrator: each file of
# migrations/ once, in file order, as mandate_journal_owner in the journal schema, recorded with its
# SHA-256 in deploy.migrations in the same transaction. An applied file whose content changed stops
# the Job, since a migration is never edited. Mirrors deploy/bootstrap.sh's loop.
#
# First, on every install and upgrade, it grants the journal's application group CONNECT on this
# database, which roles.sql grants each role at bootstrap, so a role CloudNativePG's managed roles
# add after bootstrap, a member of the group, can connect too; the group already uses the journal
# schema (DEC-2083). The grant is idempotent and gives the group nothing a role did not have.
set -euo pipefail

migrations="${MANDATE_MIGRATIONS:-/migrations}"

sql() {
  psql "$MANDATE_JOURNAL_DSN" -X -q -v ON_ERROR_STOP=1 "$@"
}

sql <<'SQL'
CREATE TABLE IF NOT EXISTS deploy.migrations (
  file text PRIMARY KEY,
  sha256 text NOT NULL,
  applied_at timestamptz NOT NULL DEFAULT now()
);
SQL

sql <<'SQL'
SELECT format('GRANT CONNECT ON DATABASE %I TO mandate_journal_app', current_database()) \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO mandate_journal_reader', current_database())
WHERE EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_reader') \gexec
SQL

for file in "$migrations"/*.sql; do
  name="$(basename "$file")"
  digest="$(sha256sum "$file" | cut -d' ' -f1)"
  applied="$(sql -tA -v name="$name" <<'SQL'
SELECT sha256 FROM deploy.migrations WHERE file = :'name';
SQL
)"
  if [ -n "$applied" ]; then
    if [ "$applied" != "$digest" ]; then
      echo "$name was applied with sha256 $applied and now reads $digest; a migration is never edited" >&2
      exit 1
    fi
    echo "already applied: $name"
    continue
  fi
  {
    printf 'BEGIN;\nSET ROLE mandate_journal_owner;\nSET search_path = journal;\n'
    cat "$file"
    printf '\nRESET ROLE;\nINSERT INTO deploy.migrations (file, sha256) VALUES (:%s, :%s);\nCOMMIT;\n' "'name'" "'digest'"
  } | sql -v name="$name" -v digest="$digest"
  echo "applied: $name"
done

# Last, the read-only group's reads (DEC-2082): select on the journal's tables, as their owner, and
# nothing else, since a member's binary does not itself refuse a write. A journal bootstrapped
# before the group was made gets it from CloudNativePG's managed roles; until then this grants
# nothing, so the Job never fails on it, and the next run grants it. The tables are named, so a
# journal table a later migration adds is not readable by the group until it is listed here.
sql <<'SQL'
SET ROLE mandate_journal_owner;
SELECT 'GRANT USAGE ON SCHEMA journal TO mandate_journal_reader'
WHERE EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_reader') \gexec
SELECT 'GRANT SELECT ON journal.event_ids, journal.events, journal.stream_heads TO mandate_journal_reader'
WHERE EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_reader') \gexec
RESET ROLE;
SQL
