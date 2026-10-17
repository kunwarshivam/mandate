# deploy/hosted/: cell #1 on two DigitalOcean droplets

Everything that stands up cell #1 from a clean account in one scripted run (E21-51,
[DEC-2001](../../docs/project/decisions/DEC-2001.md)): k3s on two droplets in `nyc3`
([DEC-995](../../docs/project/decisions/DEC-995.md)), the workspace chart of `../cell/` with cell
#1's values, ingress only through Cloudflare Tunnel, Cloudflare Access in front of every public
hostname, and the journal's cold store bound for R2. Paper and demo data only; no live connection.

The founder runs it with their own tokens, by [RUNBOOK.md](RUNBOOK.md). Agents never create a
cloud resource, spend money, or see a token.

| Path | What it is |
|---|---|
| `cell1.py` | The bring-up, in five idempotent steps: `cloudflare`, `droplets`, `nodes`, `cluster`, `check` (or `all`). Prints every command and API request before it runs it; `--dry-run` prints them all and calls nothing. `--target k3d` runs `nodes` and `cluster` against a local two-node k3d cluster |
| `cell1.json` | The settings: region, size, names, hostnames, the pinned k3s, cloudflared and CloudNativePG chart, each with its SHA-256. No secret |
| `firewall.json` | The cloud firewall, by tag: inbound only from the cell's own droplets, outbound by port |
| `node/` | What cloud-init puts on each node: the k3s unit (never started by cloud-init), the ops tunnel's unit (cell1-a), the metadata block, sshd's drop-in, and the scripts `nodes` calls over SSH |
| `values/cell1.yaml` | The workspace chart's values on top of `../cell/values/k3s.yaml`: `deploymentTier: dev` |
| `values/cell1-journal-db.yaml` | The journal's Postgres: one instance, 30 GiB, until lane S2 adds the standby |
| `../tests/test_hosted.py` | The invariants below, each shown failing on a seeded bug, and two recorded runs |

## What holds

Each is a test in `deploy/tests/test_hosted.py` (or `test_cell.py` for the chart):

- **No inbound port from the internet**, SSH included: the firewall's only inbound source is the
  cell's tag, and it exists before the first droplet. Outbound is by named port only.
- **The founder arrives through the ops tunnel `cell1-ops`** on cell1-a, behind Access:
  `ssh-cell1.<zone>` for SSH (keys only, `cell1_admin` only, never root) and `kube-cell1.<zone>`
  for kubectl, through `cloudflared access`. cell1-b is reached by `ProxyJump` over the VPC.
- **Every download is pinned and checked** before it is installed, one command a line under
  `set -eu`, so a mismatch fails cloud-init.
- **cloud-init never starts k3s.** `nodes` starts the server, makes the join token on it, pipes it
  to cell1-b over SSH, and starts the agent.
- **The ops tunnel's first token is the only secret in user data,** on cell1-a only, as a
  root-only 0600 file loaded by its unit as a systemd credential. No pod can read the metadata
  service that serves it (DEC-2001 item 4 has the residuals).
- **Secrets travel on stdin only.** The GHCR pull secret, the app tunnel's token and the R2 key
  pair reach their Secrets on kubectl's stdin; the join token on SSH's. A recorded run plants each
  value and finds it in no argv and no output; a command line carrying one is refused.
- **`MANDATE_DEPLOYMENT_TIER` comes from the chart's `deploymentTier`**: `production` unless
  values say otherwise, `ci` in the local cell, `dev` on cell #1 (DEC-2001 item 6, Proposed).
- **The pins match the local cell's:** k3s and the CloudNativePG chart and operator are the
  versions `xtask/src/cell.rs` runs the drills on.

## The local proof

The node stage needs droplets; the `cluster` step does not. CI's `hosted-k3d` job runs

```bash
cargo xtask cell hosted     # python3 deploy/hosted/cell1.py --target k3d all
```

on every change that is not documentation only: a k3d cluster of a server and an agent,
then the same namespace, secrets, service-account patch, CloudNativePG operator, journal-db and
workspace chart as cell #1, with the tunnel off. With `--local-image mandate.local/mandate:<tag>`
(the image `cargo xtask cell up` built) the CLI's image is imported too; without it the chart
renders only the migration Job. `python3 deploy/hosted/cell1.py --dry-run all ...` prints cell
#1's whole plan.

## What is not here yet

- The API's route: the app tunnel sends `api.<zone>` to `http://api.ws-ws1.svc.cluster.local:8080`,
  which answers once lane A1 (E21-31) adds the API server; until then Access answers and the
  origin returns an error behind it.
- A server-side caller of the API (the `web/` Worker) needs an Access service token (lane W2).
- R2: the Secret and the ConfigMap exist; no process ships segments until E5-7's shipper or
  E21-41's backups, which also set the `object-storage` egress CIDRs.
- Rotating the ops tunnel's first token (DEC-2001 item 4).
