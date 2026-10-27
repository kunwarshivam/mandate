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
    sudo -u owlhead_api mandate agent pause --agent <AGENT> --workspace <WORKSPACE> --user <USER> \
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

# Founder steps: staging

These steps are for the hosted staging cell (DigitalOcean, `api-staging.owlhead.ai`), not the demo
host above. The same rule holds: never paste a secret, a token or an address into chat, an issue,
a commit or a command line.

## H. The Alpaca paper OAuth app ([DEC-2636](../docs/project/decisions/DEC-2636.md))

The web connect sends an Alpaca user through Alpaca's own consent page, and staging's token
exchange redeems the code. That needs an OAuth app registered by you, for paper.

1. In Alpaca's dashboard, register an OAuth app:
   - **Redirect URI**, exactly: `https://api-staging.owlhead.ai/v1/oauth/alpaca/callback`.
     Nothing else will be accepted: the API sends this URI, and Alpaca refuses any other.
   - **Scopes:** `trading` and `data`, and nothing else. The API asks for `scope=trading data` with
     `env=paper` (connections spec §5.3; `REQUESTED_SCOPES` in `mandate-connections`). Do not add
     `account:write`.
   - **Environment:** paper. No live account is ever connected (AGENTS.md rule 8).
2. Alpaca shows a **client id**, which is public, and a **client secret**, which is not.
3. **The client secret** goes in one place: GitHub, repository **Settings → Environments →
   `infra-staging` → Environment secrets → Add secret**, name `ALPACA_OAUTH_CLIENT_SECRET`. Paste it
   there only, never into a terminal or a chat.
   - The next staging `apply` reads it, masked at once.
   - It writes it to the staging vault at `platform/alpaca-oauth-client-secret`, on the standard
     input of `mandate vault put alpaca-oauth-client-secret`, through the secret-import identity.
   - Only a connection's token exchange can read it there.
   - The run prints `alpaca oauth client secret: written`. Until you add it, it prints `not set`
     and changes nothing.
   - To rotate it, replace the environment secret and run `apply` again.
4. **The client id** goes in `deploy/gitops/clusters/staging/workspace.yaml`, at
   `alpacaOAuth.clientId`, which is empty today. Either send it to the coordinator, and an agent
   opens the one-line PR, or open that PR yourself. It is public, so committing it is safe.
5. Dispatch `infra` with `apply` on `staging` at the commit that holds the client id (the `sha`
   input). Until both the client id and the secret are in place, the API offers no connect.

## I. A runner reserved for `infra` ([DEC-2614](../docs/project/decisions/DEC-2614.md))

Do these only once you have recorded DEC-2614 as Accepted. Until you set the variable in step 5,
`infra` keeps running on GitHub's hosted runner, so nothing changes before then.

1. **The machine.** A small VM that runs nothing else, Ubuntu 24.04 x86_64, with no inbound access
   at all: no open port, outbound only.
   - Size: 4 vCPU, 8 GB of memory and 60 GB of disk. The job builds xtask from source, which is
     slow on 2 vCPU.
   - For example, DigitalOcean `s-4vcpu-8gb` (about $48 a month) or a Hetzner CPX31 (about €15 a
     month). This is spending: pick one, or a machine you already run.
   - Turn on two-factor sign-in for the provider's account.
2. **A user for the runner.** Make an unprivileged user, `runner`, with passwordless `sudo`
   limited to `install` into `/usr/local/bin`. The job installs OpenTofu, kubectl and flux with
   `sudo install`. Install `curl`, `git`, `unzip` and `build-essential`, and `rustup` for that
   user.
3. **The runner itself.** As `runner`, download GitHub's runner from repository **Settings →
   Actions → Runners → New self-hosted runner → Linux x64**, following GitHub's own commands for
   the download and its checksum.
