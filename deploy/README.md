# deploy/: the demo API host

This directory has the runbook, scripts, and systemd units for the one VM that serves the demo's
workspace API ([DEC-822](../docs/project/decisions/DEC-822.md), pending on PR #762). That VM is a
Hetzner CX22 with Ubuntu 24.04 x86_64, behind a Cloudflare Tunnel, serving `api.owlhead.ai`.

The founder runs everything here; agents never get host access. The order of the steps is in
[FOUNDER-STEPS.md](FOUNDER-STEPS.md).

| File | What it does |
|---|---|
| `bootstrap.sh` | PostgreSQL 18 from apt.postgresql.org, with its key checked by fingerprint, listening on localhost only. Creates the `owlhead` database, the journal's two roles, the `owlhead_api` login role, and the `journal` schema, then applies `../migrations/`. Creates the API's system user and directories, installs the API unit (not started) and the backup timer, and copies `api.env.example` to `/etc/owlhead/api.env` if that file is absent |
| `install-cloudflared.sh` | cloudflared from pkg.cloudflare.com, its key pinned by fingerprint, and the tunnel as a service. The token is checked for shape and read without echo into `/etc/owlhead/cloudflared.env` (0600) |
| `set-secrets.sh` | Fills `/etc/owlhead/api.env` on the host: random keys from openssl, the Alpaca id and secret asked for. It never prints a value or puts one on a command line |
| `backup.sh` | Installed as `/usr/local/sbin/owlhead-backup`: after a disk-space check, a nightly `pg_dump` and a tarball of `/var/lib/owlhead` (the artifact store and the vault), 14 days kept. A failure runs `owlhead-backup-failed.service`, which logs an error-priority line |
| `restore-check.sh` | Restores a dump into a scratch database, runs `mandate journal export` and `mandate journal verify` on every stream, and checks each verified span ends at the head `stream_heads` records |
| `api.env.example` | The API's environment variables by name, with how to make each value; the database URL, which holds no secret, is the only value. **The names are provisional: the API server (A2) takes them from this file** |
| `tests/test_invariants.py` | The security properties below, asserted on the committed files, each shown failing on a seeded bug; CI's pytest runs it |
| `systemd/owlhead-api.service` | The API unit: dedicated user, loopback bind, hardened. **The template** for the runtime and executor units |
| `systemd/owlhead-backup.{service,timer}`, `systemd/cloudflared.service` | The backup job and the tunnel |
| `postgres/10-owlhead.conf` | `listen_addresses = 'localhost'` |
| `lib.sh` | Shared helpers. Every change goes through `run` or `put_file`, so `--dry-run` prints the plan and changes nothing |

## Choices worth knowing

- **No database password.** The API connects over the Unix socket as `owlhead_api` by peer
  authentication (the bootstrap checks `pg_hba.conf` still has its `local all all peer` rule), so
  its database URL holds no secret. `owlhead_api` is a login role that is a member of
  `mandate_journal_app` and inherits its privileges: it is the application role
  `mandate-journal-pg` describes, reached through membership. It can read and append the journal
  tables and nothing else: it cannot update or delete events, create tables, or read the `deploy`
  schema. `PUBLIC` may not connect to `postgres` or `template1`.
- **Migrations are tracked in `deploy.migrations`,** with each file's SHA-256. An applied file
  whose content later changes stops the bootstrap, since a migration is never edited.
  `mandate-journal-pg`'s embedded sqlx migrator keeps its own table. When a migration command
  that uses it ships, it must take over from this table rather than run beside it.
- **Secrets stay on the host.** `/etc/owlhead/api.env` and `/etc/owlhead/cloudflared.env` are
  root:root 0600. systemd reads them before it drops to the service user. The tunnel token is in
  an environment file, not in the world-readable unit or on a command line.
- **The tunnel targets `127.0.0.1:8080`, not `localhost`,** because the API binds IPv4 loopback
  only. The bind is the unit's `--bind 127.0.0.1:8080` argument, never an environment variable,
  which `EnvironmentFile=` would override.
- **The minimal vault** (V1) lives in `/var/lib/owlhead/vault`, owned by `owlhead_api`, mode
  0700, encrypted with `MANDATE_VAULT_KEY`. The nightly backup carries it. Hetzner's disk images
  carry it **and** `/etc/owlhead/api.env`, so an image holds the vault and its key together:
  treat the images as secret.
- **What the restore check cannot see:** a stream whose events and `stream_heads` row were both
  deleted leaves nothing in the database. Only the cold segments and anchors (journal spec §6.2,
  §10, §11) show that, and they are not on this host yet.

## Restore after losing the VM or a bad migration

On a fresh host (after losing the VM) or on the same one (after a bad migration), as root, with
`deploy/` and `migrations/` in `/root/owlhead` (FOUNDER-STEPS steps 1 to 6) and the backup files in
`/root/restore`:

```bash
systemctl stop owlhead-api 2>/dev/null || true
cd /root/owlhead/deploy && bash bootstrap.sh            # roles, schema, units; safe on a used host
runuser -u postgres -- dropdb --if-exists owlhead
runuser -u postgres -- createdb owlhead
runuser -u postgres -- pg_restore --exit-on-error -d owlhead < /root/restore/owlhead-<stamp>.dump
bash bootstrap.sh                                         # re-applies grants and settings, skips migrations
install -d -o owlhead_api -g owlhead_api -m 0700 /var/lib/owlhead
tar -C /var/lib/owlhead -xzf /root/restore/state-<stamp>.tar.gz   # artifacts/ and vault/
chown -R owlhead_api:owlhead_api /var/lib/owlhead && chmod -R go= /var/lib/owlhead
bash restore-check.sh /root/restore/owlhead-<stamp>.dump /root/restore/state-<stamp>.tar.gz
```

Then put back `/etc/owlhead/api.env` from your password manager (the vault key must be the one the
vault was written with), and start the API (FOUNDER-STEPS step 10). After a whole-disk image
restore instead, run only the last line on the newest dump: an image is crash-consistent.

## Checks

- **ShellCheck.** The lint job (`cargo xtask ci lint`) runs ShellCheck over every `*.sh` here, as
  it does over `.github/scripts/`.
- **Security invariants.** `tests/test_invariants.py`, run by CI's pytest (`python/pyproject.toml`
  lists it): strict mode in every script, the env files 0600 root:root, the API unit's loopback
  bind and hardening, PostgreSQL on localhost only, the API role's minimal grants, the tunnel token
  never shown, names-only `api.env.example`, the restore check's head comparison, and the backup
  covering the vault. Each has a seeded bug the test shows caught.
- **Dry run.** `bash bootstrap.sh --dry-run` and `bash install-cloudflared.sh --dry-run` run
  without root and print every command, file, and SQL statement they would apply.
