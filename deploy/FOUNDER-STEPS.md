# Founder steps: the demo API host

These are your steps, in order, to bring up the API host for the demo ([DEC-822](../docs/project/decisions/DEC-822.md),
pending on PR #762). This means one Hetzner CX22 (or its current equivalent) with Ubuntu 24.04, behind a Cloudflare Tunnel, serving
`api.owlhead.ai`. Do each step only when you reach it. Agents never get access to the host,
Hetzner, or Cloudflare. Never paste a secret, a token, or the server's IP address into chat, an
issue, or the repository.

`<IP>` below stands for the server's IP address. Keep it on your machine.

## 0. Before anything else: two-factor sign-in

0. **Turn on two-factor sign-in** on your **Hetzner** and **Cloudflare** accounts, with a hardware
   security key or an authenticator app (TOTP), not SMS ([DEC-822](../docs/project/decisions/DEC-822.md)
   item 4). Hetzner's disk images hold the vault together with its keys, and Cloudflare holds the
   tunnel to the API, so whoever signs in to either reaches the host's secrets. Keep the recovery
   codes in your password manager.

## A. Hetzner Cloud console

1. **Project.** Create a project named `owlhead`.
2. **SSH key.** Under *Security → SSH keys*, add your public key, for example
   `~/.ssh/id_ed25519.pub`. If you have none, create one with `ssh-keygen -t ed25519`.
3. **Firewall.** Under *Firewalls*, create `owlhead-ssh`:
   - one inbound rule: TCP port 22, source your own IP address (a `/32`). If your IP changes and
     SSH stops answering, update this rule in the console, or use the server's *Console* button
     (a browser terminal that needs no SSH);
   - no other inbound rule;
   - leave outbound open. The tunnel, apt, and Alpaca are all outbound; the services' own units deny
     all but the hosts each needs (step 10).
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
   ssh root@<IP>      # the last time: bootstrap (step 7) turns root login off
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

   - SSH hardening (DEC-822 item 7): the admin user `owlhead_admin`, which gets the SSH key you gave
     Hetzner, and a drop-in `/etc/ssh/sshd_config.d/00-owlhead.conf` that turns off passwords and
     root login and lets only `owlhead_admin` in.

   It is safe to run again.

   **Before you close the root session,** open a second terminal on your laptop and check that the
   admin login works:

   ```bash
   ssh owlhead_admin@<IP> sudo true
   ```

   If it fails, fix it from the root session (or the Hetzner *Console* button) before you leave:
   root login is now off. From here on, log in as `owlhead_admin` and run the host commands with
   `sudo` (`sudo -i` gives a root shell).

## C. The API binary

Root login is off from here on. Every command below that is "on the host" is run as
`owlhead_admin` after `sudo -i`, which gives a root shell with `/root/owlhead` as its home.

8. **Install the binaries**, once the API lane says the build is ready. Copy these onto the host
   (x86_64 Linux builds):
   - `mandate-api-server` (the API);
   - `mandate` (the CLI, used for restore checks).

   The executor's binary comes from the live lane later; its unit is installed and stays off until
   then.

   ```bash
   scp mandate-api-server mandate owlhead_admin@<IP>:/tmp/
   ssh owlhead_admin@<IP>
   sudo install -o root -g root -m 0755 /tmp/mandate-api-server /tmp/mandate /usr/local/bin/
   rm /tmp/mandate-api-server /tmp/mandate
   ```

## D. Secrets, last, then the first start

9. **Set the secrets** on the host. Have Alpaca's OAuth app page (paper, DEC-821) open for its
   client id.

   ```bash
   cd /root/owlhead/deploy
   bash set-secrets.sh
   ```

   The script does four things, keeping every value already set and printing none:
   - it makes the vault's two keys and the session key with `openssl`, straight into
     `/etc/owlhead/credentials/` (root:root 0600; DEC-822 item 4);
   - it asks for the Alpaca client id, which is public, and fills it into both env files;
   - **the client secret, after V1 lands:** it asks for the secret (not echoed) and pipes it into
     the vault's import. The secret is never written to a file under `/etc/owlhead`. Until V1 is
     installed, the script says it skipped this; skip it too, and run the script again once V1
     lands;
   - the database URLs are already filled in; they have no password.

   Then copy both vault keys into your password manager: they are never in a backup, and without
   them a restored vault cannot be read, so every connection would need re-authorizing.
   `cat /etc/owlhead/credentials/vault-pending-key /etc/owlhead/credentials/vault-token-key` shows
   them once; clear the screen after.

   Check the files with `ls -l /etc/owlhead/credentials`: each is `root root` and `-rw-------`.
10. **Allow the API's one outbound host, then start it and check it answers locally.** The API unit
    denies all outbound traffic but loopback (DEC-822 item 5). Give it the host of your Supabase
    project (DEC-820), which it needs to verify sign-ins; use the host name only, for example
    `abcd1234.supabase.co`:

    ```bash
    cd /root/owlhead/deploy
    bash allow-egress.sh owlhead-api <supabase-project-host>
    ```

    Run it again if sign-in later stops working with a network error: the unit allows the
    addresses the name had when you ran it (see *Egress* in [README.md](README.md)). Then:

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

    The script also allows the tunnel's two Cloudflare edge names outbound (the unit denies the rest).
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

## G. The kill switch without Cloudflare (DEC-822 item 6)

Cloudflare sits in front of the API. If Cloudflare or its account has a problem, the web app cannot
reach pause or the kill switch. The fallback needs neither: the `mandate` CLI on the host, over
SSH. Try it once now, before any agent runs, and again after any change to the CLI or the host.

16. **Rehearse the fallback.** From your laptop, which needs no Cloudflare:

    ```bash
    ssh owlhead_admin@<IP>
    sudo -u owlhead_api mandate agent --help          # lists pause and kill
    ```

    Then, with your workspace id, your owner id, and a test agent's id (the commands and flags are
    in [README.md](README.md), *The kill switch without Cloudflare*):

    ```bash
    sudo -u owlhead_api mandate agent pause <AGENT> --workspace <WORKSPACE> --user <USER> \
      --journal 'postgresql://owlhead_api@localhost/owlhead?host=/var/run/postgresql' \
      --store /var/lib/owlhead/artifacts
    ```

    The rehearsal passes only when `mandate agent --help` lists `pause` and `kill` and the pause
    commits. **If `--help` lists no `pause` or `kill`, the rehearsal has failed:** the binary on the
    host predates the CLI's control commands, and the web app is then the only way to pause. Say so
    in the lane's tracker and do not start an agent on this host until it passes.

## What each backup covers

- **Nightly local backup** (`owlhead-backup.timer`, 03:30 UTC, 14 days kept): a consistent
  `pg_dump` of the database and a tarball of the artifact store and the vault, in
  `/var/backups/owlhead`. It
  protects against a bad migration, a mistaken delete, or a damaged table. It is on the same disk,
  so it does not protect against losing the VM.
- **Hetzner Backups** (daily, 7 kept): an image of the whole disk, taken while the server runs.
  This covers losing the VM, and it includes the local backups above. Being a whole-disk copy, it
  also holds `/etc/owlhead/credentials`, so an image holds the vault together with its keys:
  protect the Hetzner account (step 0) accordingly. The nightly backup never holds them. This is
  accepted for the paper-only demo; before any live credential or customer data, backups must
  exclude every secret outside the vault (DEC-822 item 4). A database restored from
  the image is crash-consistent, so after such a restore, run the restore check on the newest
  local dump.
- **Neither is off-site** beyond Hetzner's own storage. An off-site copy comes before any customer
  data.