4. **Register it ephemerally, again after every job.** DEC-2614 item 3 asks for one job per
   registration, so nothing a run leaves behind reaches the next. The registration token GitHub
   shows lasts one hour and serves one registration, so re-registering after each job needs
   something that can mint a new token. **This is your choice:**
   - **(a) A supervisor with a token-minting credential.**
     - Create a fine-grained personal access token limited to this one repository, with
       *Administration: read and write* and nothing else.
     - Store it root-only on the VM (mode 0600, readable by root alone, never by `runner`).
     - Run a root systemd service that loops:
       1. use the token to request a registration token (`POST
          /repos/kunwarshivam/mandate/actions/runners/registration-token`);
       2. wipe `runner`'s home and the runner's `_work` directory;
       3. run, as `runner`, `./config.sh --url https://github.com/kunwarshivam/mandate --token
          <that token, on no command line you keep> --labels mandate-infra --ephemeral
          --unattended --replace`;
       4. run `./run.sh`, which exits after one job.

     The job never sees the access token. But the VM then holds a credential that can register
     runners and change repository settings, which DEC-2614 item 3's "holds no credential of its
     own" did not foresee. Accepting (a) means amending that item, and an agent will write the
     amendment when you choose it.
   - **(b) No standing credential.** Register by hand before each `apply`, with the token GitHub
     shows, and use the same `--ephemeral --labels mandate-infra` flags. That is one extra step per
     run, and the VM holds nothing.
5. **Turn it on.** Repository **Settings → Secrets and variables → Actions → Variables → New
   repository variable**: name `INFRA_RUNNER`, value `mandate-infra`. From then on, `infra` takes
   the reserved runner. If the runner is offline, the job waits, so delete the variable to fall
   back to GitHub's hosted runner.
6. **Check it.** Dispatch `infra` with `preflight` on `staging`. The run's *Set up job* step names
   the runner, which should be yours, and the job should start at once.

No other workflow may ask for this runner. `cargo xtask lint` refuses any `runs-on` that is not a
hosted runner outside `infra.yml`. Keep the runner registered to this repository alone.

## J. Confirming a step-up challenge in staging ([DEC-2106](../docs/project/decisions/DEC-2106.md), [DEC-2125](../docs/project/decisions/DEC-2125.md))

In paper, the web app shows a step-up challenge and an `XXXX-XXXX` code before it grants an
approval or starts an Alpaca connect, and waits for you to confirm it with the `mandate` CLI. In
staging the CLI runs as the chart's one-off `control` run: a Job in `ws-ws1` under the control
process's existing database role and ServiceAccount, the same run the kill switch uses
([RB-03](../docs/runbooks/RB-03-kill-switches.md), *In the cell*). It widens no role, no grant and
no credential, and the command reads the approval from the journal, never from the browser.

1. **Reach the cluster from your machine, once.** Staging's cluster API admits only each `infra`
   run's own runner. To use `kubectl` yourself, add your address to the `infra-staging`
   environment secret `KUBE_API_ALLOWED_CIDRS` (for example `["203.0.113.7/32"]`, no wider than
   /24; [deploy/tofu/README.md](tofu/README.md)) and dispatch `infra` with `apply` on `staging`.
   Then fetch a kubeconfig as yourself: `doctl kubernetes cluster kubeconfig save mandate-staging`.
   Remove the address again when you no longer need it.
2. **Take the values staging runs with**, and use the chart from the commit staging was last
   promoted from:

   ```bash
   helm get values workspace -n ws-ws1 --all -o yaml > /tmp/staging-values.yaml
   ```

3. **Confirm an approval's grant.** Copy the principal, agent, approval, challenge and code from
   the web app; give each run a new `run.id`:

   ```bash
   helm template workspace deploy/cell/charts/workspace -n ws-ws1 -f /tmp/staging-values.yaml \
     --show-only templates/run.yaml \
     --set run.id="stepup1" \
     --set-json 'run.args=["step-up","confirm","--workspace","01KDEM0000000000000000000W","--principal","<PRINCIPAL>","--agent","<AGENT>","--approval","<APPROVAL>","--challenge","<CHALLENGE>","--code","<CODE>","--journal","$(MANDATE_JOURNAL_DSN)","--store","$(MANDATE_ARTIFACTS)"]' \
     | kubectl apply -n ws-ws1 -f -
   kubectl logs -n ws-ws1 job/control-stepup1
   ```

