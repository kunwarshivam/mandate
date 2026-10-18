# ADR-0006: Hosted infrastructure as code: OpenTofu, DigitalOcean Kubernetes and GitOps

| | |
|---|---|
| **Status** | Accepted ([DEC-2011](../project/decisions/DEC-2011.md), the founder's; [DEC-2012](../project/decisions/DEC-2012.md), lane IAC's engineering choices) |
| **Date** | 2026-10-18 |
| **Deciders** | The founder (platform, environments, cap); an agent (Claude Code, lane IAC) under DEC-79 for the layout inside it |

Every ADR has a decision behind it; this one's are [DEC-2011](../project/decisions/DEC-2011.md)
and [DEC-2012](../project/decisions/DEC-2012.md).

## Context

Cell #1 was to be k3s on two droplets brought up by `deploy/hosted/cell1.py` from the founder's
machine (DEC-995, DEC-2001, open in #1539): cloud-init, k3s units, a join token piped over SSH, and
an ops tunnel whose token had to sit in a droplet's user data. On 2026-10-18 the founder chose
instead OpenTofu, DigitalOcean managed Kubernetes (DOKS) and GitOps, a staging and a production
environment, a $150 monthly cap with an alert at 80%, staging up for the 2026-10-26 demo, and
cloud tokens held only in GitHub environments the founder approves (DEC-2011).

Constraints carried over: no inbound port from the internet; ingress only through a Cloudflare
Tunnel; Access in front of the API's hostname (DEC-995 item 5, DEC-967 item 6); the journal's
Postgres is CloudNativePG in the cell (DEC-967 item 8); and no pull-request job may hold a
credential (ES-12, [DEC-1005](../project/decisions/DEC-1005.md)). Agents never run `apply`
against a real account and never see a token (`AGENTS.md` rules 7 and 8).

## Decision

1. **Two layers, two lanes.** OpenTofu under `deploy/tofu/` (lane IAC, E21-56) makes the cloud
   resources, and stops at a reachable, empty cluster with its buckets, DNS and Access. GitOps
   (lane GO) installs the cluster's add-ons and the applications, the workspace chart among them,
   from the repository. The hand-off is OpenTofu's outputs: the cluster's id and endpoint, the
   tunnel's id and token, and the archive bucket with its key.
2. **Modules per concern, a root per environment** (DEC-2012 items 1 and 2):

   ```text
   deploy/tofu/
     modules/network      VPC, cloud firewall on the node tag
     modules/doks         cluster, one pool of two nodes, version pin, auto-upgrade off
     modules/spaces       state bucket (adopted), journal-archive bucket, archive key
     modules/cloudflare   tunnel, DNS record, Access application and policy
     modules/budget       monthly estimate and cap
     environments/staging     root: backend, providers, lock file, values, tests
     environments/production  root: the same, with production's values
   ```

3. **State** in a Spaces bucket per environment, encrypted on the client by OpenTofu with a
   passphrase from the environment's secrets (DEC-2012 item 3).
4. **Who runs what.** A pull request runs only checks that need no credential: format, validate,
   tflint and `tofu test` against mocked providers as the policy check (DEC-2012 item 8). Only
   `.github/workflows/infra.yml` runs `plan` and `apply`, from `main`, on manual dispatch, under
   the GitHub environment `infra-staging` or `infra-production`, each with the founder as its
   required reviewer. The founder creates the tokens, the environments and their secrets
   (`deploy/tofu/README.md`, the runbook).
5. **Spend.** About $57 a month for staging (two `s-2vcpu-4gb` nodes, Spaces, block storage) and
   about $52 for production, at list price, against the cap's $75 share each; the plan fails past
   it (DEC-2012 item 7). The 80% billing alert is set by the founder in DigitalOcean's billing
   settings, since no API sets it.

```text
  GitHub (main, manual dispatch) ──founder approves──▶ infra.yml ── tofu apply ──▶ DigitalOcean
        │                                                         │                 VPC, firewall
        │ pull requests: fmt, validate, tflint, tofu test (mocks)  │                 DOKS (2 nodes)
        │ no credential                                            │                 Spaces buckets
        ▼                                                         └───────────────▶ Cloudflare
   lane GO: GitOps installs add-ons and apps into the empty cluster                 Tunnel, DNS, Access
   users ──▶ Cloudflare edge ──Access──▶ Tunnel ──▶ cloudflared pod ──▶ API (no public port)
```

## Consequences

- Node bring-up, k3s pinning, the join token and the ops tunnel's token in user data are gone;
  DigitalOcean runs the control plane, and its version moves only when a pull request changes the
  pin and the founder applies it.
- DOKS's own worker firewall is managed by DigitalOcean and can open what ours cannot close
  (firewalls add up). A Service of type `LoadBalancer` or `NodePort` would be reachable, so lane
  GO creates none, as the chart already does not; the nodes keep public addresses for egress.
- The control plane's endpoint is public behind its own firewall, which admits only the CIDRs the
  founder sets. Reaching it through the tunnel and Access is lane GO's choice to add.
- Spaces has no object lock, so the journal archive is versioned rather than write-once
  (DEC-2012 item 9); the archive key can delete versions. A store with object lock is a
  follow-up.
- The tunnel token and the archive key live in OpenTofu's state, encrypted; whoever holds the
  state passphrase and the Spaces key can read them, which is the founder through the environment.
- Two DOKS nodes in one region are one failure domain (DEC-995 item 6 still holds).
- The state bucket is created by hand once per environment; everything else is in code.

## Alternatives rejected

| Alternative | Why |
|---|---|
| `cell1.py` on droplets (DEC-2001) | Replaced by the founder (DEC-2011) |
| Terraform | BUSL-licensed since 1.6; DEC-2011 names OpenTofu |
| Pulumi or CDK for Terraform | A general-purpose language and its runtime for a few dozen resources; DEC-2011 names OpenTofu |
| One root with workspaces | One backend and one variable set for both environments; a wrong workspace plans production |
| OPA/conftest or checkov over plan JSON | A new tool, and a real plan needs credentials; `tofu test` with mocks needs neither |
| Plan on pull requests with read-only tokens | Any token on a pull-request job breaks ES-12 |
| Isolated workers behind a NAT gateway | The gateway's price would take much of the cap; a later hardening |
| HA control plane | Extra monthly cost; staging and a small production do not need it yet |
