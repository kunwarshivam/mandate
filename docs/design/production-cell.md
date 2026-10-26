# Customer production, cell 1: design and monthly cost (proposed, never applied)

| | |
|---|---|
| **Status** | Proposed: [DEC-2064](../project/decisions/DEC-2064.md), founder-reserved (spending, DEC-79). Nothing here is provisioned until the founder approves a monthly amount |
| **Date** | 2026-10-26 (UTC); every price below was read on this date |
| **Owner** | Lane IAC (E21-56) |
| **Code** | `deploy/tofu/environments/production-cell1/` and the modules `cluster`, `ingress`, `archive`, `monitoring`, `costing` |

The founder asked on 2026-10-26: "Yes, bring a costed plan". This is that plan: production
infrastructure for customer trading, separate from staging, with what it costs a month at a
demo-scale load and at a first-customers load.

## 1. What this is, and what it is not

- **It is** one managed cell (HLD §4, "Workspace deployment") in `sfo2`, the US West (DEC-2047),
  that hosts many retail workspaces with logical isolation, and dedicated pools for business
  workspaces (HLD §8, infrastructure §3.2).
- **It is paper first.** Customer workspaces trade on paper and venue demo environments until the
  Phase 2 gate: counsel, the M13 penetration test, and a signed `live` build (infrastructure §1.1).
  The design carries no live route, and adds none.
- **It never runs today.** `infra.yml` dispatches only `staging` and `production`, and
  `cargo xtask infra` refuses any other environment. The root also refuses every plan until its
  `approved_monthly_usd` is at least the estimate, and that is 0 until the founder approves a spend
  (`terraform_data.founder_approval`). No `plan` or `apply` was run against any account; the checks
  are `tofu validate`, tflint and `tofu test` against mocked providers.
- **It replaces** the small `environments/production` root (two nodes, $55 a month, DEC-2012
  item 12) once approved. Until then that root stays as it is, under DEC-2011's $150 cap.

## 2. Invariants

Each is enforced by a validation, a precondition or a test in the code, or disclosed in §8.

1. Nothing in the cell accepts a connection from the internet. The only ingress is the Cloudflare
   Tunnel, behind Access. No load balancer, `LoadBalancer` or `NodePort` Service exists.
2. A person admitted to one workspace's API host is not admitted to another's. Every workspace has
   its own host and its own Access policy naming only its own people, and the tunnel routes each
   host to that workspace's namespace alone. Every other host name gets a 404.
3. A workspace's processes run only in its own namespace, on nodes no other cell service runs on,
   and a business workspace's only on its own pool.
4. Every count is fixed, with no autoscaling, so the estimate is the bill.
5. No credential for a broker or exchange ever enters OpenTofu's state. Those live only in the vault.
   The state holds the tunnel token, the two archive writer tokens and the people's email
   addresses, all encrypted on the client.
6. A backup writer can neither lift its bucket's lock nor reach another bucket.
7. A plan past the approved amount fails.

## 3. Topology and sizing

```text
 people ──▶ Cloudflare edge ──Access (per workspace)──▶ Tunnel ──▶ cloudflared (system pool, 2 replicas)
                                                                       │ api-<id>.owlhead.ai → api.ws-<id>.svc
 ┌──────────────────────── DOKS mandate-prod-cell1, sfo2, HA control plane ───────────────────────┐
 │ system pool (3 nodes): Flux, policy-controller, CloudNativePG + journal (3 instances),          │
 │   OpenBao (3 Raft replicas), Prometheus/Alertmanager/Grafana, cloudflared                       │
 │ workspace pool (tainted, N/2 + 1 nodes): ws-<id> namespaces, one chart instance per workspace   │
 │ dedicated pools (tainted per workspace): business workspaces                                    │
 └────────────────────────────────────────────────────────────────────────────────────────────────┘
 Barman ──bucket-scoped token──▶ R2 mandate-journal-archive-prod-cell1 (lock 30 d)
 vault snapshot job ──bucket-scoped token──▶ R2 mandate-vault-snapshots-prod-cell1 (lock 30 d)
 DigitalOcean Monitoring and Uptime ──email──▶ operator   (OpenTofu state: Spaces, versioned)
```

