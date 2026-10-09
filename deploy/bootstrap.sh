#!/usr/bin/env bash
# Bootstraps the demo API host (DEC-822): Ubuntu 24.04 x86_64, one VM. Safe to run again; each
# step checks what is already there. Run as root from this directory:
#
#   bash bootstrap.sh --dry-run   # print every change, make none
#   bash bootstrap.sh
#
# It installs PostgreSQL 18 from apt.postgresql.org (localhost only), creates the database, its
# roles and schema, applies migrations/, creates the API's system user and directories, installs
# the executor unit template, the backup timer, and /etc/owlhead/api.env and executor.env from
# their examples if absent, and the empty /etc/owlhead/credentials/ (0700). Last, it hardens SSH
# (DEC-822 item 7): the admin user owlhead_admin gets root's authorized key, and an sshd_config.d
# drop-in turns off passwords and root login. It never writes a
# secret: set-secrets.sh and install-cloudflared.sh make the credentials on the host, last.
#
# Two service users (DEC-692): owlhead_api runs the API (the control services), owlhead_exec the
# connection executors, which never run inside the API process. The minimal vault's directories
# let the filesystem enforce who may read what: the API writes pending authorization codes the
# executor takes, and may unlink a token file it knows the name of, but never list or read one.
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
EXEC_USER=owlhead_exec
# The one group both service users share: the artifact store's. Neither user is ever in the other's
# own group, so the API never reads the executor's tokens.
ART_GROUP=owlhead_art
ARTIFACTS=/var/lib/owlhead/artifacts
VAULT=/var/lib/owlhead/vault
ADMIN_USER=owlhead_admin
ADMIN_KEYS="/home/$ADMIN_USER/.ssh/authorized_keys"
SSHD_DROPIN=/etc/ssh/sshd_config.d/00-owlhead.conf
SUDOERS=/etc/sudoers.d/90-owlhead-admin

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
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '$EXEC_USER') THEN
    CREATE ROLE $EXEC_USER LOGIN IN ROLE mandate_journal_app;
  END IF;
END
\$\$;
REVOKE ALL ON DATABASE $DB FROM PUBLIC;
REVOKE CONNECT ON DATABASE postgres FROM PUBLIC;
REVOKE CONNECT ON DATABASE template1 FROM PUBLIC;
GRANT CONNECT ON DATABASE $DB TO $API_USER;
GRANT CONNECT ON DATABASE $DB TO $EXEC_USER;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
CREATE SCHEMA IF NOT EXISTS journal AUTHORIZATION mandate_journal_owner;
GRANT USAGE ON SCHEMA journal TO mandate_journal_app;
ALTER ROLE $API_USER IN DATABASE $DB SET search_path = journal;
ALTER ROLE $EXEC_USER IN DATABASE $DB SET search_path = journal;
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

say "Service users, directories, and the minimal vault's layout (DEC-692)"
if ! getent group "$ART_GROUP" >/dev/null; then
  run groupadd --system "$ART_GROUP"
fi
for user in "$API_USER" "$EXEC_USER"; do
  if ! id -u "$user" >/dev/null 2>&1; then
    run useradd --system --user-group --groups "$ART_GROUP" --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin "$user"
  elif ! id -nG "$user" | tr ' ' '\n' | grep -qx "$ART_GROUP"; then
    run gpasswd --add "$user" "$ART_GROUP"
  fi
done
run install -d -o root -g root -m 0700 /etc/owlhead
# The three kinds of secret outside the vault, each a root:root 0600 file systemd loads with
# LoadCredential= (DEC-822 item 4); set-secrets.sh and install-cloudflared.sh fill it.
run install -d -o root -g root -m 0700 /etc/owlhead/credentials
run install -d -o root -g root -m 0700 /var/backups/owlhead
run install -d -o root -g root -m 0755 /var/lib/owlhead "$VAULT"
# The artifact store is written and read by both services, through the shared group. Every
# directory the store uses is made here, setgid, so each object lands in that group (and with
# UMask=0027 is group-readable); a shard one user created on demand would be closed to the other.
shards=("$ARTIFACTS" "$ARTIFACTS/tmp" "$ARTIFACTS/sha256")
for hi in 0 1 2 3 4 5 6 7 8 9 a b c d e f; do
  for lo in 0 1 2 3 4 5 6 7 8 9 a b c d e f; do
    shards+=("$ARTIFACTS/sha256/$hi$lo")
  done
done
if [ "$DRY_RUN" = 1 ]; then
  echo "+ install -d -o root -g $ART_GROUP -m 2770 $ARTIFACTS, its tmp/, sha256/, and the 256 shards sha256/00 to sha256/ff"
