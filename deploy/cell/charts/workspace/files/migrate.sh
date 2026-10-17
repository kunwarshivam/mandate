#!/usr/bin/env bash
# The migration Job's body (infrastructure §7.2, OPS-13), as mandate_migrator: each file of
# migrations/ once, in file order, as mandate_journal_owner in the journal schema, recorded with its
# SHA-256 in deploy.migrations in the same transaction. An applied file whose content changed stops
# the Job, since a migration is never edited. Mirrors deploy/bootstrap.sh's loop.
set -euo pipefail

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

for file in /migrations/*.sql; do
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