### 3.1 One workspace's footprint

The requests are the workspace chart's own (`deploy/cell/charts/workspace/processes.json`):

| Process | CPU request | Memory request |
|---|---|---|
| api | 100m | 64Mi |
| deploy-manager | 50m | 64Mi |
| role-provisioner | 50m | 32Mi |
| connect-finish | 50m | 64Mi |
| runtime (one per agent, DEC-08) | 100m | 128Mi |
| executor (guaranteed) | 250m | 256Mi |
| token-exchange | 50m | 32Mi |
| scheduler (guaranteed) | 100m | 128Mi |
| model-gateway | 100m | 128Mi |
| exporter | 50m | 64Mi |
| data | 100m | 128Mi |
| dispatcher (planned, X8; sized like the runtime) | 100m | 128Mi |
| sidecars and init containers, about | 150m | 160Mi |
| **One workspace, two agents** | **about 1.35 vCPU** | **about 1.5 GiB** |

A workspace's data service, executors and runtimes sit on one node (infrastructure §3.2), so the
unit of placement is a whole workspace.

### 3.2 Pools

| Pool | Demo scale | First customers | Why |
|---|---|---|---|
| System | 3 × `s-2vcpu-4gb` | 3 × `s-4vcpu-8gb` | Three nodes, so the vault's Raft and the journal's three instances each keep a quorum through one node's loss |
| Workspaces | 2 × `s-4vcpu-8gb` | 6 × `s-4vcpu-8gb` | Two workspaces a node (about 2.7 vCPU of the node's roughly 3.5 allocatable), `ceil(n / 2) + 1`: one node spare, so a lost node's workspaces restart elsewhere |
| Dedicated | none | none | A business workspace adds `dedicated_pools`; each is priced by the same size list |

The system pool's demo load, by request: Flux about 0.4 vCPU and 0.6 GiB, policy-controller 0.1
and 0.2, the CloudNativePG operator 0.1 and 0.25, cloudflared 0.2 and 0.25, monitoring 0.8 and
1.5, the vault 0.75 and 0.75, the journal 3 × 0.5 vCPU and 1 GiB. That is about 3.85 vCPU and
6.6 GiB of roughly 5.7 and 8.4 allocatable. At first customers the journal is 3 × 1 vCPU and 2 GiB
and monitoring 1 vCPU and 3 GiB: about 5.6 vCPU and 11 GiB of roughly 10.5 and 18. The allocatable
figures are estimates: DOKS reserves part of every node for itself.

### 3.3 Storage (DigitalOcean block storage)

| Claim | Demo | First customers |
|---|---|---|
| Journal, 3 instances | 3 × 20 GB | 3 × 50 GB |
| Vault Raft, 3 replicas | 3 × 5 GB | 3 × 5 GB |
| Prometheus | 20 GB | 50 GB |
| Artifacts, 5 GB a workspace | 10 GB | 50 GB |
| **Total** | **105 GB** | **265 GB** |

## 4. Tenancy

Following HLD §8 and infrastructure §3.2 and §4.1:

- **Namespace per workspace** (`ws-<id>`, an opaque id, never a name), each with its own instance
  of the workspace chart, its default-deny NetworkPolicies, and its own Cilium FQDN egress
  allow-list per process (DEC-2058). The policy-controller's signature check includes every
  workspace namespace (DEC-2016 item 1).
- **Scheduling:** workspace pods tolerate `mandate.dev/pool=workspaces:NoSchedule`, and nothing
  else does, so no cell service shares a workspace node. A business workspace's pods tolerate only
  `mandate.dev/workspace=<id>:NoSchedule`, on its own pool. Agent processes are never shared
  across workspaces (HLD §8).
- **Journal:** one CloudNativePG cluster per cell, three instances with a synchronous quorum of one
  (journal §5.3), so losing one standby never stalls appends. One database per workspace, each with
  its own journal §6.1 roles; a business workspace may get its own cluster.
- **Vault:** one OpenBao namespace per workspace, with its per-process policies (identity spec,
  infrastructure §5.2).
- **Ingress:** one host and one Access application per workspace (§5).
- **A dedicated cell** for a large customer is another root, `production-cell<n>`, from the same
  modules.

## 5. Ingress and Access

- One remotely managed tunnel for the cell. cloudflared runs as its own two-replica Deployment in
  the system pool, rather than as a sidecar in one API pod, since the tunnel now serves every
  workspace (lane GO's change, §9).
- Each workspace's host is `api-<id>.owlhead.ai`, one label deep, so the zone's free Universal SSL
  certificate covers it. A host two labels deep (`<id>.api.owlhead.ai`) would need a paid
  certificate.
- Each host has its own Access application and policy, naming only that workspace's people, from
  the GitHub environment secret `ACCESS_ALLOWED_EMAILS`, as a map by workspace id. A workspace with
  no named person fails the plan: there is no account-members fallback. As in staging and
  production, only an unauthenticated CORS preflight is forwarded (DEC-2381).
- The web app must learn each workspace's API host (lane W, §9).

## 6. Monthly cost

Prices are list prices for `sfo2`, read on **2026-10-26**:

| # | Price used | Source |
|---|---|---|
| P1 | DOKS control plane free; HA control plane $40 a month; Basic nodes' outbound transfer 2,000 GiB a node, pooled | [digitalocean.com/pricing/kubernetes](https://www.digitalocean.com/pricing/kubernetes) |
| P2 | `s-2vcpu-4gb` $24, `s-4vcpu-8gb` $48 a month (DOKS nodes are priced as Droplets; the plan reads the live size list itself) | [digitalocean.com/pricing/droplets](https://www.digitalocean.com/pricing/droplets) |
| P3 | Block storage $0.10 per GiB a month | [digitalocean.com/pricing/volumes](https://www.digitalocean.com/pricing/volumes) |
| P4 | Spaces $5 a month, with 250 GiB and 1 TiB of transfer | [digitalocean.com/pricing/spaces-object-storage](https://www.digitalocean.com/pricing/spaces-object-storage) |
| P5 | R2 Standard $0.015 per GB-month, Class A $4.50 per million, egress free; free tier 10 GB-month and 1 million Class A | [developers.cloudflare.com/r2/pricing](https://developers.cloudflare.com/r2/pricing/) |
| P6 | Cloudflare Zero Trust free up to 50 users; pay-as-you-go $7 per user a month | [cloudflare.com/plans/sase-zero-trust](https://www.cloudflare.com/plans/sase-zero-trust.md) |
| P7 | Uptime: one check free, then $1 per check a month | [digitalocean.com/pricing/uptime-monitoring](https://www.digitalocean.com/pricing/uptime-monitoring) |
| P8 | DigitalOcean Monitoring and its alert policies: no cost | [docs.digitalocean.com/products/monitoring/details/pricing](https://docs.digitalocean.com/products/monitoring/details/pricing/) |

The two loads:

- **Demo scale:** 2 workspaces (the founder's and one design partner), about 2 agents each, 4
  people through Access, paper only.
- **First customers:** 10 workspaces, about 2 agents each, about 21 people through Access, paper
  only.

| Component | Demo scale | First customers | Price |
|---|---|---|---|
| DOKS HA control plane | $40.00 | $40.00 | P1 |
| System pool | $72.00 (3 × $24) | $144.00 (3 × $48) | P2 |
| Workspace pool | $96.00 (2 × $48) | $288.00 (6 × $48) | P2 |
| Block storage | $10.50 (105 GB) | $26.50 (265 GB) | P3 |
| Spaces (OpenTofu state; per account) | $5.00 | $5.00 | P4 |
| R2: journal archive and vault snapshots | $0.47 (25 GB, 20,000 writes) | $2.85 (160 GB, 100,000 writes) | P5 |
| Cloudflare Access | $0.00 (4 people) | $0.00 (21 people) | P6 |
| Uptime checks, one per API host | $1.00 (2 checks) | $9.00 (10 checks) | P7 |
| Node and uptime alerts | $0.00 | $0.00 | P8 |
| Tunnel, DNS, Universal SSL | $0.00 | $0.00 | Cloudflare's free plan, as staging |
| Outbound transfer | $0.00 (well under 10,000 GiB pooled over 5 nodes) | $0.00 (well under 18,000 GiB pooled over 9 nodes) | P1 |
| **Total, this cell** | **$224.97** | **$515.35** | |
| Suggested approval, with about 10% headroom | $250 | $570 | |

`tofu test` asserts both totals (`environments/production-cell1/tests/plan.tftest.hcl`), computed
by the `costing` module from the same inputs. No free tier is credited, since staging may use it.

**Beside this cell, not in its total:**

- **Staging** stays at about $57 a month (DEC-2012 item 12). With this cell the account comes to
  about $282 (demo) or $572 (first customers), against DEC-2011's $150 cap, which the founder's
  approval would raise. The small `production` root's $55 goes away when this replaces it.
- **Model spend** is capped at $50 a month per workspace (DEC-997): at most $100 (demo) or $500
  (first customers). It is OpenRouter's bill, not the cloud's.
- **Each further workspace** costs about $27 a month: half a workspace node ($24), 5 GB of
  artifacts ($0.50), and an uptime check ($1), plus its model spend.
- **The Access cliff:** at the 51st person every seat is paid, $357 a month at 51 people. Before
  then, the alternative is Access only for operators, with customers signing in with passkeys
  alone (DEC-2103). That changes the founder's decision that Access stays in front of the API
  (handover of 2026-10-26), so it is the founder's call.
- **Not included and needed before live money:** the journal §6.2 cold store with its compliance
  lock and its second-region replica, a second region for infrastructure §6.1's region-loss row,
  and the M13 penetration test. Each is priced when the founder schedules it.

**Cheaper, if wanted:** drop the HA control plane (−$40; the kill switch's Job then depends on a
single control plane), or run the demo's workspaces on one node with no spare (−$48; a node loss
stops both workspaces until it returns).

## 7. Backups, the vault, and monitoring

- **Journal:** CloudNativePG's Barman archives WAL continuously and takes base backups to
  `mandate-journal-archive-prod-cell1`, R2 in `wnam`, locked 30 days, with point-in-time recovery
  (DEC-973 item 6, DEC-2605 row 4). Its token is Object Read and Write on that bucket alone.
- **Vault:** OpenBao, three Raft replicas, with an hourly snapshot job to
  `mandate-vault-snapshots-prod-cell1`, locked 30 days, under its own bucket-scoped token. A
  snapshot is encrypted by the vault's barrier, so it is useless without the unseal material.
  Production's unseal is not decided (DEC-2021 item 6); DEC-2064 item 8 asks the founder.
- **Restore drills** run on staging monthly (infrastructure §6.4), and on this cell before its
  first customer.
- **Monitoring inside the cell** is lane GO's Prometheus, Alertmanager and Grafana on the system
  pool, with opaque labels only (infrastructure §8.1), alerting on infrastructure §8.2's
  safety-rule alerts.
- **Monitoring outside the cell**, so a dead cell still pages: DigitalOcean's CPU, memory and disk
  alerts on the cell's node tag, and an uptime check with a down alert on every API host
  (`modules/monitoring`). They carry only a node, a host and a threshold (OPS-10), and go to the
  addresses in the secret `ALERT_EMAILS`.

## 8. Security

**Least privilege.**

- The DigitalOcean token is production's (`deploy/tofu/README.md` §1.1), plus Monitoring and
  Uptime create, read, update and delete, as DigitalOcean names them. It has no Droplet,
  database, load-balancer, domain, billing or account scope.
- The Cloudflare token is production's (§2.3). Account API Tokens Edit, which mints the writer
  tokens, is admin-level (DEC-2029, DEC-2605 row 12); it stays in the environment alone.
- Each backup writer holds Object Read and Write on its own bucket, and nothing else.
- The control plane admits only the founder's CIDRs: never empty, none wider than /24. This root
  has no runner window (DEC-2028 is staging's), so its bootstrap runs from one of the founder's
  addresses (`deploy/gitops/BOOTSTRAP.md`).

**No secret in state but these.** The tunnel token, the two writer tokens, and the Access and alert
email addresses, all `sensitive`, in state encrypted by PBKDF2-SHA-512 and AES-GCM under this
cell's own passphrase (DEC-2012 item 5). No broker or exchange credential ever enters state. Whoever
holds the passphrase and a Spaces key can read the state, as for production.

**Egress.**

- The node firewall admits inbound traffic only from the cell's VPC (`10.130.0.0/20`).
- Outbound, it allows only TCP 443, DNS on 53, NTP on UDP 123, and the tunnel's 7844. Plain-web
  port 80, which staging keeps, is dropped (`modules/network`'s `egress`).
- Inside the cluster, every workspace process reaches only its own hosts by exact name on 443
  (DEC-2058). The journal's archive client reaches only R2's addresses (DEC-2017).

**Residuals, disclosed.**

- DOKS's own managed worker firewall adds to ours, and firewalls add up, so the node firewall
  cannot close what DOKS opens. The guarantee is that nothing creates a public port.
- The nodes keep public addresses for egress. DOKS's `isolated_workers` with a NAT gateway would
  remove them; that is a later hardening, priced when chosen.
- R2's lock is governance-equivalent: the Cloudflare infra token can lift it (DEC-973 item 5).
- One region is one failure domain (DEC-995 item 6). The synchronous standby protects against
  node loss, not region loss.
- An infra run that dies outright can leave its Spaces key until the next run deletes it
  (DEC-2030 item 4).

## 9. Release and promotion from staging

Unchanged in principle (DEC-2015, DEC-2016, DEC-2048):

1. `release.yml` builds from `main`, then signs each image index and chart keyless as itself on
   `refs/heads/main`.
2. Staging follows `main`: `cargo xtask gitops promote staging` pins the new digests, and staging
   runs its drills.
3. Production moves only by `cargo xtask gitops promote production`, which copies the digests
   staging ran on `main`, in a pull request the founder approves. `test_gitops.py` refuses a
   digest staging never ran.
4. Flux in the cell pulls those digests, and the policy-controller admits only signed images in
   every workspace namespace. The kill switch's Job keeps its exemption (DEC-2016 item 2).
5. Infrastructure changes go through `infra.yml`, from `main`, under the GitHub environment
   `infra-production-cell1`, with the founder as its required reviewer (DEC-2014).

**Follow-ups once the founder approves.** Each is its own PR:

- **Lane IAC:** add `production-cell1` to `infra.yml` and `cargo xtask infra`, its preflight
  items, and the runbook rows (the `infra-production-cell1` environment, its passphrase,
  `ACCESS_ALLOWED_EMAILS` as a map, `ALERT_EMAILS`); set `approved_monthly_usd` to the approved
  amount; retire the small `production` root; move the `cloudflare` module's archive onto
  `modules/archive`.
- **Lane GO:**
  - a `clusters/production-cell1` overlay with one HelmRelease per workspace;
  - cloudflared as its own Deployment;
  - CloudNativePG at three instances, with a database per workspace;
  - OpenBao HA and its snapshot job;
  - the monitoring stack;
  - tolerations, and `promote production-cell1`.
- **Lane W:** each workspace's API host in the web app.