4. **Confirm a connect's start** the same way, with no agent or approval:

   ```bash
   helm template workspace deploy/cell/charts/workspace -n ws-ws1 -f /tmp/staging-values.yaml \
     --show-only templates/run.yaml \
     --set run.id="connect1" \
     --set-json 'run.args=["step-up","confirm-connect","--workspace","01KDEM0000000000000000000W","--principal","<PRINCIPAL>","--challenge","<CHALLENGE>","--code","<CODE>","--journal","$(MANDATE_JOURNAL_DSN)","--store","$(MANDATE_ARTIFACTS)"]' \
     | kubectl apply -n ws-ws1 -f -
   kubectl logs -n ws-ws1 job/control-connect1
   ```

`--store` is the workspace's own artifact store, which the run mounts, not the Job's scratch
directory: a connect's confirmation stores the action it names there, beside the workspace's other
artifacts, and the Job's scratch directory is gone when the Job is.

The log names the confirmation it appended, or the one it had already appended if you run it
again. A Job that fails leaves the challenge unconfirmed and adds nothing; the challenge expires on
its own. The code is short-lived, but it is still not for chat or an
issue.

## K. The OpenRouter key, for research with a real model ([DEC-997](../docs/project/decisions/DEC-997.md), [DEC-2116](../docs/project/decisions/DEC-2116.md), [DEC-2396](../docs/project/decisions/DEC-2396.md))

Research calls a real model only through staging's model-gateway service, which reads the
OpenRouter key once from the vault, at `platform/model-provider-key`, and from nowhere else
(DEC-2020, DEC-2257). No agent asks for the key, sees it or handles it. This step is yours alone.

You chose the models with [DEC-2282](../docs/project/decisions/DEC-2282.md). The gateway's live
start reads the key and serves the vetted entries, but offers none of them until its §4.3 gate
has passed ([DEC-2216](../docs/project/decisions/DEC-2216.md)). So a key in the vault spends
nothing until IAC runs the gate, which costs at most $7.56 for both models.

1. In OpenRouter, create an API key for this purpose alone. In its settings, set the key's own
   credit limit to the monthly cap you accepted ($50, DEC-997), with a **monthly** reset (a limit
   with no reset is a lifetime limit), so the provider refuses past it even if the platform's meter
   were wrong. In your account's privacy settings, turn on **Zero
   Data Retention** only, and turn off training on your prompts.
2. **The key** goes in one place: GitHub, repository **Settings → Environments → `infra-staging`
   → Environment secrets → Add secret**, name `OPENROUTER_API_KEY`. Paste it there only, never
   into a terminal, a chat, an issue or a file.
   - The next staging `apply` reads it, masked at once.
   - It writes it to the staging vault at `platform/model-provider-key`, on the standard input of
     `mandate vault put model-provider-key`, through the secret-import identity.
   - Only the model-gateway service can read it there.
   - The run prints `model provider key: written`. Until you add it, it prints `not set` and
     changes nothing.
3. Dispatch `infra` with `apply` on `staging`.
4. **To rotate it**, replace the environment secret and run `apply` again, then restart the
   model gateway so it reads the new key (it reads the key once, at start, DEC-2257 item 3):
   `kubectl rollout restart deployment/model-gateway -n ws-ws1`. Revoke the old key in OpenRouter
   only after the restart. **To stop all model spend at once**, revoke the key in OpenRouter: every
   call then ends `credential_invalid`, the safe default, with no output, so no proposal and no
   order.

## L. Does Alpaca's free data plan carry halts? (trading spec §4.4, [DEC-2380](../docs/project/decisions/DEC-2380.md) item 2)

The executor's binding gate holds every Alpaca paper opening while the instrument's halt status is
unread (`crates/mandate-executord/src/gate.rs`, `DeployedGate::input`): no halt is ever assumed
absent (`AGENTS.md` rule 3). The fixture broker is unaffected.
Alpaca's stock stream has `statuses` and `lulds` channels, but its documentation does not say
whether the free `iex` feed carries them. One run of a probe tells, and an agent builds the status
reader only if it does.

