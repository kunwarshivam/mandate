#!/usr/bin/env bash
# The migration Job's body (infrastructure §7.2, OPS-13), as mandate_migrator: each file of
# migrations/ once, in file order, as mandate_journal_owner in the journal schema, recorded with its
# SHA-256 in deploy.migrations in the same transaction. An applied file whose content changed stops
# the Job, since a migration is never edited. Mirrors deploy/bootstrap.sh's loop.
#
# First, on every install and upgrade, it grants the journal's application group what roles.sql
# grants each role at bootstrap, connecting to this database and using the journal schema, so a
# role CloudNativePG's managed roles add after bootstrap, a member of the group, can connect too
# (DEC-2083). The grants are idempotent and give the group nothing a role did not already have.
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
DO $$
BEGIN
  EXECUTE format('GRANT CONNECT ON DATABASE %I TO mandate_journal_app', current_database());
END
$$;
SET ROLE mandate_journal_owner;
GRANT USAGE ON SCHEMA journal TO mandate_journal_app;
RESET ROLE;
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
