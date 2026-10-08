# deploy/: the demo API host

This directory has the runbook, scripts, and systemd units for the one VM that serves the demo's
workspace API ([DEC-822](../docs/project/decisions/DEC-822.md), pending on PR #762). That VM is a
Hetzner CX22 with Ubuntu 24.04 x86_64, behind a Cloudflare Tunnel, serving `api.owlhead.ai`.

The founder runs everything here; agents never get host access. The order of the steps is in
[FOUNDER-STEPS.md](FOUNDER-STEPS.md).

| File | What it does |
|---|---|
| `bootstrap.sh` | PostgreSQL 18 from apt.postgresql.org, with its key checked by fingerprint, listening on localhost only. Creates the `owlhead` database, the journal's two roles, the `owlhead_api` login role, and the `journal` schema, then applies `../migrations/`. Creates the API's system user and directories, installs the API unit (not started) and the backup timer, and copies `api.env.example` to `/etc/owlhead/api.env` if that file is absent |
| `install-cloudflared.sh` | cloudflared from pkg.cloudflare.com, and the tunnel as a service. The token is read without echo into `/etc/owlhead/cloudflared.env` (0600) |
| `backup.sh` | Installed as `/usr/local/sbin/owlhead-backup`: a nightly `pg_dump` and artifact-store tarball, 14 days kept |
| `restore-check.sh` | Restores a dump into a scratch database, then runs `mandate journal export` and `mandate journal verify` on every stream |
| `api.env.example` | The API's environment variables, by **name only**, with how to make each value |
| `systemd/owlhead-api.service` | The API unit: dedicated user, loopback bind, hardened. **The template** for the runtime and executor units |
| `systemd/owlhead-backup.{service,timer}`, `systemd/cloudflared.service` | The backup job and the tunnel |
| `postgres/10-owlhead.conf` | `listen_addresses = 'localhost'` |
| `lib.sh` | Shared helpers. Every change goes through `run` or `put_file`, so `--dry-run` prints the plan and changes nothing |

## Choices worth knowing

- **No database password.** The API connects over the Unix socket as `owlhead_api` by peer
  authentication, so its database URL holds no secret. The role can read and append the journal
  tables and nothing else: it cannot update or delete events, create tables, or read the
  `deploy` schema.
- **Migrations are tracked in `deploy.migrations`,** with each file's SHA-256. An applied file
  whose content later changes stops the bootstrap, since a migration is never edited.
  `mandate-journal-pg`'s embedded sqlx migrator keeps its own table. When a migration command
  that uses it ships, it must take over from this table rather than run beside it.
- **Secrets stay on the host.** `/etc/owlhead/api.env` and `/etc/owlhead/cloudflared.env` are
  root:root 0600. systemd reads them before it drops to the service user. The tunnel token is in
  an environment file, not in the world-readable unit or on a command line.
- **The tunnel targets `127.0.0.1:8080`, not `localhost`,** because the API binds IPv4 loopback
  only.

## Checks

- **ShellCheck.** The lint job (`cargo xtask ci lint`) runs ShellCheck over every `*.sh` here, as
  it does over `.github/scripts/`.
- **Dry run.** `bash bootstrap.sh --dry-run` and `bash install-cloudflared.sh --dry-run` run
  without root and print every command, file, and SQL statement they would apply.
