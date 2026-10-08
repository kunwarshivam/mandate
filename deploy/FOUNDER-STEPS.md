# Founder steps: the demo API host

These are your steps, in order, to bring up the API host for the demo ([DEC-822](../docs/project/decisions/DEC-822.md),
pending on PR #762). This means one Hetzner CX22 (or its current equivalent) with Ubuntu 24.04, behind a Cloudflare Tunnel, serving
`api.owlhead.ai`. Do each step only when you reach it. Agents never get access to the host,
Hetzner, or Cloudflare. Never paste a secret, a token, or the server's IP address into chat, an
issue, or the repository.

`<IP>` below stands for the server's IP address. Keep it on your machine.

## A. Hetzner Cloud console

1. **Project.** Create a project named `owlhead`.
2. **SSH key.** Under *Security → SSH keys*, add your public key, for example
   `~/.ssh/id_ed25519.pub`. If you have none, create one with `ssh-keygen -t ed25519`.
3. **Firewall.** Under *Firewalls*, create `owlhead-ssh`:
   - one inbound rule: TCP port 22, source your own IP address (a `/32`). If your IP changes and
     SSH stops answering, update this rule in the console, or use the server's *Console* button
     (a browser terminal that needs no SSH);
   - no other inbound rule;
   - leave outbound open. The tunnel, apt, and Alpaca are all outbound.
4. **Server.** Under *Servers → Add server*:
   - pick a location;
   - image: **Ubuntu 24.04**;
   - type: **CX22**, or its current equivalent if Hetzner renamed it (shared vCPU, x86, 2 vCPU,
     4 GB). Check the price shown;
   - SSH key: the one from step 2. Set no root password;
   - firewall: `owlhead-ssh`;
   - **Backups: on**;
   - name: `owlhead-api`.

   Create it, then note its IP address on your machine only.

## B. First login and the bootstrap

5. **Log in and update.**

   ```bash
   ssh root@<IP>
   apt-get update && apt-get -y upgrade
   reboot            # only if the upgrade asks for it; then ssh in again
   ```

6. **Copy the runbook to the host.** Run this from your laptop, in a checkout of the repository
   at `main`. The host needs no GitHub access.

   ```bash
   ssh root@<IP> mkdir -p /root/owlhead
   scp -r deploy migrations root@<IP>:/root/owlhead/
   ```

7. **Bootstrap.** On the host, preview it first, then run it:

   ```bash
   cd /root/owlhead/deploy
   bash bootstrap.sh --dry-run | less
   bash bootstrap.sh
   ```

   This installs and configures:
   - PostgreSQL 18, listening on localhost only;
   - the `owlhead` database and its roles, with the migrations applied;
   - two service users, `owlhead_api` (the API) and `owlhead_exec` (the connection executors), and
     the `owlhead_art` group they share for the artifact store;
   - the vault's directories, `/var/lib/owlhead/vault/pending` and `/var/lib/owlhead/vault/tokens`,
     and the artifact store, `/var/lib/owlhead/artifacts`, each with its own owner and mode;
   - the API service and the executor template, installed but not started;
   - the nightly backup timer;
   - `/etc/owlhead/api.env` and `/etc/owlhead/executor.env`, waiting for their secrets.

   It is safe to run again.

## C. The API binary

8. **Install the binaries**, once the API lane says the build is ready. Copy these onto the host
   (x86_64 Linux builds):
   - `mandate-api-server` (the API);
   - `mandate` (the CLI, used for restore checks).

   The executor's binary comes from the live lane later; its unit is installed and stays off until
   then.

   ```bash
   scp mandate-api-server mandate root@<IP>:/tmp/
   ssh root@<IP>
   install -o root -g root -m 0755 /tmp/mandate-api-server /tmp/mandate /usr/local/bin/
   rm /tmp/mandate-api-server /tmp/mandate
   ```

## D. Secrets, last, then the first start

9. **Fill `/etc/owlhead/api.env` and `/etc/owlhead/executor.env`** on the host. Have Alpaca's OAuth
   app page (paper, DEC-821) open for its client id and secret.

   ```bash
   cd /root/owlhead/deploy
   bash set-secrets.sh
   ```

   It makes the session key and the vault's two keys with `openssl` straight into the files, asks for the
   Alpaca client id and then the secret (the secret is not echoed), and keeps every value already
   set. The secret goes only into `executor.env`: the API never holds it. The database URLs are
   already filled in; they have no password. Nothing is printed.

   Then copy both vault keys into your password manager: they are never in a backup, and without
   them a restored vault cannot be read, so every connection would need re-authorizing.
   `grep '^MANDATE_VAULT_.*_KEY=' /etc/owlhead/executor.env` shows both once; clear the screen
   after.

   If you edit the file by hand instead (`nano /etc/owlhead/api.env`): Ctrl+O, then Enter, saves;
   Ctrl+X quits. Check that it is still `root root` and `-rw-------` with
   `ls -l /etc/owlhead/api.env`.
10. **Start the API and check it answers locally:**

    ```bash
    systemctl enable --now owlhead-api
    systemctl status owlhead-api --no-pager
    curl -i http://127.0.0.1:8080/
    journalctl -u owlhead-api -n 50 --no-pager   # if it did not start
    ```

## E. Cloudflare Tunnel, only once the API answers

11. **Create the tunnel.** `owlhead.ai` must already be a zone in your Cloudflare account.
    Cloudflare moves its menus, so these are the outcomes to reach rather than exact clicks:
    - a **Cloudflare Tunnel** of type *cloudflared*, named `owlhead-api` (under Zero Trust, in the
      Networks or Tunnels section);
    - on its connector-install page, the **tunnel token**: the long string after
      `service install` in the command shown. Copy only that string. Do not run the command.
12. **Run the connector on the host.** Paste the token when the script asks for it; it does not
    echo, and it refuses anything that is not a single token (for example the whole command).

    ```bash
    cd /root/owlhead/deploy
    bash install-cloudflared.sh
    ```

    The dashboard should show the tunnel as *Healthy*.
13. **Public hostname.** On the tunnel, add a public hostname (its *Public Hostname* or *Routes*
    tab):
    - subdomain `api`, domain `owlhead.ai`;
    - service type **HTTP**, URL **`127.0.0.1:8080`**. Use `127.0.0.1`, not `localhost`: the API
      listens on IPv4 loopback only, and `localhost` can resolve to IPv6 first.

    Saving creates the `api.owlhead.ai` DNS record. Check it from your laptop with
    `curl -i https://api.owlhead.ai/`.

## F. Backups, then a restore check

14. **Run one backup now** and confirm the timer is set:

    ```bash
    systemctl start owlhead-backup.service
    ls -l /var/backups/owlhead
    systemctl list-timers owlhead-backup.timer --no-pager
    ```

    A failed backup logs an error. Check weekly, until notifications exist:
    `journalctl -p err -t owlhead-backup --since -7d --no-pager` should print nothing.

15. **Prove the backup restores.** Do this now, then monthly:

    ```bash
    cd /root/owlhead/deploy
    bash restore-check.sh /var/backups/owlhead/owlhead-<stamp>.dump /var/backups/owlhead/state-<stamp>.tar.gz
    ```

    It restores into a scratch database, runs `mandate journal verify` on every stream, and checks
    each one ends at its recorded head. The run passes only if its last line starts with
    `restore check passed`. Before the first agent runs, the journal is empty, and the last line
    says so: `restore check passed: the restored journal is empty`.

    To restore for real, after losing the VM or a bad migration, follow *Restore after losing the
    VM or a bad migration* in [README.md](README.md).

## What each backup covers

- **Nightly local backup** (`owlhead-backup.timer`, 03:30 UTC, 14 days kept): a consistent
  `pg_dump` of the database and a tarball of the artifact store and the vault, in
  `/var/backups/owlhead`. It
  protects against a bad migration, a mistaken delete, or a damaged table. It is on the same disk,
  so it does not protect against losing the VM.
- **Hetzner Backups** (daily, 7 kept): an image of the whole disk, taken while the server runs.
  This covers losing the VM, and it includes the local backups above. Being a whole-disk copy, it
  also holds `/etc/owlhead`, so an image holds the vault together with its key and the client
  secret: protect the Hetzner account (two-factor sign-in) accordingly. The nightly backup never
  holds them. A database restored from
  the image is crash-consistent, so after such a restore, run the restore check on the newest
  local dump.
- **Neither is off-site** beyond Hetzner's own storage. An off-site copy comes before any customer
  data.