1. Close any other Alpaca market-data stream on the key first: the free plan allows one
   connection.
2. On your own machine, with your Alpaca **paper** key (never a live one) in the environment, run
   once from the repository root:

   ```bash
   APCA_API_KEY_ID=... APCA_API_SECRET_KEY=... \
     uv run --no-project --with 'websockets==15.0.1' python scripts/alpaca-status-probe.py
   ```

   It connects only to Alpaca's market-data stream, places no order, and prints only what the stream
   answered to the sign-in and the subscription, then the message types that arrived in 20 seconds:
   types, codes and channel names, never the key, the secret, a price or a time. `websockets` is
   fetched for this run only and added to no project. Its last line is one of:
   - `result: subscription confirmed for statuses and lulds, delivery observed (...)`;
   - `result: subscription confirmed for statuses and lulds, delivery not observed`: the plan
     takes the subscription, but a halt is rare, so no status arrived to prove delivery;
   - `result: subscription not confirmed for statuses and lulds`;
   - `result: sign-in refused`, or `result: inconclusive (...)`: run it again later.
3. Paste the printed lines (they hold no secret) to the coordinator. If the subscription is not
   confirmed, Alpaca paper openings stay held. A paid feed, or reading §4.4's presumed halt (a
   dropped status feed bars only market orders, and openings are limit orders, §5.1) as allowing
   them, are your decisions: the second weakens a safety rule (DEC-79).

## M. The Alpaca news key, for the agent to read news ([DEC-2394](../docs/project/decisions/DEC-2394.md), [DEC-2277](../docs/project/decisions/DEC-2277.md))

The agent reads Alpaca's news feed with a key of its own. It never uses a trading connection's
token. The news reader reads the key once from the vault, at `platform/news-data-key`, and from
nowhere else. No agent asks for the key, sees it or handles it. This step is yours alone.

**Runnable, but nothing reads the key yet.** The vault path, `mandate vault put news-data-key` and
`infra`'s apply line have merged, so steps 1 to 4 store the key. The news reader that reads it is
still to come, and its PR says so here. Until it runs, the key sits in the vault unread. Once it
runs, and until you do this step, the reader refuses `no_key`, reads no news, and the agent works
on without it.

1. **A paper account for news alone.** In Alpaca's dashboard, open a new **paper** account for
   this purpose only, with no strategy and no connection to the platform. Alpaca's API keys carry
   no scopes, so this account is what the key can reach. Never use a live account (AGENTS.md
   rule 8), and never the key of a paper account a trading connection uses. The reader refuses a
   key whose id does not begin `PK`, Alpaca's paper prefix.
2. **Its API key.** In that paper account, generate an API key. Alpaca shows a **key id** and a
   **secret**. The secret is shown only once.
3. **Both go in one place:** GitHub, repository **Settings → Environments → `infra-staging` →
   Environment secrets → Add secret**, as `ALPACA_NEWS_KEY_ID` and `ALPACA_NEWS_SECRET`. Paste
   them there only, never into a terminal, a chat, an issue or a file.
   - The next staging `apply` reads them, masked at once.
   - It writes them to the staging vault at `platform/news-data-key`, on the standard input of
     `mandate vault put news-data-key`, through the secret-import identity.
   - Only the news reader and the data process's bars reader (DEC-2283) can read it there.
   - The run prints `news data key: written`. Until you add both, it prints `not set` and changes
     nothing.
4. Dispatch `infra` with `apply` on `staging`.
5. **To rotate it:** generate a new key in that paper account, replace both environment secrets,
   run `apply` again, then restart the news reader so it reads the new key (its PR adds the
   command here). Revoke the old key in
   Alpaca only after the restart. **To stop all news reads at once**, revoke the key in Alpaca:
   every fetch then fails with no item, so research goes on without news, and no order depends on
   it.