else
  install -d -o root -g "$ART_GROUP" -m 2770 "${shards[@]}"
fi
# pending/: the API writes the encrypted code (0640), the executor reads it and deletes it. Setgid,
# so each file the API creates is in owlhead_exec's group; without it the executor could not read it.
run install -d -o "$API_USER" -g "$EXEC_USER" -m 2770 "$VAULT/pending"
# tokens/: the executor writes tokens (0600); the API may unlink a known name but not list or read.
run install -d -o "$EXEC_USER" -g "$API_USER" -m 0730 "$VAULT/tokens"
for name in api executor; do
  if [ ! -f "/etc/owlhead/$name.env" ]; then
    put_file "/etc/owlhead/$name.env" 0600 root:root <"$DEPLOY_DIR/$name.env.example"
  else
    echo "keeping the existing /etc/owlhead/$name.env"
    run chown root:root "/etc/owlhead/$name.env"
    run chmod 0600 "/etc/owlhead/$name.env"
  fi
done

say "systemd units: the API and the executor template (installed, not started) and the backup timer"
put_file /usr/local/sbin/owlhead-backup 0755 root:root <"$DEPLOY_DIR/backup.sh"
for unit in owlhead-api.service owlhead-executor.service owlhead-backup.service owlhead-backup.timer owlhead-backup-failed.service; do
  put_file "/etc/systemd/system/$unit" 0644 root:root <"$DEPLOY_DIR/systemd/$unit"
done
run systemctl daemon-reload
run systemctl enable --now owlhead-backup.timer

say "SSH: keys only, and a non-root admin user (DEC-822 item 7)"
if ! id -u "$ADMIN_USER" >/dev/null 2>&1; then
  run useradd --create-home --shell /bin/bash --groups sudo "$ADMIN_USER"
fi
run install -d -o "$ADMIN_USER" -g "$ADMIN_USER" -m 0700 "/home/$ADMIN_USER/.ssh"
if [ "$DRY_RUN" = 1 ]; then
  echo "+ copy root's authorized_keys (the key given to Hetzner) to $ADMIN_KEYS, if that file is empty"
elif [ ! -s "$ADMIN_KEYS" ]; then
  if [ ! -s /root/.ssh/authorized_keys ]; then
    echo "root has no authorized_keys to give $ADMIN_USER; root login stays on. Add your public key to $ADMIN_KEYS and run again" >&2
    exit 1
  fi
  install -o "$ADMIN_USER" -g "$ADMIN_USER" -m 0600 /root/.ssh/authorized_keys "$ADMIN_KEYS"
fi
if [ "$DRY_RUN" = 0 ] && [ ! -s "$ADMIN_KEYS" ]; then
  echo "$ADMIN_KEYS is empty; root login stays on. Add your public key and run again" >&2
  exit 1
fi
echo "$ADMIN_USER ALL=(ALL) NOPASSWD:ALL" | put_file "$SUDOERS" 0440 root:root
run visudo -cf "$SUDOERS" || { rm -f "$SUDOERS"; exit 1; }
if [ "$DRY_RUN" = 0 ] && ! grep -qE '^Include[[:space:]]+/etc/ssh/sshd_config\.d/\*\.conf' /etc/ssh/sshd_config; then
  echo "/etc/ssh/sshd_config does not include sshd_config.d; stopping before changing SSH" >&2
  exit 1
fi
put_file "$SSHD_DROPIN" 0644 root:root <<SSHD
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
AuthenticationMethods publickey
PermitRootLogin no
AllowUsers $ADMIN_USER
X11Forwarding no
MaxAuthTries 3
SSHD
run sshd -t
run systemctl try-reload-or-restart ssh
if [ "$DRY_RUN" = 0 ]; then
  effective="$(sshd -T)"
  for setting in "passwordauthentication no" "permitrootlogin no" "kbdinteractiveauthentication no"; do
    if ! grep -qx "$setting" <<<"$effective"; then
      echo "sshd does not report '$setting'; another file wins over $SSHD_DROPIN. Fix it before closing this session" >&2
      exit 1
    fi
  done
fi

say "Done"
cat <<'NEXT'
Root SSH login is now off. BEFORE closing this session, open a second terminal and check
`ssh owlhead_admin@<IP> sudo true` works; from now on log in as owlhead_admin and use sudo.

Next (deploy/FOUNDER-STEPS.md): copy the API binary to /usr/local/bin/mandate-api-server, run
`bash set-secrets.sh`, then `systemctl enable --now owlhead-api`. The API unit does not start
until the binary exists.
NEXT
