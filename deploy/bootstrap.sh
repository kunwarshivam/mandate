#!/usr/bin/env bash
# Bootstraps the demo API host (DEC-822): Ubuntu 24.04 x86_64, one VM. Safe to run again; each
# step checks what is already there. Run as root from this directory:
#
#   bash bootstrap.sh --dry-run   # print every change, make none
#   bash bootstrap.sh
#
# It installs PostgreSQL 18 from apt.postgresql.org (localhost only), creates the database, its
# roles and schema, applies migrations/, creates the API's system user and directories, installs
# the API unit (not started), the backup timer, and /etc/owlhead/api.env from api.env.example if it
# is absent. It never writes a secret: the founder fills api.env on the host, last.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_flags "$@"
require_host

MIGRATIONS_DIR="${MIGRATIONS_DIR:-$DEPLOY_DIR/../migrations}"
PG_VERSION=18
PG_CONF_D="/etc/postgresql/$PG_VERSION/main/conf.d"
# The PostgreSQL apt repository's signing key (ACCC4CF8), checked before the repository is trusted.
PGDG_FINGERPRINT=B97B0AFCAA1A47F044F244A07FCC7D46ACCC4CF8
PGDG_KEY=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc
DB=owlhead
API_USER=owlhead_api

if [ ! -d "$MIGRATIONS_DIR" ]; then
  echo "no migrations directory at $MIGRATIONS_DIR; copy migrations/ beside deploy/ or set MIGRATIONS_DIR" >&2
  exit 1
fi

say "Base packages and automatic security updates"
run apt-get update
run env DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates curl gnupg postgresql-common unattended-upgrades
run dpkg-reconfigure -f noninteractive unattended-upgrades

say "PostgreSQL $PG_VERSION from apt.postgresql.org"
if [ ! -f /etc/apt/sources.list.d/pgdg.sources ] && [ ! -f /etc/apt/sources.list.d/pgdg.list ]; then
  # Ubuntu's own postgresql-common ships this script and the repository key it installs.
  run /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y
fi
if [ "$DRY_RUN" = 0 ]; then
  found="$(gpg --show-keys --with-colons "$PGDG_KEY" | awk -F: '$1 == "fpr" { print $10; exit }')"
  if [ "$found" != "$PGDG_FINGERPRINT" ]; then
    echo "the PostgreSQL repository key is $found, not $PGDG_FINGERPRINT; stopping" >&2
    exit 1
  fi
fi
run env DEBIAN_FRONTEND=noninteractive apt-get install -y "postgresql-$PG_VERSION"
put_file "$PG_CONF_D/10-owlhead.conf" 0644 postgres:postgres <"$DEPLOY_DIR/postgres/10-owlhead.conf"
run systemctl enable postgresql
if [ "$CHANGED" = 1 ]; then
  run systemctl restart postgresql
else
  run systemctl start postgresql
fi
if [ "$DRY_RUN" = 0 ] && [ -z "$(query postgres "SELECT 1 FROM pg_hba_file_rules WHERE type = 'local' AND 'all' = ANY (user_name) AND auth_method = 'peer'")" ]; then
  echo "pg_hba.conf has no 'local all all peer' rule; the API's passwordless socket login needs it" >&2
  exit 1
fi

say "Database, roles, and schema"
if [ -z "$(query postgres "SELECT 1 FROM pg_database WHERE datname = '$DB'")" ]; then
  run runuser -u postgres -- createdb "$DB"
fi
sql "$DB" <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_owner') THEN
    CREATE ROLE mandate_journal_owner NOLOGIN;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_app') THEN
    CREATE ROLE mandate_journal_app NOLOGIN;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '$API_USER') THEN
    CREATE ROLE $API_USER LOGIN IN ROLE mandate_journal_app;
  END IF;
END
\$\$;
REVOKE ALL ON DATABASE $DB FROM PUBLIC;
REVOKE CONNECT ON DATABASE postgres FROM PUBLIC;
REVOKE CONNECT ON DATABASE template1 FROM PUBLIC;
GRANT CONNECT ON DATABASE $DB TO $API_USER;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
CREATE SCHEMA IF NOT EXISTS journal AUTHORIZATION mandate_journal_owner;
GRANT USAGE ON SCHEMA journal TO mandate_journal_app;
ALTER ROLE $API_USER IN DATABASE $DB SET search_path = journal;
CREATE SCHEMA IF NOT EXISTS deploy;
REVOKE ALL ON SCHEMA deploy FROM PUBLIC;
CREATE TABLE IF NOT EXISTS deploy.migrations (
  file text PRIMARY KEY,
  sha256 text NOT NULL,
  applied_at timestamptz NOT NULL DEFAULT now()
);
SQL

say "Migrations, in file order, each once, as mandate_journal_owner"
for file in "$MIGRATIONS_DIR"/*.sql; do
  name="$(basename "$file")"
  digest="$(sha256sum "$file" | cut -d' ' -f1)"
  applied="$(query "$DB" "SELECT sha256 FROM deploy.migrations WHERE file = '$name'")"
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
    printf '\nRESET ROLE;\nINSERT INTO deploy.migrations (file, sha256) VALUES (%s, %s);\nCOMMIT;\n' \
      "'$name'" "'$digest'"
  } | sql "$DB"
done

say "The API's system user and directories"
if ! id -u "$API_USER" >/dev/null 2>&1; then
  run useradd --system --user-group --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin "$API_USER"
fi
run install -d -o root -g root -m 0700 /etc/owlhead
run install -d -o "$API_USER" -g "$API_USER" -m 0700 /var/lib/owlhead /var/lib/owlhead/artifacts /var/lib/owlhead/vault
run install -d -o root -g root -m 0700 /var/backups/owlhead
if [ ! -f /etc/owlhead/api.env ]; then
  put_file /etc/owlhead/api.env 0600 root:root <"$DEPLOY_DIR/api.env.example"
else
  echo "keeping the existing /etc/owlhead/api.env"
  run chown root:root /etc/owlhead/api.env
  run chmod 0600 /etc/owlhead/api.env
fi

say "systemd units: the API (installed, not started) and the backup timer"
put_file /usr/local/sbin/owlhead-backup 0755 root:root <"$DEPLOY_DIR/backup.sh"
for unit in owlhead-api.service owlhead-backup.service owlhead-backup.timer owlhead-backup-failed.service; do
  put_file "/etc/systemd/system/$unit" 0644 root:root <"$DEPLOY_DIR/systemd/$unit"
done
run systemctl daemon-reload
run systemctl enable --now owlhead-backup.timer

say "Done"
cat <<'NEXT'
Next (deploy/FOUNDER-STEPS.md): copy the API binary to /usr/local/bin/mandate-api-server, run
`bash set-secrets.sh`, then `systemctl enable --now owlhead-api`. The API unit does not start
until the binary exists.
NEXT
