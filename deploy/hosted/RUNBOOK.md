# Runbook: stand up cell #1 (E21-51)

For the founder, or the coordinator on the founder's behalf, with the founder's own tokens. Run
from a checkout of `main` on your Mac. Every command is exact; no value in this file is a secret.
Each token goes into a file only you can read, by pasting it at a prompt that does not echo; no
step prints one. Paper and demo data only (DEC-995, DEC-2001).

Replace `owlhead.ai` below if the zone differs, and `you@example.com` with each operator's email
(repeat `--access-email` for each).

## 0. Gates: stop unless all four hold

1. **Every C1 drill has passed on `main`** (DEC-990 item 2): the latest `ci` run on `main` shows
   the `cell` job green, with the drills `boot`, `journal`, `compose`, `restart` and `egress` held
   (and `kill-switch` skipped by name until its lanes land).
2. **DEC-995 is Accepted**: you have posted its approval on its PR. It approves the spend.
3. **DEC-971 item 7(b) is answered** in your own words: may hosted cell #1, paper and demo only,
   run unsigned builds before the hardware key is wired? `cluster` installs with
   `deploymentTier: dev` (DEC-2001 item 6); if the answer is no, stop before step 6.
4. **DEC-2001 item 7**: you accept `doctl` (or do step 4 by hand in the DigitalOcean console from
   the dry run's plan).

## 1. Tools, each checked

```bash
mkdir -p ~/cell1-tools && cd ~/cell1-tools
curl -sSfLO https://github.com/digitalocean/doctl/releases/download/v1.186.0/doctl-1.186.0-darwin-arm64.tar.gz
echo "ab637f9fe053c98a283e97dd44f818210e3b5aaf9c8e60ec920709ba20e0b892  doctl-1.186.0-darwin-arm64.tar.gz" | shasum -a 256 -c
tar xzf doctl-1.186.0-darwin-arm64.tar.gz && sudo install -m 0755 doctl /usr/local/bin/doctl
curl -sSfLO https://github.com/cloudflare/cloudflared/releases/download/2026.9.3/cloudflared-darwin-arm64.tgz
echo "587c2cfb1c230fe36c7fa7727da78be459dae028cabe8c001291999350f07095  cloudflared-darwin-arm64.tgz" | shasum -a 256 -c
tar xzf cloudflared-darwin-arm64.tgz && sudo install -m 0755 cloudflared /usr/local/bin/cloudflared
curl -sSfLo kubectl https://dl.k8s.io/release/v1.35.0/bin/darwin/arm64/kubectl
echo "cf699c56340dc775230fde4ef84237d27563ea6ef52164c7d078072b586c3918  kubectl" | shasum -a 256 -c
sudo install -m 0755 kubectl /usr/local/bin/kubectl
```

helm 4.3.0 from the table in [`deploy/cell/README.md`](../cell/README.md#on-a-laptop). Then
`doctl version`, `cloudflared --version`, `kubectl version --client` and `helm version` should
print 1.186.0, 2026.9.3, v1.35.0 and v4.3.0, and `python3 --version` 3.9 or later (macOS's own
is). These are darwin-arm64 builds; on an Intel Mac take the `darwin-amd64` assets of the same
releases and check them against each release's published checksums.

## 2. Accounts and tokens

Make each token in its provider's dashboard with exactly these permissions, then save it:

| File | Token | Permissions |
|---|---|---|
| `cloudflare-api-token` | Cloudflare, *My Profile → API Tokens → Create Token → Custom* | Account: *Cloudflare Tunnel: Edit*, *Access: Apps and Policies: Edit*; Zone `owlhead.ai`: *DNS: Edit*. Nothing else |
| `ghcr-read-token` | GitHub, *Settings → Developer settings → Personal access tokens (classic)* | `read:packages` only, 90-day expiry |
| `r2-access-key-id`, `r2-secret-access-key` | Cloudflare, *R2 → Manage API tokens → Create* | *Object Read & Write*, bucket `mandate-cell1-journal-cold` only |

Save each one with this, pasting the value at the silent prompt:

```bash
mkdir -m 700 -p ~/.mandate-cell1-secrets
save() { umask 077; printf 'paste %s, then Enter: ' "$1"; IFS= read -rs v; echo; printf '%s\n' "$v" > ~/.mandate-cell1-secrets/"$1"; unset v; }
save cloudflare-api-token
save ghcr-read-token
save r2-access-key-id
save r2-secret-access-key
```

DigitalOcean: `doctl auth init --context mandate-cell1` (it prompts for the token without echo),
then `doctl auth switch --context mandate-cell1`. doctl keeps it; `cell1.py` never reads it.

The SSH key for the admin user, kept on your Mac:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/mandate-cell1 -C cell1-admin
```

The non-secret ids: your Cloudflare account id (*Dashboard → Account home*, right column) and the
zone id of `owlhead.ai` (*the zone's Overview*, right column).

```bash
export CF_ACCOUNT_ID=<account id>
export CF_ZONE_ID=<zone id>
export R2_ENDPOINT="https://${CF_ACCOUNT_ID}.r2.cloudflarestorage.com"
S=~/.mandate-cell1-secrets
cell1() {
  python3 deploy/hosted/cell1.py "$@" --zone owlhead.ai --cf-account-id "$CF_ACCOUNT_ID" \
    --cf-zone-id "$CF_ZONE_ID" --cf-token-file "$S/cloudflare-api-token" \
    --access-email you@example.com --ssh-public-key ~/.ssh/mandate-cell1.pub \
    --ghcr-user <your GitHub user> --ghcr-token-file "$S/ghcr-read-token" \
    --r2-endpoint "$R2_ENDPOINT" --r2-access-key-id-file "$S/r2-access-key-id" \
    --r2-secret-access-key-file "$S/r2-secret-access-key" --published ~/cell1-tools/PUBLISHED
}
```

## 3. The images and the dry run

Download the multi-arch `PUBLISHED` list of the `release` run on `main` whose commit you deploy
(*Actions → release → the run → Artifacts → `published-<commit>`*), unzip it to
`~/cell1-tools/PUBLISHED`. Then read the whole plan; nothing is called:

```bash
cell1 --dry-run all | less
```

## 4. Cloudflare, then the droplets (this spends money)

```bash
cell1 cloudflare
cell1 droplets
```

`droplets` makes the tag and the firewall first, then the VPC, the SSH key, two 40 GiB volumes
(about $4 a month each) and two `s-4vcpu-8gb` droplets (about $48 a month each). Run it again if
it stops: it lists before it creates.

## 5. The nodes

```bash
cell1 nodes
```

The first SSH opens your browser for Access, once. It waits for cloud-init on both nodes, starts
k3s on cell1-a, moves the join token to cell1-b over SSH, starts the agent, and saves the
kubeconfig to `~/.mandate-cell1/kubeconfig` (0600). From now on:
`ssh -F ~/.mandate-cell1/ssh_config cell1-a` (or `cell1-b`).

**If cloud-init failed.** `nodes` stops with cloud-init's own error. cell1-a starts its ops tunnel
before anything else, so you can usually still SSH in and read
`sudo cat /var/log/cloud-init-output.log`. If SSH to cell1-a never answers (the tunnel itself did
not start), there is no other way in, by design: delete that droplet and its volume
(`doctl compute droplet delete <name>`, `doctl compute volume delete <name>-data`) and run
`cell1 droplets` again, which makes only what is missing.

## 6. The cluster, then the check

Only after gate 3.

```bash
cell1 cluster --accept-dev-tier
cell1 check
```

`cluster` refuses to run without `--accept-dev-tier`, the flag that says gate 3 holds.

`cluster` makes the namespace's Secrets from your files on kubectl's stdin, then installs the
CloudNativePG operator, the journal's Postgres and the workspace chart, and waits for the
migration Job. `check` requires `https://api.owlhead.ai/` to send an unauthenticated caller to
Access.

To use kubectl later: `cloudflared access tcp --hostname kube-cell1.owlhead.ai --url 127.0.0.1:16443 &`
then `KUBECONFIG=~/.mandate-cell1/kubeconfig kubectl get pods -A`.

## Taking it down

Nothing here runs it for you; read each line before you run it.

```bash
doctl compute droplet delete cell1-a cell1-b
doctl compute volume delete cell1-a-data cell1-b-data
doctl compute firewall list    # then: doctl compute firewall delete <id>
doctl vpcs list                # then: doctl vpcs delete <id>
```

The tunnels `cell1-ops` and `cell1-app`, their DNS records and the Access applications are deleted
in the Cloudflare dashboard (*Zero Trust → Networks → Tunnels*, *Access → Applications*). Revoke
the GHCR and R2 tokens there too, and `rm -r ~/.mandate-cell1 ~/.mandate-cell1-secrets`.
