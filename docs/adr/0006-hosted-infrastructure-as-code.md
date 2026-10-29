# ADR-0006: Hosted infrastructure as code: OpenTofu, DigitalOcean Kubernetes and GitOps

| | |
|---|---|
| **Status** | Accepted ([DEC-2011](../project/decisions/infrastructure.md#dec-2011), the founder's; [DEC-2012](../project/decisions/infrastructure.md#dec-2012), lane IAC's engineering choices) |
| **Date** | 2026-10-18 |
| **Deciders** | The founder (platform, environments, cap); an agent (Claude Code, lane IAC) under DEC-79 for the layout inside it |

Every ADR has a decision behind it. This one's are
[DEC-2011](../project/decisions/infrastructure.md#dec-2011) and [DEC-2012](../project/decisions/infrastructure.md#dec-2012).

## Context

Cell #1 was to be k3s on two droplets, brought up by `deploy/hosted/cell1.py` from the founder's
machine (DEC-995, and DEC-2001 open in #1539). That meant cloud-init, k3s units, a join token
piped over SSH, and an ops tunnel whose token had to sit in a droplet's user data.

On 2026-10-18 the founder chose instead (DEC-2011):
- OpenTofu, DigitalOcean managed Kubernetes (DOKS) and GitOps;
- a staging and a production environment;
- a $150 monthly cap, with an alert at 80%;
- staging up for the 2026-10-26 demo;
- cloud tokens held only in GitHub environments the founder approves.

Constraints carried over:
- no inbound port from the internet;
- ingress only through a Cloudflare Tunnel, with Access in front of the API's hostname (DEC-995
  item 5, DEC-967 item 6);
- the journal's Postgres is CloudNativePG in the cell (DEC-967 item 8);
- its WAL archive and base backups go to Cloudflare R2 (DEC-973 item 6);
- no pull-request job may hold a credential (ES-12, [DEC-1005](../project/decisions/DEC-1005.md));
- agents never run `apply` against a real account and never see a token (`AGENTS.md` rules 7 and
  8).

## Decision

1. **Two layers, two lanes.**
   - OpenTofu under `deploy/tofu/` (lane IAC, E21-56) makes the cloud resources, and stops at a
     reachable, empty cluster with its buckets, DNS and Access.
   - GitOps (lane GO, E21-58, Flux) installs the cluster's add-ons and the applications from the
     repository, the workspace chart among them.
   - Flux pulls, so after the founder's one-time bootstrap neither CI nor `apply` needs the
     Kubernetes API (DEC-2012 item 11).
2. **Modules per concern, a root per environment** (DEC-2012 items 3 and 4):

   ```text
   deploy/tofu/
     modules/network      VPC, cloud firewall on the node tag
     modules/doks         cluster, one pool of two nodes, version pin, auto-upgrade off
     modules/spaces       OpenTofu's state bucket (adopted), nothing else
     modules/cloudflare   tunnel, DNS record, Access, journal-archive R2 bucket and its lock rule
     modules/budget       monthly estimate and its $65 per-environment refusal
     modules/cell         the DigitalOcean modules wired together
     environments/staging     root: backend, providers, lock file, values, tests
     environments/production  root: the same, with production's values
   ```

3. **State** lives in a Spaces bucket per environment. It is encrypted on the client by OpenTofu
   (PBKDF2-SHA-512, 600,000 iterations, AES-GCM), from a passphrase held only in the environment
   secret `TOFU_STATE_PASSPHRASE`. The bucket holds state alone: no journal spec §6.2 object and no
   backup (DEC-2012 item 5).
4. **The journal archive** is an R2 bucket per environment. One enabled lock rule covers every
   key for at least 30 days, the chart's archive retention. Barman's R2 token is made by the
   founder, scoped to that bucket, and never enters state (DEC-2012 item 9).
5. **Who runs what.**
   - A pull request runs only checks that need no credential: format, validate, tflint, and
     `tofu test` against mocked providers as the policy check (DEC-2012 item 13).
   - Only `.github/workflows/infra.yml` runs `plan` and `apply`: from `main`, on manual dispatch,
     under the GitHub environment `infra-staging` or `infra-production`, each with the founder as
     its required reviewer (DEC-2014).
   - The founder creates the tokens, the environments and their secrets (`deploy/tofu/README.md`,
     the runbook).
6. **Secrets into the cluster.**
   - The tunnel token is a sensitive output and is never printed.
   - The tunnel token and Barman's R2 token reach the cluster as Secrets the founder creates at
     the Flux bootstrap, or as SOPS files whose age key only the cluster holds. They are never
     plaintext in git or logs. That route is lane GO's (DEC-2012 item 10).
7. **Spend.**
   - Staging is about $57 a month: two `s-2vcpu-4gb` nodes, Spaces and 40 GB of block storage.
   - Production is about $55: the same, with 20 GB.
   - Both are at list price. Spaces is counted in each environment, though it is per account.
   - The plan fails past $65 per environment, which leaves room under the $150 cap for bandwidth,
     snapshots and egress.
   - The founder sets the 80% billing alert by hand, since no API sets it (DEC-2012 item 12).

```text
  GitHub (main, manual dispatch) ──founder approves──▶ infra.yml ── tofu apply ──▶ DigitalOcean
        │                                                         │                 VPC, firewall,
        │ pull requests: fmt, validate, tflint, tofu test (mocks)  │                 DOKS (2 nodes),
        │ no credential                                            │                 Spaces (state)
        ▼                                                         └───────────────▶ Cloudflare
   lane GO: Flux pulls add-ons and apps into the empty cluster                      Tunnel, DNS, Access,
   users ──▶ Cloudflare edge ──Access──▶ Tunnel ──▶ cloudflared pod ──▶ API          R2 archive + lock
   Barman (in the cell) ──R2 token, bucket-scoped──▶ R2 archive
```

## Consequences

- Node bring-up, k3s pinning, the join token and the ops tunnel's token in user data are gone.
  DigitalOcean runs the control plane, and its version moves only when a pull request changes the
  pin and the founder applies it.
- DOKS's own worker firewall is managed by DigitalOcean and can open what ours cannot close, since
  firewalls add up. A Service of type `LoadBalancer` or `NodePort` would be reachable, so lane GO
  creates none, as the chart already does not. The nodes keep public addresses for egress.
- The control plane's endpoint is public behind its own firewall, which admits only the CIDRs the
  founder sets.
- R2's bucket lock is governance-equivalent: a token that can edit the bucket's configuration can
  lift it (DEC-973 item 5). It protects the WAL archive against accidents and against Barman's
  own credential, not against that token. §6.2's objects never go there.
- The tunnel token lives in OpenTofu's state, encrypted. Whoever holds the state passphrase and
  the Spaces key can read it, and that is the founder, through the environment.
- Two DOKS nodes in one region are one failure domain (DEC-995 item 6 still holds).
- The state bucket and the Spaces key are created by hand once per environment. Everything else is
  in code.

## Alternatives rejected

| Alternative | Why |
|---|---|
| `cell1.py` on droplets (DEC-2001) | Replaced by the founder (DEC-2011) |
| Terraform | BUSL-licensed since 1.6; DEC-2011 names OpenTofu |
| Pulumi or CDK for Terraform | A general-purpose language and its runtime for a few dozen resources; DEC-2011 names OpenTofu |
| One root with workspaces | One backend and one variable set for both environments; a wrong workspace plans production |
| The journal archive in Spaces | No object lock at all; DEC-973 item 6 puts it on R2 |
| OPA/conftest or checkov over plan JSON | A new tool, and a real plan needs credentials; `tofu test` with mocks needs neither |
| Plan on pull requests with read-only tokens | Any token on a pull-request job breaks ES-12 |
| Isolated workers behind a NAT gateway | The gateway's price would take much of the cap; a later hardening |
| HA control plane | Extra monthly cost; staging and a small production do not need it yet |
