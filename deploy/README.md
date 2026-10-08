# deploy/: the demo API host

This directory has the runbook, scripts, and systemd units for the one VM that serves the demo's
workspace API ([DEC-822](../docs/project/decisions/DEC-822.md), pending on PR #762). That VM is a
Hetzner CX22 with Ubuntu 24.04 x86_64, behind a Cloudflare Tunnel, serving `api.owlhead.ai`.

The founder runs everything here; agents never get host access. The order of the steps is in
[FOUNDER-STEPS.md](FOUNDER-STEPS.md).

| File | What it does |
|---|---|
| `bootstrap.sh` | PostgreSQL 18 from apt.postgresql.org, with its key checked by fingerprint, listening on localhost only. Creates the `owlhead` database, the journal's two roles, the `owlhead_api` login role, and the `journal` schema, then applies `../migrations/`. Creates the API's system user and directories, installs the API unit (not started) and the backup timer, and copies `api.env.example` to `/etc/owlhead/api.env` if that file is absent. Last, hardens SSH: the admin user `owlhead_admin` and an `sshd_config.d` drop-in (keys only, no root login) |
| `allow-egress.sh` | Adds the addresses of the named hosts to one unit's outbound allow list, as a systemd drop-in. See *Egress* below |
| `install-cloudflared.sh` | cloudflared from pkg.cloudflare.com, its key pinned by fingerprint, and the tunnel as a service. The token is checked for shape and read without echo into the credential `/etc/owlhead/credentials/tunnel-token` (0600), which the unit loads with `LoadCredential=` |
| `set-secrets.sh` | Makes the vault's two keys and the session key with openssl, straight into `/etc/owlhead/credentials/` (0600), asks for the Alpaca client id and fills it into both env files, and, once V1 is installed, pipes the client secret into the vault's import. It never prints a value or puts one on a command line |
| `backup.sh` | Installed as `/usr/local/sbin/owlhead-backup`: after a disk-space check, a nightly `pg_dump` and a tarball of `/var/lib/owlhead` (the artifact store and the vault), 14 days kept. A failure runs `owlhead-backup-failed.service`, which logs an error-priority line |
| `restore-check.sh` | Restores a dump into a scratch database, runs `mandate journal export` and `mandate journal verify` on every stream, and checks each verified span ends at the head `stream_heads` records |
| `api.env.example`, `executor.env.example` | The API's and the executor's non-secret settings by name; the database URLs and the vault directory are the only values. No secret is ever an environment variable (DEC-822 item 4). **The names are provisional: the API server (A2), the executor, and V1 take them from these files** |
| `tests/test_invariants.py` | The security properties below, asserted on the committed files, each shown failing on a seeded bug; CI's pytest runs it |
| `systemd/owlhead-api.service` | The API unit: `owlhead_api`, loopback bind, hardened. **The template** for the runtime unit |
| `systemd/owlhead-executor.service` | The executor template: `owlhead_exec`, `executor.env`, the vault's two keys as credentials, the same hardening, no listening port; installed, never enabled |
| `systemd/owlhead-backup.{service,timer}`, `systemd/cloudflared.service` | The backup job and the tunnel |
| `postgres/10-owlhead.conf` | `listen_addresses = 'localhost'` |
| `lib.sh` | Shared helpers. Every change goes through `run` or `put_file`, so `--dry-run` prints the plan and changes nothing |

## Choices worth knowing

- **Vault key, two files.** DEC-822 item 4 lists three values outside the vault: "the vault key",
  the session signing key, and the tunnel token. The vault key is one kind of secret held as two
  files, one per purpose ([DEC-692](../docs/project/decisions/DEC-692.md) item 2:
  `vault-pending-key` and `vault-token-key`, "together DEC-822's single vault key"). That adds no
  kind of secret outside the vault, so the count of kinds is still three. The reading was recorded
  there under DEC-176 and was not repeated.

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
- **Secrets are systemd credentials, never environment variables** (DEC-822 item 4). Exactly three
  kinds live outside the vault, each a root:root 0600 file under `/etc/owlhead/credentials/`
  (0700):
  - the vault's keys: `vault-pending-key` and `vault-token-key`;
  - `session-signing-key`;
  - `tunnel-token`.

  Each unit loads only its own with `LoadCredential=`, and reads them from
  `$CREDENTIALS_DIRECTORY`:

  | Unit | Credentials |
  |---|---|
  | API | the pending key and the session key |
  | Executor | the pending key and the token key |
  | cloudflared | the tunnel token, through `--token-file` |

  systemd copies each into a directory only that service can read, so no credential is in a
  process environment, a unit file, or a command line. The env files hold settings only: the
  database URL, `MANDATE_VAULT_DIR`, and `ALPACA_OAUTH_CLIENT_ID`. **The Alpaca client secret is in
  the vault,** imported once through V1's `mandate vault import alpaca-oauth-client-secret` from
  stdin; it is never a file under `/etc/owlhead`. Until V1 lands that step is skipped.
- **The tunnel targets `127.0.0.1:8080`, not `localhost`,** because the API binds IPv4 loopback
  only. The bind is the unit's `--bind 127.0.0.1:8080` argument, never an environment variable,
  which `EnvironmentFile=` would override.
- **Two service users** (DEC-692): `owlhead_api` runs the API (the control services) and
  `owlhead_exec` the connection executors, which never run inside the API. Each has its own
  peer-authenticated database role, a member of `mandate_journal_app`, and its own env file:
  `/etc/owlhead/api.env` and `/etc/owlhead/executor.env`, both root:root 0600. Only the executor
  reads the Alpaca client secret from the vault (infrastructure §5.1). `owlhead-executor.service` is a template
  installed but never enabled; the live lane supplies its binary.
- **The minimal vault** (V1) lives in `MANDATE_VAULT_DIR`, `/var/lib/owlhead/vault` (root, 0755),
  and V1 will refuse to start if these owners and modes differ, or if a vault key appears in its
  environment. Two keys (DEC-692), both credentials:
  - `vault-pending-key`, loaded by the API and the executor, encrypts `pending/`;
  - `vault-token-key`, loaded by the executor only, encrypts `tokens/`. The API can therefore unlink a token but never write
  one the executor would accept, and V1 will also check each token file is `owlhead_exec`'s, mode
  0600, before using it.
  - `pending/` is `owlhead_api:owlhead_exec` 2770: the API writes an encrypted authorization code
    (0640), and the executor reads it and deletes it. The setgid bit puts each new file in
    `owlhead_exec`'s group; without it the file would carry the API's group and the executor could
    not read it;
  - `tokens/` is `owlhead_exec:owlhead_api` 0730: the executor writes tokens (0600), and the API
    can unlink a token file it knows the name of, but cannot list the directory or read a token.

  The artifact store, `/var/lib/owlhead/artifacts`, is shared through a third group,
  `owlhead_art`, whose only members are the two service users: it, `tmp/`, `sha256/`, and all 256
  shards are `root:owlhead_art` 2770, made by bootstrap, so every object is group-readable by both
  services. Neither user is in the other's own group.
- **Backups never hold a credential** (infrastructure OPS-1). The nightly tarball carries the
  encrypted vault but never `/etc/owlhead`, so the three kinds of credential are not in it. Keep
  both vault keys in a password manager; restoring without them means every connection is
  re-authorized.
- **The image residual** ([DEC-822](../docs/project/decisions/DEC-822.md) item 4). Hetzner's
  whole-disk images necessarily hold `/etc/owlhead/credentials` and the vault together, so an image
  holds the vault with its keys. The founder accepted this for the **paper-only** demo. Before any
  live credential or any customer data reaches this host, backups must exclude every secret
  outside the vault: images off, or the credentials moved off the imaged disk. Until then, protect
  the Hetzner account with two-factor sign-in (FOUNDER-STEPS step 0).
- **What the restore check cannot see:** a stream whose events and `stream_heads` row were both
  deleted leaves nothing in the database. Only the cold segments and anchors (journal spec §6.2,
  §10, §11) show that, and they are not on this host yet.

## Egress (DEC-822 item 5)

Every service unit sets `IPAddressDeny=any` and `IPAddressAllow=localhost`, so a service reaches
only loopback unless a drop-in adds more. Postgres is reached over its Unix socket, which no IP rule
touches. `allow-egress.sh` writes the drop-in, one unit at a time, from the names a founder step
gives it:

| Unit | Outbound allowed |
|---|---|
| `owlhead-api` | loopback, plus the Supabase project host's addresses (OIDC and JWKS, DEC-820), set in FOUNDER-STEPS step 10 |
| `owlhead-executor` | loopback only until the live lane sets the hosts DEC-821 allows (Alpaca's OAuth and paper API) with `allow-egress.sh owlhead-executor <hosts>`. The template is never enabled by bootstrap |
| `cloudflared` | loopback, plus the addresses of `region1.v2.argotunnel.com` and `region2.v2.argotunnel.com`, set by `install-cloudflared.sh` |
| backup units | loopback only |

The runtime, when it joins this host, gets a unit copied from the API's with no `allow-egress.sh`
call: no route to a broker or the internet.

**Residual.** systemd filters by address, not by name or port. The allowed addresses are what each
name resolves to when `allow-egress.sh` runs; Supabase and Cloudflare sit behind CDNs, so a name
can later resolve elsewhere, and a service then fails to connect until the script is run again.
That fails closed. It also allows every port at an allowed address, and the addresses of a CDN
serve other tenants. Port-level or name-level egress would need a host firewall (nftables) or a
proxy, which this PR does not add. A founder-visible check: `systemctl show owlhead-api -p
IPAddressAllow -p IPAddressDeny`.

## SSH (DEC-822 items 6 and 7)

`bootstrap.sh` ends by creating `owlhead_admin` (in group `sudo`, with passwordless `sudo`, since the
account has no password and logs in by key only) and giving it the key Hetzner put in root's
`authorized_keys`. It then writes `/etc/ssh/sshd_config.d/00-owlhead.conf`:

- `PasswordAuthentication no`, `KbdInteractiveAuthentication no`, `AuthenticationMethods publickey`;
- `PermitRootLogin no` and `AllowUsers owlhead_admin`;
- `MaxAuthTries 3`, `X11Forwarding no`.

The `00-` prefix makes it win over cloud-init's `50-cloud-init.conf`, since sshd keeps the first value
it reads. The script stops before touching SSH if the admin has no key, checks the file with `sshd
-t`, and reads the effective settings back with `sshd -T`. Unattended security updates are enabled
in the same script, and the provider firewall is the only inbound path (FOUNDER-STEPS step 3).

## The kill switch without Cloudflare (DEC-822 item 6)

The web app's pause and kill switch go through Cloudflare. The fallback needs only SSH:

```bash
ssh owlhead_admin@<IP>
sudo -u owlhead_api mandate agent pause <AGENT> --workspace <WORKSPACE> --user <USER> \
  --journal 'postgresql://owlhead_api@localhost/owlhead?host=/var/run/postgresql' \
  --store /var/lib/owlhead/artifacts
sudo -u owlhead_api mandate agent kill --help     # the scope flags: agent, connection, or workspace
```

The CLI commits one control-stream event per command ([the CLI module](../crates/mandate-cli/src/agent.rs):
`pause` needs no code; a kill switch is never refused for a missing code). The login is by peer
authentication on the Unix socket as `owlhead_api`, so no password is involved. FOUNDER-STEPS step 16
rehearses it once.

## Deferred

- **The CLI's `pause` and `kill` subcommands are not in the binary yet.** `crates/mandate-cli` has
  the code (`agent.rs`) but its `agent` subcommand offers only `deploy` today. The fallback above is
  documented as the intended path, and step 16 fails closed: the rehearsal does not pass until
  `mandate agent --help` lists `pause` and `kill`. The exact flags of `kill` are then read from
  `--help`, not from this file. Nothing in this PR can rehearse it, because agents have no host.
- **Egress by name or port** (see *Egress*): not expressible in systemd units.
- **A live deployment** needs a new decision for the kill switch and for Cloudflare's place in
  front of the API (DEC-822 item 6).

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
tar -C /var/lib/owlhead --same-owner -xpzf /root/restore/state-<stamp>.tar.gz   # artifacts/ and vault/, owners and modes kept
bash bootstrap.sh                                         # re-asserts the vault's layout (DEC-692)
bash restore-check.sh /root/restore/owlhead-<stamp>.dump /root/restore/state-<stamp>.tar.gz
```

Then put the vault keys back from your password manager, as root:root 0600 files
`/etc/owlhead/credentials/vault-pending-key` and `vault-token-key`. They must be the ones the vault
was written with; without them, re-authorize every connection instead. Then run
`bash set-secrets.sh` for the rest, and start the API (FOUNDER-STEPS step 10). After a whole-disk image
restore instead, run only the last line on the newest dump: an image is crash-consistent.

## Checks

- **ShellCheck.** The lint job (`cargo xtask ci lint`) runs ShellCheck over every `*.sh` here, as
  it does over `.github/scripts/`.
- **Security invariants.** `tests/test_invariants.py`, run by CI's pytest (`python/pyproject.toml`
  lists it): strict mode in every script, the env files 0600 root:root, the API unit's loopback
  bind and hardening, PostgreSQL on localhost only, the API role's minimal grants, the tunnel token
  never shown, names-only `api.env.example`, the restore check's head comparison, and the backup
  covering the vault, default-deny egress in every service unit, and the sshd settings. Each has a seeded bug the test shows caught.
- **Dry run.** `bash bootstrap.sh --dry-run` and `bash install-cloudflared.sh --dry-run` run
  without root and print every command, file, and SQL statement they would apply.
