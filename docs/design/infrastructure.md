# Infrastructure, Deployment, and Operations (design v0.2, draft)

| | |
|---|---|
| **Status** | Draft v0.2: the fixes from the post-merge review of v0.1 (#552). The engineering readings are Accepted (agent) in [DEC-434](../project/decisions/DEC-434.md), items 1 to 12 and 22 to 24. The founder accepted items 13 to 20 on 2026-10-03. Item 21, what an exit does while the journal is unavailable (§2.1), is Proposed for the founder. OPS-8, §6.3, §6.4, and §8.2 take [DEC-789](../project/decisions/DEC-789.md) item 9 (the founder, 2026-10-09): a token-only `incomplete` verification passes a restore drill, and only `result: fail` raises the SEV-1 alert |
| **Date** | 2026-10-03 |
| **Owner** | Engineering (founder) |
| **Builds on** | [HLD](../HLD.md) §4, §5, §6 D, §7, §8, §11; [ADR-0001](../adr/0001-engineering-setup.md) ES-08, ES-09, ES-14, ES-17 to ES-20, ES-23; [journal spec](../specs/journal.md) §5, §6, §10, §11; [trading domain spec](../specs/trading-domain.md) §5.4, §5.5, §5.7, §11; [quality and release plan](../project/07-quality-and-release.md) |
| **Siblings** | The [agent harness spec](../specs/agent-harness.md) (the process shell around the runtime), the [inference spec](../specs/inference.md) (the model gateway and providers; its invariants are `INF-n`), and the [data-plane spec](../specs/data-plane.md) (market data ingestion and storage; `DP-n`). This document covers how those components are hosted and operated, not what they compute. The [threat model](../security/threat-model.md) cites this document's `OPS-n` invariants |

This document says how Mandate runs: which processes exist, where they run, where data is
stored, how secrets are held, how the system is deployed, upgraded, backed up, restored, watched,
and paid for, and what each component does when something fails.

The repository is public, so this document stays at the architecture level. It names no hosts,
accounts, addresses, or vendors' account details. Those live in operator configuration outside
the repository.

## Contents

1. [Scope and environments](#1-scope-and-environments)
2. [Invariants](#2-invariants)
3. [Runtime topology](#3-runtime-topology)
4. [Data stores](#4-data-stores)
5. [Secrets and the vault](#5-secrets-and-the-vault)
6. [Backups and disaster recovery](#6-backups-and-disaster-recovery)
7. [Deploy and upgrade](#7-deploy-and-upgrade)
8. [Observability and on-call](#8-observability-and-on-call)
9. [Security baseline](#9-security-baseline)
10. [Cost model](#10-cost-model)
11. [Failure walk](#11-failure-walk)
12. [What exists and what is planned](#12-what-exists-and-what-is-planned)
13. [Decisions](#13-decisions)
14. [Backlog](#14-backlog)
15. [Open questions](#15-open-questions)

---

## 1. Scope and environments

### 1.1 Environments

There are five environments. Only one of them may ever touch live money, and it does not exist
yet.

| Environment | Purpose | Broker access | Data | Who runs it |
|---|---|---|---|---|
| **Dev** | An agent's or the founder's worktree | Recorded fixtures; Alpaca paper only when a story needs it, with paper keys from local configuration (ES-19) | Local Postgres, local files | Each developer |
| **CI** | Per-PR and weekly checks (ES-12) | None. Recorded fixtures only; no secrets in PR jobs | Ephemeral Postgres service container | GitHub Actions |
| **Paper** (Phase 1) | The Phase 1 soak: one workspace, the founder's own Alpaca paper account (M6, M7) | Alpaca paper only | One Postgres, filesystem artifact store, local Parquet | The founder, on one host the founder controls |
| **Staging** (from M8) | A managed cell in miniature: upgrade drills, failover drills, restore drills, fault injection, release candidates | Paper and venue demo environments only | Synthetic workspaces; never customer data | Platform operators |
| **Production** (Proposed, after the Phase 2 gate) | Managed cells for customers; the hybrid and on-prem installers ship from the same build | Paper for paper agents; live only after counsel sign-off, the M13 penetration test, and a signed `live` build (ES-23) | Customer data | Platform operators (managed); the customer (hybrid, on-prem) |

Rules that hold across the ladder:

- Every environment except production runs a build in which only the paper trading and data hosts
  are compiled in, and the `live` cargo feature is forbidden (ES-23). The one exception, for paper
  Alpaca OAuth only, is Alpaca's token endpoint in the token-exchange client (§2, OPS-5; [DEC-821](../project/decisions/DEC-821.md)). Configuration's
  `environment` accepts only `paper` or `backtest` there (ES-19).
- A stream's `environment` is fixed in its `StreamOpened`, and appends with another environment
  are rejected (ES-23), so a paper journal can never become a live one.
- Until the founder decides to go live (reserved by DEC-79), no production environment exists and
  the most conservative option holds: paper only.

### 1.2 What runs where, by deployment mode

The [HLD's deployment modes](../HLD.md#deployment-modes) decide ownership. This table adds who
operates each piece.

| Component | Managed cell | Hybrid | On-prem / air-gapped |
|---|---|---|---|
| Global control plane (directory, licensing, fleet health, relay, catalog) | We run it | We run it | The customer runs the same software |
| Workspace control services | We run them, per cell | Customer's cluster or host | Customer's |
| Data plane: runtimes, executors, scheduler, model gateway | We run them, per cell | Customer's | Customer's |
| Journal Postgres | Ours, per cell, with a synchronous standby | Customer's (PostgreSQL 17 floor, ES-08) | Customer's |
| Cold store and artifacts (object storage, object lock) | Ours, two regions | Customer's; installer checks lock mode at startup (journal §6.2) | Customer's |
| Vault | Ours, backed by a key management service | Customer's | Customer's |
| Backups and restore drills | Us | Customer, with our runbook | Customer, with our runbook |
| On-call for the data plane | Us | Customer; we support | Customer; we support |
| Updates | We roll them out | Signed bundles pulled over the outbound link | Signed bundles installed by hand |

In v1, the fully on-prem control plane and the shared data plane are out of scope
([backlog, Won't](../project/06-backlog-v1.md#wont-v1)); the on-prem column is the target the
packaging must not rule out.

Engineering rule from the HLD: every component ships from the same installer, and no hosted-only
dependency sits in a core path. A managed service we buy (a managed Postgres, a cloud key service)
is allowed only behind an interface the on-prem build can satisfy with self-run software.

---

## 2. Invariants

Each invariant names how it is checked. "Test" means an automated test in CI or the weekly run;
"drill" means a scheduled exercise in staging or paper whose result is journaled; "check" means a
startup or CI check that refuses to proceed.

| ID | Invariant | Source | How it is checked |
|---|---|---|---|
| **OPS-1** | Credentials exist in plaintext only inside the vault and in the memory of the one process that uses them. They never appear in logs, metrics, traces, the journal, artifacts, backups outside the vault's own encrypted snapshots, environment variables of live processes, or the repository | Rule 7; ES-09; ES-19; ES-23 | Log-scan test (ES-09); gitleaks per PR and full history weekly; a test that the live build reads credentials only from the vault client; a backup-scan drill that greps restored backups for known canary secrets |
| **OPS-2** | Journal before acting survives any crash. No order request leaves a process unless its intent and `OrderSubmitted` are durably committed (`Committed` or `AlreadyCommitted`), and durable means `synchronous_commit = on`, with a synchronous standby in a second failure domain in managed live deployments | Rule 5; journal §5.2, §5.3 | Fault injection killing the process, the database connection, and the database at every step of submission (07, Phase 1 gate); a configuration check at startup that refuses live trading when `synchronous_commit` or the standby requirement is not met |
| **OPS-3** | No single failure sends an order twice. Each order carries its journaled `client_order_id`; recovery resubmits only after the broker confirms the order is absent; one writer per stream is guaranteed by the writer epoch, not by the orchestrator | Rule 5; journal §5.1, §5.2; trading §5.7 | Fault injection: zero duplicates (Phase 1 gate); a test that starts two executors for one account and shows the older one is `Fenced` before it can send |
| **OPS-4** | **Given an available journal,** no single failure loses a risk exit or protection: a journaled risk exit is re-driven by recovery, and the kill switch depends on no model, research agent, global control plane, or telemetry. **Whether or not the journal is available,** no deploy, restart, failover, or journal outage cancels a protective order resting at the broker. While the journal is unavailable, a new risk exit is **held**, not sent: that is the disclosed limit of §2.1, not part of this invariant | Rule 13; rule 5; trading §5.4, §5.5; HLD §5 Durability; §2.1 | Fault injection during exit sequences with the journal up; a test that runs the kill switch with the model gateway, the global control plane, and the metrics exporter all unreachable; the upgrade drill (OPS-7). For the journal-down case, a separate test asserts what §2.1 states and nothing more: no order of any kind is sent, no resting protective order is canceled, the hold is alerted, and the owed exit is the first thing sent once appends succeed |
| **OPS-5** | Every environment except production has no path to live money: no live host compiled in, no live credential in its vault, and no network egress to a live trading host. **Narrowing of ES-23 and of this invariant, for paper Alpaca OAuth only ([DEC-821](../project/decisions/DEC-821.md) items 2 and 5):** one live-host URL, `POST https://api.alpaca.markets/oauth/token`, is compiled in, only in the token-exchange client, and only the token-exchange process (§3.1) has egress to that host. No executor, runtime, or API process has egress to any live host. Nothing else in this invariant changes | Rule 8; ES-23; [DEC-821](../project/decisions/DEC-821.md) | CI forbids the `live` feature; a build test that the only live-host URL compiled in is the token-exchange client's; an egress test in staging that a request to each live trading host fails at the network layer from every process but the token-exchange process; the token-exchange client's pinned tests (connections spec §5.2 step 4); the tracer refuses any host but Alpaca's paper host (E7-7) |
| **OPS-6** | Tenant isolation holds at every layer: agent processes are never shared across workspaces; rows carry `workspace_id` with row-level security; data and vault paths are per workspace, under per-workspace keys; telemetry and alerts carry opaque IDs only | HLD §8; journal §6.1, §6.5 | Cross-workspace access tests at the API, database, vault, and messaging layers (07, Isolation); a label lint on the metrics registry |
| **OPS-7** | A deploy, upgrade, restart, or rollback never drops a running agent's protection, and never interrupts an exit sequence in a way the trading spec does not already bound | Trading §5.4; FR-9.3 | The upgrade drill: upgrade every process type with open positions, exits in flight, and pending approvals, and assert no protective order was canceled by the deploy and every unprotected interval stayed within `max_unprotected_s` |
| **OPS-8** | Backups restore to a verifiable journal. After any restore, journal §11 verification passes over every stream from its trusted start to the restored head, and every anchor and `SegmentExported` is consistent with the restored heads; otherwise the restore is an integrity incident and no agent trades. **Passes** here includes a run that ends `incomplete` on the token check alone: every event walked, nothing failed, and the timestamp token the only check left unproven ([DEC-789](../project/decisions/DEC-789.md) item 9, the founder). Any other `incomplete`, and any `fail`, is an integrity incident | Journal §10, §11 | The restore drill (§6.4) runs journal §11 verification, whose result is journaled as `VerificationRun`; the drill's own record (which backup, which drill, pass or fail) is journaled once the journal spec defines its events (E21-25, journal spec first); a test that restores a backup older than the last anchor and asserts the incident path, not a silent resume |
| **OPS-9** | A restored or recovered agent trades only after replay and broker reconciliation pass; the broker is the source of truth for orders and positions; anything unexplained pauses the agent until the owner acknowledges with step-up | Trading §11; HLD §6 D | Fault injection and restore drills; reconciliation reference cases |
| **OPS-10** | Notifications and operator alerts carry no sensitive content: opaque IDs and generic text only | Rule 6 | Payload capture tests (07, Privacy) extended to the paging channel |
| **OPS-11** | Telemetry is never the audit record and never an input to a trading decision; losing all telemetry changes no order | DEC-73 | A test that runs the decision cycle with the exporter failing; a layering check that no core crate depends on the telemetry API |
| **OPS-12** | The global control plane is never in the trade path: its outage stops no trading, no kill switch, and no reconciliation | HLD §4; PRD §7 Availability | A staging drill with the outbound link cut |
| **OPS-13** | Schema changes are forward-only and applied only by `mandate-cli db migrate` under the migration-owner role, never at process startup; application roles have INSERT and SELECT only on journal tables | ES-08; journal §6.1 | The existing xtask lint and Postgres tests; DDL and superuser sessions alert (journal §6.1) |
| **OPS-14** | A host clock out of tolerance never adds risk: it is measured and journaled, events recorded meanwhile are flagged, and broker `event_time` stays authoritative for executions | Journal §5.4 | A test feeding an out-of-tolerance offset to the scheduler |
| **OPS-15** | Every release that runs against a broker is identified: its build digest is journaled from the first event (`actor.build`), and in staging and production it is a signed artifact whose signature is checked before start | ES-17 | A startup check that refuses an unsigned binary outside dev and CI |

### 2.1 The journal-outage hold: a disclosed limit, and the founder's decision

Rule 5 (journal before acting) and rule 13 (risk reduction is never denied) meet at one point.
While the journal is unavailable, an executor cannot commit `OrderSubmitted`, so it can send **no**
order: no opening, and also no risk exit, no owner exit, no kill-switch close, and no re-placement
of protection. One failure, the journal, therefore holds every new exit.

Rule 13 lists the only things that may hold an exit: agent mode `paused` or `stopped`, an `Unknown`
order in the same instrument, or the broker. Journal unavailability is not on that list, and neither
the trading spec nor the mandate spec says what an exit does then. So this is a gap between two
non-negotiable rules, and closing it is the founder's decision (DEC-79), recorded as Proposed in
[DEC-434](../project/decisions/DEC-434.md) item 21. This document does not resolve it.

**What the design does meanwhile: the most conservative option.** Nothing is ever sent without
being journaled first. During a journal outage:

| Question | Answer |
|---|---|
| What is held | Every order the executor would send, exits and kill-switch closes included |
| What still protects the position | Orders already resting at the broker: bracket and OCO legs, and crypto stop-limits (trading §5.4). No outage, deploy, restart, or failover cancels them (OPS-4) |
| The backstop's own limits | Equity stops trigger in the regular session only; a crypto stop-limit can miss on a gap; a fractional remainder and a position inside an unprotected interval have no resting order (trading §5.4) |
| The kill switch | The owner's command cannot be journaled either, so the platform's kill switch does not act until the journal returns (§3.6). The owner can always cancel and close directly at their broker; reconciliation later ingests that as external activity (trading §7.1) |
| Who is told | The operator, by the journal-unavailable alert (§8.2, RB-07) |
| How it ends | Appends succeed again. The executor reads fresh state, and exits owed are evaluated and sent before anything else (the priority channel, ES-06) |
| What narrows the window | The synchronous standby and fenced failover (§4.1), and the failover drill (§6.4). Journal availability is the first thing §6 and §11 protect |

**Exposure, stated plainly.** If the journal is down, a risk limit confirms, and the market moves
through a stop outside the regular session (equities) or gaps through a stop-limit (crypto), the
position can lose more than the mandate's limit intends, and the platform does nothing until the
journal returns.

---

## 3. Runtime topology

### 3.1 Processes in a workspace deployment

| Process | How many | Writes | Holds | Network (egress allow-list, §9) | Host-local channels (not network) |
|---|---|---|---|---|---|
| **Agent runtime** (`mandate-runtime` inside the shell; see `docs/specs/agent-harness.md`) | One per agent deployment (DEC-08) | Its agent stream | No broker credential (rule 12) | Postgres; model gateway; artifact store. No broker, no internet | Reads the workspace's data service |
| **Account executor** (`mandate-executor` plus a connector) | One per broker account | That account's stream (journal §2) | A vault lease for that one connection. Never the platform's OAuth client secret | Postgres; vault (its one connection's lease, and while `connecting` one write: the broker account id into the personal-data vault under the pending connection's path); that connection's broker hosts only. A paper connection's executor reaches paper hosts only: no executor's egress contains a live host ([DEC-821](../project/decisions/DEC-821.md) item 4) | Reads the workspace's data service |
| **Token-exchange process** (paper Alpaca OAuth only, [DEC-821](../project/decisions/DEC-821.md) item 2) | One per code exchange, started by the connection manager for one `connecting` connection; it stops when the exchange ends | Nothing: no journal stream, no writer epoch | The single-use grant on the platform's OAuth client secret; while it runs, the code, the PKCE verifier, and the token it receives, as `secrecy` values. No order client, no connector, and no token lease | Vault (redeem the grant; read once the code and verifier under the pending connection's path; write the token there and delete the code). One network destination: `api.alpaca.markets` on 443, for the one call `POST /oauth/token` that its dedicated client can make. Residual, disclosed: allow lists work on addresses, not paths, so at the network level it can reach the whole live API; the client type is the only barrier in this process ([DEC-821](../project/decisions/DEC-821.md) item 4; [connections spec](../specs/connections.md) §5.2) | None |
| **Scheduler** | One per workspace | The scheduler stream (`clock:`) | Nothing secret | Postgres; time sources | None |
| **Workspace control services** (Phase 1: the founder's CLI) | One set per workspace deployment | The control stream | Session keys for the identity provider; an OAuth authorization code only in transit, during the callback ([connections spec](../specs/connections.md) §5.2). Never the client secret or a token | Postgres; identity provider; users; vault, with these named capabilities and no read: write and delete under pending connections' credential paths (not the personal-data vault, whose entries end only by the retention erasure of journal §6.4); issue and revoke of the token-exchange process's single-use client-secret grant; and the keyed fingerprint computation, which returns only the result ([connections spec](../specs/connections.md) §3.1, §5.2 steps 3, 5, and 6) | None |
| **Model gateway** ([inference spec](../specs/inference.md)) | One per workspace deployment, replicated; exactly one replica holds the meter writer role per workspace (inference spec §3.6; see below) | Each workspace's meter stream, `meter:{workspace_id}` (proposed, E15-8), by the holder only. The call record itself, `ModelInvocationRecorded`, is appended by the caller | Provider keys | Postgres; vault; the meter holder (other replicas); allowed model providers only | None |
| **Cold exporter and anchorer** | One per workspace deployment | Control stream (`SegmentExported`, `AnchorComputed`) | Object-storage write credential | Postgres; object storage; timestamping authority | None |
| **Market data service** ([data-plane spec](../specs/data-plane.md)) | Per workspace in v1 (no redistribution, HLD §12 item 3) | Parquet datasets; no journal stream | The workspace's data credential | That data host | Serves the workspace's runtimes and executors |

Two writers, two streams: the runtime writes the agent stream and the executor writes the account
stream (DEC-131). Facts cross between them only as journaled copies (journal §2). Processes find
each other's events through journal tailing with Postgres `LISTEN`/`NOTIFY` as a wake-up hint
(DEC-17); a lost notification costs latency, not correctness.

**The market-data channel is host-local, and it is not in the egress allow-list.** The data-plane
spec (§3.6) has the data service push normalized quotes, instrument statuses, and LULD bands to the
workspace's runtimes and executors over an in-process or same-host channel. Here that is a Unix
socket or loopback endpoint on the shared host, bounded and read-only for the consumers, one per
workspace and never reachable from another workspace's processes (OPS-6). It crosses
no network boundary, so the "Network" column, which §9 turns into the egress allow-list, does not
list it; the last column does. An allow-list built from this table must leave that local channel
open: without it no quote reaches the executor, no `MarkUpdated` is appended, every held
instrument goes stale, and every opening is denied. Nothing about the channel changes what is
journaled: marks enter risk state only as journaled `MarkUpdated` events (data-plane §3.6).
The placement this requires is in §3.2 (DEC-434 item 22).

**The model gateway has one meter writer per workspace.** The inference spec (§3.6, v0.2, DEC-432
item 20) gives the gateway a meter stream per workspace and requires a call's reservation to be
appended before its first attempt leaves. A stream has one writer, fenced by epoch (journal §2,
§5.1). The gateway may run as several replicas, but exactly one holds the meter writer role for a
workspace; it alone checks caps and appends, and every other replica asks the holder to reserve
before it sends anything. Operationally:

- A replica that cannot reserve refuses. If the holder is unreachable within the call's deadline,
  or the replica's own append returns `Fenced`, the call ends `meter_unavailable`: nothing is sent
  to a provider, and the caller records the output as missing, which never adds risk (inference
  spec INF-4).
- Each such refusal is counted and alerts the operator (§8.2), so a replica that lost the role is
  never a silent partial outage.
- Hand-over is by epoch, as for any writer (§3.4, §7.3): the new holder takes the next epoch and
  folds the stream before it reserves, and unsettled reservations stay counted.

DEC-434 item 23 records this reading. It replaces v0.1's row, which said the gateway wrote nothing
to the journal.

### 3.2 Scheduling and placement

- **Managed:** one Kubernetes cell per region (HLD §11). Each process is its own pod. A workspace's
  processes stay in one cell. In v1 a workspace's data service, executors, and runtimes are placed
  on one node (pod affinity), because the market-data channel is host-local (§3.1, data-plane
  §3.6). The journal stays the only channel between a runtime and an executor. Spreading a
  workspace over nodes later turns the data channel into a network route: a data-plane spec change
  and a new allow-list entry, not a scheduling setting.
- **Hybrid:** Helm on the customer's cluster, or Docker Compose on one host (FR-9.2). A single host
  is a supported shape for a small workspace; its failure walk is §11's "node loss" with no
  standby node.
- **Phase 1 paper:** one host, the processes under a process supervisor (systemd or Compose), one
  Postgres on the same host or next to it.
- **Placement rules:** an executor is never co-scheduled with another workspace's processes on a
  shared kernel without a container boundary; agent processes are never shared across workspaces
  (HLD §8). Retail workspaces may share nodes; business workspaces may get a dedicated node pool or
  cell.

### 3.3 Resource limits

- Every process has CPU and memory limits (HLD §5). Memory has no swap, so an overrun ends in an
  out-of-memory kill, which is a crash, which recovers from the journal (ES-09: `panic = "abort"`).
- The executor and the scheduler get a guaranteed CPU share (requests equal to limits): they carry
  the kill switch, protection, and the risk clock.
- The runtime's model calls are bounded by deadlines in the gateway, and the research agent's spend
  by `cost_cap_usd_per_day` (mandate spec, DEC-120), not by process limits.

### 3.4 Health, readiness, and restart

| Probe | Meaning | Effect when failing |
|---|---|---|
| **Liveness** | The process loop has handled an input or a heartbeat within its bound | The orchestrator restarts it |
| **Readiness** | The process holds its writer epoch, has replayed its stream to the head, and its last reconciliation passed (executor) or its mode is known (runtime) | It is not ready: it takes no new openings, and the deployment manager reports the agent as `Recovering`. A runtime that is not ready still forwards kill-switch commands; an executor that is not ready still runs risk exits and protective re-placement once its own replay is done |
| **Start-up checks** | Signed build (OPS-15), environment matches the stream (ES-23), Postgres durability settings (OPS-2), object lock mode (journal §6.2, hybrid and on-prem), clock offset (journal §5.4) | The process refuses to start trading and alerts |

Restart policy: always restart, with exponential back-off. After a crash loop (Proposed: five
restarts in ten minutes), the deployment manager leaves the agent `Paused` and alerts; it does not
keep restarting a process that crashes during recovery (HLD lifecycle: `Recovering` → `Paused`).

**At most one writer is the fence's job.** The orchestrator may briefly run two copies of a
process (a partition, a slow termination, a manual mistake). Safety does not depend on it: a new
process takes ownership by incrementing `writer_epoch`, and every append by the old one returns
`Fenced` before any side effect (journal §5.1). The executor checks its epoch by appending
`OrderSubmitted` before every broker request, so a fenced executor cannot send.

### 3.5 How a process finds what it needs

A process starts with only its identity: workspace ID, its subject (agent deployment ID or account
reference), and a workload identity issued by the orchestrator. Everything else it reads:

1. **Journal:** it connects with its process-type database role (INSERT and SELECT only) and takes
   its stream's writer epoch.
2. **Mandate version:** the control stream's `AgentDeployed` and `MandateVersionApplied` name the
   mandate version by content hash; the body comes from the content-addressed configuration store.
   A missing object halts replay (journal §8); the process never falls back to a default mandate.
3. **Credentials:** only the executor asks the vault, authenticating with its workload identity, for
   a lease on its one connection (§5). The token-exchange process, started by the connection
   manager for one `connecting` paper Alpaca OAuth connection, redeems one single-use grant on the
   platform's OAuth client secret and reads that connection's code and verifier once, for its one
   code exchange; it writes the token but takes no lease on it (§5.2; [DEC-821](../project/decisions/DEC-821.md) item 2). No executor reads
   the client secret. The runtime asks for nothing.
4. **Configuration:** fee tables, calendars, and instrument snapshots by content hash
   (`config_refs`), never by "latest".

### 3.6 The kill-switch path

The owner's kill switch is journaled on the control stream (`OwnerCommandIssued`) and copied by the
stream owners, which journal `KillSwitchActivated` in their own streams (journal §2). The path
needs exactly three things: Postgres, the executor process, and the broker (with the executor's
cached vault lease). It does not need the model gateway, the research agent, market data beyond
the current status the spec requires, the global control plane, telemetry, or the web app (an
owner can also use the CLI). Postgres is on that list: while the journal is unavailable the
platform's kill switch does not act, which is the limit §2.1 discloses. Kill-switch and risk-exit commands are read first from a priority
channel (ES-06), so a full input queue cannot delay them. The managed global switch cannot reach
into hybrid or on-prem deployments (HLD §8).

---

## 4. Data stores

### 4.1 The journal hot store (Postgres)

- **Shape.** One logical journal per workspace deployment. In managed cells, a Postgres cluster
  per cell, with row-level security per workspace for retail and a dedicated database or cluster
  for business tiers (HLD §8). Tables, roles, and triggers are those of journal §6.1, as built in
  `migrations/0001_journal.sql` and `mandate-journal-pg`.
- **Durability.** `synchronous_commit = on`; in managed live deployments, at least one synchronous
  standby in a second failure domain acknowledges before an append returns (journal §5.3).
  Target: p99 append under 5 ms including the standby.
- **High availability.** A failover manager with fencing of the old primary. It promotes only a
  synchronous standby, which holds every acknowledged append. If no synchronous standby is
  available, the primary keeps waiting for one rather than downgrading to asynchronous commit, so
  appends return `Unavailable` and agents hold; durability is never traded for availability on a
  live journal.
- **Phase 1 paper.** One Postgres without a standby, with continuous WAL archiving (§6). Paper
  carries no money, so an outage costs soak time only.

**What happens to an in-flight intent during failover.**

| The primary fails… | The writer saw | What it does next | Result |
|---|---|---|---|
| Before the append's commit | `Unavailable` or a broken connection | Retries with the same drafts on the new primary (journal §5.1) | Committed once, or `AlreadyCommitted` |
| After commit, before the acknowledgment reached the writer | `Ambiguous` | Re-queries by `event_id` on the new primary before acting (journal §5.1) | Finds it committed (the standby had it) and proceeds; never sends twice |
| After `OrderSubmitted` committed, before the broker request | — (the executor crashed or lost its epoch) | Recovery folds the stream, finds `OrderSubmitted` with no acknowledgment, queries the broker by `client_order_id`, resubmits only if absent (journal §5.2, trading §5.7) | At most one order |
| After the broker request, before the acknowledgment was journaled | The order is `Unknown` | Lookup by `client_order_id` (trading §5.7) | Adopted from the broker's state |

### 4.2 The cold store and object storage

- Closed segments, manifests, artifacts, configuration objects, timestamp tokens, and verifier
  releases go to object storage with **object lock in compliance mode**, retained six years and
  extended while a supported position, lot, or account is open (journal §6.2, trading §13,
  DEC-33). Segments ship within one minute of closing before live capital; daily in Phase 0.
- A second-region replica exists before live capital (journal §6.2).
- The pure half (manifests, per-range checks, export) is `mandate-journal-cold`. The shipping job,
  object lock, the replica, the evictor, and the drill are E5-7.
- The hot store's rows are deleted only by the evictor role, only up to the last **verified** cold
  segment, journaled as `SegmentEvicted` (journal §6.1). No eviction runs in Phase 0.
- **Object storage interface.** S3-compatible with compliance-mode object lock. Managed mode uses
  the hosting provider's object store; hybrid and on-prem use the customer's. The HLD names MinIO
  for the edge; its server license is AGPL-3.0, so bundling it in our installer is a licensing
  question for counsel, like Restate's in ES-20. Until then the installer requires the customer to
  supply an S3-compatible store with object lock, and bundles none (DEC-434 item 16).

### 4.3 The artifact store

- Content-addressed by `sha256:{hex}` of the stored bytes (journal §6.3). Today the filesystem
  backend `mandate-artifacts-fs` writes to a temporary file, flushes, and hard-links into place, so
  a crash leaves at most a temporary file.
- **Ordering rule:** an artifact is durable before the journal event that names it is appended.
  A crash between the two leaves an unreferenced artifact (harmless), never an event pointing at
  nothing (`artifact_missing`, journal §11). DEC-434 item 5 records this reading.
- In managed and hybrid production, artifacts move to object storage under the same lock and
  retention as segments, with the same key layout. Artifacts that may contain personal data are
  encrypted under a vault-held key and hashed as ciphertext (journal §6.3).

### 4.4 Market data

- `mandate-marketdata` stores bars and trades as Parquet with exact decimals (ES-23). These
  datasets are **caches**: re-downloadable and not trading records.
- The data that informed a decision is a record: the runtime journals observations, and data
  snapshots that informed decisions are artifacts (HLD §7). So losing the Parquet cache loses no
  record.
- v1 data comes through each user's own Alpaca account; there is no shared redistribution until
  licensing exists (HLD §12 item 3). Ingest topology and the shared data plane are
  `docs/specs/data-plane.md`'s subject.

### 4.5 Retention

| Data | Kept for | Where | Erasable |
|---|---|---|---|
| Journal events | Six years after the later of creation and the close of what they support; extended by holds (DEC-33) | Hot until verified cold and evicted; cold write-once | No. Personal data is referenced and crypto-shredded (journal §6.4) |
| Artifacts, configuration objects, timestamp tokens, verifier releases | As the records they support | Object storage, write-once | As above |
| Personal data | Records period for identity records; otherwise per E5-5 | Personal-data vault, per-person keys | Yes, by key destruction, journaled `PersonalDataErased` |
| Postgres backups and WAL archive | Proposed: 35 days of point-in-time recovery, plus monthly fulls kept 13 months | Object storage, separate bucket and keys, governance-mode lock | Expire by policy |
| Market data caches | Until replaced | Local disk or object storage | Yes |
| Logs | Proposed: 14 days | Inside the workspace deployment | Yes |
| Metrics | Proposed: 30 days at full resolution, 13 months downsampled | Inside the cell or site; fleet counts in the global plane | Yes |
| Vault audit log | As the records period (it records who used which credential) | Vault's audit device, shipped write-once | No |

---

## 5. Secrets and the vault

### 5.1 What is secret

| Secret | Used by | Custody |
|---|---|---|
| Broker credentials (API keys, OAuth tokens) | The account executor for that connection | Workspace vault; write-only after entry: no person reads them back |
| Broker OAuth client secrets (the platform's registered app, e.g. Alpaca) | The token-exchange process only, for the one code exchange, through a single-use grant ([connections spec](../specs/connections.md) §5.2; DEC-690 item 1; [DEC-821](../project/decisions/DEC-821.md) item 2) | The cell's vault; no person, no executor, and no API process reads it. Custody in hybrid and on-prem is open |
| Model provider keys | The model gateway | Workspace vault (customer-supplied keys) or the cell's vault (platform keys) |
| Database credentials | Each process type's role | Short-lived, issued by the vault per process |
| Object-storage write credentials | Cold exporter, artifact writer | Vault, per workspace prefix |
| Per-workspace data keys and per-person keys | Encryption at rest; personal-data vault (E5-5, journal §6.5) | Key management service or the customer's HSM; versions kept for the retention period |
| mTLS identity to the global control plane | The workspace deployment's outbound link | Vault; rotated |
| Release signing key | The release pipeline | The founder's hardware key (ES-17); never on a server |

### 5.2 Custody and scoping

- **Managed:** a vault backed by a key management service, per cell, with a namespace per
  workspace. **Hybrid and on-prem:** the customer's vault; credentials never leave the customer's
  environment (HLD §8).
- **Per-process scoping.** An executor's workload identity can read exactly one connection's
  credential, and never the platform's OAuth client secret. The token-exchange process ([DEC-821](../project/decisions/DEC-821.md) item 2;
  connections spec §5.2) is the only identity that reads the client secret, through a single-use
  grant. The connection manager asks the vault to issue the grant when it starts that process. The
  vault binds it to that process's workload identity, with a lifetime of the code's 10 minutes, and
  revokes it when the exchange ends, at `ConnectionEstablished`, or at teardown. The same identity
  may read the pending connection's code and verifier once, and write the token, but holds no lease
  on the token. A refresh, if Alpaca ever issues refresh tokens, would run in that process with a
  new grant (connections spec §5.4, U-A3, open). A runtime's identity can read none. The model gateway's can read its workspace's provider
  keys. Nothing has a wildcard policy.
- **Leases.** The executor holds its credential in memory as a `secrecy` value (ES-09), renews its
  lease ahead of expiry, and drops it on stop. A vault outage does not stop a running executor
  until the lease expires (Proposed lease length: 24 hours, renewed hourly); it does stop new
  processes from starting, so their agents stay `Recovering` with protection at the broker.
- **Live credentials** come only from the vault, never from environment variables (ES-23). Paper
  keys for dev and the Phase 1 paper environment come from local configuration outside the
  repository (ES-19).

### 5.3 Broker key permission checks

At connection time and at every executor start:

1. **Scope:** an OAuth grant must contain only trading and account-read scopes (E7-1). An API key
   whose venue exposes its permissions must not allow withdrawals or transfers; one that does is
   refused. Where a venue cannot report a key's permissions, a live key is refused; a paper or
   demo key is accepted with the limit recorded and disclosed to the owner
   ([connections spec](../specs/connections.md) CN-2, [DEC-441](../project/decisions/DEC-441.md)
   item 4).
2. **Environment:** the credential must work against the environment the stream records and only
   that one. In non-production builds only paper hosts exist (ES-23), Alpaca's token endpoint in
   the token-exchange client aside (§2, OPS-5); in production a paper stream refuses a live
   credential and the reverse.
3. **Account:** the account the credential reaches must be the account the connection names (the
   dedicated agentic account for Robinhood, E7-6).

**Narrowings for paper Alpaca OAuth only ([DEC-821](../project/decisions/DEC-821.md) item 5).** Each cites DEC-821 and changes nothing
else in its rule:

- **ES-23 and §2 (OPS-5):** the one token endpoint, `POST https://api.alpaca.markets/oauth/token`,
  reachable only by the token-exchange process's dedicated client.
- **CN-3 and item 2 above:** a grant requested with `env=paper`, whose single-use `state` names a
  request the server itself issued with `env=paper` for that workspace, is treated as paper-only.
- **DEC-441 item 21:** superseded for `env=paper` grants only. A grant naming `live` or both
  environments is still refused, and so is every live Alpaca OAuth connect; a live Alpaca OAuth
  connection needs a new decision ([DEC-821](../project/decisions/DEC-821.md) item 7).

DEC-441 item 22's conditions apply in full ([DEC-821](../project/decisions/DEC-821.md) item 4). Before the connection is established, the
token's possible breadth (that Alpaca's live host might honour it) is journaled with the connection
and disclosed to the owner; the journal spec change that adds that event comes first. Orders, and
every trading or account call, go only to the paper trading host, from the executor.

Each check's result is journaled without the credential. The executor appends checks 1, 2, 3, and
7 on its account stream as `ConnectionChecked`; checks 5 and 6 come from `AccountStateObserved`
(E7-17, PR #786; journal §9.2). Check 4, uniqueness, runs in the
connection manager. The control stream's `ConnectionEstablished` cites the passing results
([connections spec](../specs/connections.md) §5.2, §8.1).

### 5.4 Rotation and revocation

- Broker credentials rotate when the owner reconnects or the OAuth token refreshes. The vault
  issues the new value; the executor picks it up at its next renewal with no restart.
- Platform secrets rotate on a schedule (Proposed: 90 days, and immediately on any suspicion).
- Data-key rotation never destroys old versions; rotations and revocations are journaled
  (`KeyRotated`, `KeyRevoked`, journal §6.5).
- Revoking a connection (`ConnectionRevoked`) revokes its vault lease; the executor holds no
  further orders for it and alerts the owner, whose resting protection stays at the broker.

### 5.5 Break-glass

- Platform staff in managed mode have **break-glass access only, with the customer's approval,
  journaled to a stream the customer can read** (journal §7).
- Break-glass is time-bound and two-person (Proposed). It grants operational actions (pause, kill
  switch, restart, read verification results), never a credential read-out: credentials are
  write-only by policy.
- The managed global switch cannot reach into customer deployments (HLD §8).

### 5.6 The vault product

The founder chose OpenBao (DEC-434 item 14, accepted 2026-10-03). The options considered:

| Option | Fits every mode? | Notes |
|---|---|---|
| **OpenBao** (open-source fork of Vault) | Yes, same software in every mode | Open-source license (to be confirmed at decision time); auto-unseal with a cloud key service in managed mode and the customer's HSM or key service on-prem |
| HashiCorp Vault | Yes technically | Business Source License: bundling it in our installer needs counsel, as with Restate (ES-20) |
| A cloud secrets manager plus key service | Managed only | Breaks the same-installer rule; on-prem would need a second implementation |
| Our own envelope encryption over a key service | Yes | Least new software, most new safety-critical code to write and review |

**Decided:** OpenBao in every mode, behind a narrow `SecretSource` interface in the shell so the
choice stays reversible. Phase 1 keeps ES-19's local paper keys, and no live credential exists
anywhere.

---

## 6. Backups and disaster recovery

### 6.1 Targets

| Scope | Recovery point (data loss) | Recovery time (to trading again) |
|---|---|---|
| Process or node loss | 0 | Under 2 minutes per agent |
| Postgres primary loss, managed live | 0 for acknowledged appends (synchronous standby, journal §5.3) | Under 2 minutes |
| Region loss, managed live | Under 1 minute for the cold store (segments ship within a minute); the hot tail since the last segment is recovered from the cross-region replica if it survived | Under 4 hours, agents resuming only after reconciliation (OPS-9) |
| Phase 1 paper | The WAL archive's lag (Proposed: under 5 minutes) | Best effort; paper only |

The founder accepted these targets (DEC-434 item 17). **The region-loss row depends on item 13.**
It assumes a second region, which is chosen with the hosting provider at M8. Until a second region
exists, the row is a target and not a capability: a region loss then ends only when the region
returns or a new one is built, and what survives is what the cold store's replica holds (journal
§6.2 requires that replica before live capital). The two items take effect together, and the
region-evacuation drill (§6.4) starts only then.

### 6.2 What is backed up, and how

| Data | Method |
|---|---|
| Journal hot store | Continuous WAL archiving plus periodic base backups to object storage in another failure domain, encrypted under a backup key separate from the database's; point-in-time recovery |
| Cold store and artifacts | Already write-once; the second-region replica is the backup (journal §6.2) |
| Vault | The product's own encrypted snapshots, under keys held apart from the data they protect; the unseal or root material is split among holders (Proposed: the founder and one escrow) |
| Configuration store | Content-addressed objects in the artifact store, so covered above |
| Market data caches | Not backed up; re-downloaded |
| Deployment configuration | In the operator's private configuration repository, outside this public repository |

### 6.3 Restore procedure

A restore never repairs the journal in place and never resumes trading on its own.

1. **Freeze.** Every agent in the restored workspace starts in `Recovering`; no openings.
2. **Restore** the hot store to the latest consistent point, then **re-import** any later cold
   segments that verify (they are canonical bytes with manifests; `mandate-journal-cold`'s
   importer and checks).
3. **Verify** every stream with journal §11 from its trusted start to the restored head. The step
   passes when every `VerificationRun` is `pass`, or `incomplete` with the token check its only
   incomplete check and every event walked (journal §11 "Incomplete", DEC-789 item 9). Any `fail`,
   or any other incomplete check, is an **integrity incident**, as in step 4.
4. **Check against anchors and exports.** For every stream, compare the restored head with the
   latest `AnchorComputed` leaf and `SegmentExported` manifest held outside the restored database
   (cold store, and in hybrid mode the global plane's anchor copies). If any of them names a `seq`
   beyond the restored head, events were lost: that is an **integrity incident** (journal §11 "On
   failure"): SEV-1, affected segments on legal hold, and a new writer epoch continues from an
   `IntegrityIncidentRecorded` event that references the last good hash and anchor.
5. **Reconcile** each account with its broker (trading §11). The broker is the source of truth for
   orders, fills, and positions. Missing fills are ingested; order states are adopted through
   journaled compensating events; an order the restored journal does not know is external activity
   (trading §7.1), even if we sent it before the loss, because the restored journal cannot prove
   otherwise.
6. **Resume** only per trading §11: any mismatch leaves the agent `paused`, and resuming a paused
   agent requires the owner's acknowledgment with step-up authentication.

### 6.4 Drills

**Where a drill's result is recorded.** The journal's event catalogue is a closed list, and it has
no backup or drill event today. Telemetry cannot hold the record (OPS-11). So the journal spec
change comes first: E21-25 adds the backup and drill events with test vectors, and E21-5 is blocked
on it. Until then a drill journals only what the catalogue already has (`VerificationRun`,
journal §11), and "journaled" in the table below means "journaled once E21-25 lands". A drill's
verification "passes" as OPS-8 says: `pass`, or a token-only `incomplete` (DEC-789 item 9).

| Drill | Where | Frequency (Proposed) | Pass condition, journaled |
|---|---|---|---|
| Hot-store restore and verify | Staging; the paper environment in Phase 1 | Monthly | Steps 1 to 4 pass; a canary scan finds no secret in the restored data (OPS-1) |
| Cold-store restore and verify (E5-7) | From the replica | Quarterly (journal §6.2) | §11 per-range checks pass against anchors; a token check left `incomplete` alone still passes (DEC-789 item 9), any other `incomplete` or any `fail` does not |
| Restore older than the last anchor | Staging | Each release that touches recovery | The integrity-incident path runs and no agent resumes (OPS-8) |
| Postgres failover | Staging, under load with intents in flight | Monthly and each Postgres upgrade | Zero duplicates, zero lost acknowledged appends |
| Region evacuation | Staging | Twice a year, once a second region exists | Agents resume only after reconciliation |

---

## 7. Deploy and upgrade

### 7.1 From main to a release

1. **Merge.** Changes land on `main` by label (DEC-175) after CI and an independent review
   (DEC-79). Nothing deploys from a branch.
2. **Build.** A release job builds from a `main` commit with `--locked`: static Linux binaries
   (rustls only, ES-17), container images pinned by digest, and from M6 a reproducible build
   (`SOURCE_DATE_EPOCH`, a double-build diff) with an SBOM and cargo-auditable metadata (ES-14,
   ES-17).
3. **Sign.** From M11 or the first external install, images and bundles are signed with cosign
   under the founder's hardware key, with build provenance (ES-17). Every process checks its own
   signature at start (OPS-15) and journals its build digest (`actor.build`).
4. **Promote.** `main` → staging automatically; staging → production by the founder's approval
   while live money is involved (DEC-79). Production rolls out one cell at a time, starting with a
   canary cell of internal workspaces (Proposed).
5. **Distribute.** Hybrid sites pull signed bundles over their outbound link; air-gapped sites
   receive signed bundles and install them by hand (HLD §4).

### 7.2 Migrations

- Forward-only SQL in `migrations/`, applied only by `mandate-cli db migrate` under the
  migration-owner role, as a separate deploy step **before** processes roll; never at startup
  (ES-08, an xtask lint).
- **Expand-only across one release:** a migration must work with both the running release and the
  next, so a rollback never needs to reverse DDL.
- Journal event changes are not DDL: they are new `schema_version`s with pure upcasters, and stored
  events are never rewritten (journal §8).
- DDL and superuser sessions alert (journal §6.1). A planned migration's alert is acknowledged
  against its release ID; any other is SEV-1.

### 7.3 Upgrading a running agent: drain and hand over

No agent process is upgraded in place. The old process drains, then the new one takes the stream.

1. **Warm.** The deployment manager starts the new version in standby: it replays the stream
   read-only, holds no writer epoch, and sends nothing.
2. **Drain.** The old process is told to drain. It starts no new decision cycle and no new opening.
   It finishes, or bounds, what is in flight:
   - no order may be left between `OrderSubmitted` and the broker request: it completes the request,
     and the response or `Unknown` is journaled;
   - an exit sequence that has canceled protection is carried to its end, which trading §5.4
     already bounds by `max_unprotected_s` (default 60 seconds), after which protection is
     re-placed;
   - resting protective orders are **never** canceled or replaced for a deploy;
   - pending approvals and durable timers need nothing: they are journal state (DEC-16).
   A drain that has not finished within its bound (Proposed: 90 seconds) is abandoned, not forced:
   the old process exits, and the new one recovers as after a crash.
3. **Hand over.** The old process exits. The new one increments `writer_epoch`, which fences the
   old one even if it is still alive, finishes replay, reconciles (executor), and becomes ready.
4. **Kill switch during hand-over.** The owner's command is journaled on the control stream first,
   so whichever process holds the epoch acts on it, and the new process reads it during replay.
   The command is never lost, only delayed by the hand-over.

Executors are upgraded outside the regular US equity session by default (Proposed), because the
hand-over is a window in which no new order is sent. Crypto trades around the clock, so crypto
accounts accept the short window, with protection resting at the broker throughout.

Mixed versions are normal during a rollout: a runtime and an executor communicate only through
journal events with schema versions, and a new release must read every event the previous one
writes (golden tests, journal §8). A `fold_version` change requires a journaled re-derivation.

### 7.4 Rollback

- A binary rollback is allowed only to a release that can read every `schema_version` the newer
  release wrote. Each release records whether it is rollback-safe. If it is not, the fix rolls
  forward.
- Migrations are never reversed (§7.2).
- A bad deploy is stopped at the canary cell. If one reached agents, the operator rolls back
  through the same drain and hand-over; the journal and the broker are unchanged by a rollback.

---

## 8. Observability and on-call

### 8.1 Telemetry

- **The journal is the audit record; telemetry never is** (DEC-73). Telemetry may be lossy and is
  never an input to a decision (OPS-11).
- **Metrics** through the OpenTelemetry API with a Prometheus pull exporter from M6 (ES-18), which
  works air-gapped with no collector. Push export (OTLP) only under a data policy, from M8.
- **Logs:** `tracing` JSON to local files, opaque IDs only (ES-09). A test scans log output for
  symbols, prices, and quantities.
- **Labels** are opaque IDs and enumerations only: workspace, agent, account reference, process
  type, reason code, release. Never a symbol, side, quantity, price, mandate field, or person
  (`AGENTS.md` "Do not"). A lint on the metrics registry enforces the allowed label set.
- **Where it goes:** managed telemetry stays in the cell; only fleet health (versions, heartbeats,
  counts) reaches the global control plane. Hybrid and on-prem telemetry stays on site unless the
  customer exports it.

### 8.2 Alerts tied to the safety rules

Operator alerts are opaque (OPS-10): an ID, a reason code, a runbook link. Owner alerts (FR-8.3)
go through the notification path with the same rule. Most alerts derive from journal events, which
are authoritative; a few come from metrics.

| Alert | Source | Severity (07) | Runbook |
|---|---|---|---|
| Reconciliation mismatch; agent paused | `ReconciliationRun` with a mismatch; `AgentModeApplied` | SEV-2 | RB-01 |
| Unprotected interval reached `max_unprotected_s` | Executor events (trading §5.4) | SEV-2 | RB-02 |
| Unattributed protective leg resting; openings held | `protection_unattributed` hold (trading §5.4) | SEV-3 | RB-02 |
| Triggered-stop watchdog fired | Watchdog `risk_exit` (trading §5.4) | SEV-2 | RB-02 |
| Kill switch activated: agent, connection, workspace | `KillSwitchActivated` | Informational; SEV-1 if automated at workspace scope | RB-03 |
| Global (managed) kill switch | Operator action | SEV-1 | RB-03 |
| Order `Unknown` beyond its lookup window | Executor events (trading §5.7) | SEV-2 | RB-04 |
| Duplicate order detected | Reconciliation | SEV-1 | RB-04 |
| Market data feed stale | Runtime observations; ingest metrics | SEV-3, SEV-2 at scale | RB-05 |
| Research model spend cap reached | `ModelInvocationRecorded` totals (DEC-120) | Informational | RB-06 |
| Model gateway errors, deadline misses, or `meter_unavailable` refusals above threshold | Gateway metrics | SEV-3 | RB-06 |
| Journal append `Unavailable` or `Ambiguous`; p99 append over 5 ms | Writer metrics | SEV-2 | RB-07 |
| Synchronous standby lost | Postgres metrics | SEV-2 | RB-07 |
| Unexpected `Fenced` | Writer outcome | SEV-2 | RB-08 |
| Journal verification failure | `VerificationRun` with `result: fail` (journal §11), and nothing else | SEV-1 | RB-09 |
| Journal verification incomplete | `VerificationRun` with `result: incomplete` (journal §11 "Incomplete"): the timestamp token cannot be proven yet (DEC-265 item 1, DEC-789) | Informational: pages no one | RB-10 |
| DDL or superuser session outside a release | Postgres audit (journal §6.1) | SEV-1 | RB-09 |
| Cold export lag over 1 minute (before live capital) | Exporter metrics (journal §6.2) | SEV-2 | RB-10 |
| Anchor or timestamping gap | `AnchorComputed` gaps (journal §10) | SEV-3 | RB-10 |
| Clock out of tolerance | `ClockToleranceExceeded` (journal §5.4) | SEV-2 | RB-11 |
| Vault lease renewal failing | Executor metrics | SEV-2 | RB-12 |
| Broker API errors, rate limits, or outage | Connector metrics | SEV-2 | RB-13 |
| Crash loop; agent left paused | Deployment manager | SEV-2 | RB-14 |
| Approval not delivered | Approval service | SEV-2 (07) | RB-15 |
| Backup failed; drill overdue or failed | Backup job metrics; the drill events E21-25 defines | SEV-2 | RB-16 |

### 8.3 SLOs (Proposed)

| SLO | Target (Proposed) | Measured from |
|---|---|---|
| Kill switch effective | Within one decision cycle (PRD §7); p99 under 2 seconds from `OwnerCommandIssued` to `KillSwitchActivated` in every stream owner | Journal |
| Journal append latency | p99 under 5 ms with the synchronous standby (journal §5.3) | Writer metrics |
| Agent managed | The agent's processes are ready for 99.9% of regular-session minutes, per month | Readiness metrics |
| Approval notification delivered | p95 under 30 seconds from `ApprovalRequested` | Approval service |
| Reconciliation freshness | Every account reconciled at each session boundary and at least every 15 minutes in session | Journal |
| Duplicate orders, orders outside the mandate | Zero. These are invariants, not SLOs: no error budget | Reconciliation, gate records |

### 8.4 On-call and incidents

- Severities and postmortems follow the [quality and release plan](../project/07-quality-and-release.md#incident-management).
  Journal verification failures are SEV-1 by journal §11.
- Until design partners, the founder is the only on-call, and the paper environment pages no one
  out of hours (Proposed). The paging tool and any rota are spend, so the founder decides.
- The Phase 2 gate needs runbooks for every FR-8.3 alert (07). The runbooks to write, as
  `docs/runbooks/RB-nn-*.md` (E21-6):

| ID | Runbook |
|---|---|
| RB-01 | Reconciliation mismatch and resuming a paused agent |
| RB-02 | Unprotected interval, unattributed leg, and triggered-stop watchdog |
| RB-03 | Kill switches at every scope, and recovering after one |
| RB-04 | `Unknown` orders and suspected duplicates |
| RB-05 | Market data feed stale |
| RB-06 | Model gateway degraded; spend cap reached |
| RB-07 | Journal unavailable; standby lost; Postgres failover |
| RB-08 | Unexpected writer fencing (two processes for one stream) |
| RB-09 | Journal integrity incident; unexpected DDL |
| RB-10 | Cold export or anchoring behind |
| RB-11 | Clock out of tolerance |
| RB-12 | Vault outage; credential rotation; connection revoked |
| RB-13 | Broker API outage or degradation |
| RB-14 | Crash loop |
| RB-15 | Approvals not delivered |
| RB-16 | Backup failure and the restore procedure (§6.3) |
| RB-17 | Region evacuation |
| RB-18 | Rolling back a release |

---

## 9. Security baseline

- **Network egress allow-lists, default deny, per process type** (the "Network" column of §3.1).
  An agent runtime has no route to a broker or the internet; an executor reaches only its
  connection's hosts; the model gateway reaches only the providers the workspace policy allows
  (HLD "Where data lives"); the workspace deployment's only link to the global control plane is
  outbound mTLS (HLD §4). In non-production environments live trading hosts are unreachable
  (OPS-5), except that the token-exchange process alone reaches Alpaca's token host for paper
  OAuth ([DEC-821](../project/decisions/DEC-821.md); §3.1).
- **No inbound ports** into a hybrid or on-prem data plane (HLD §4). In managed mode, the only
  ingress is the workspace API gateway behind the identity provider.
- **Least privilege:** per-process database roles with INSERT and SELECT on the journal; the
  migration-owner role used only by `db migrate`; per-process vault policies (§5.2); no operator
  role with DDL (journal §6.1).
- **Encryption:** TLS (rustls) on every connection; storage encrypted at rest with per-workspace
  keys, bring-your-own-key for businesses (journal §6.5).
- **Supply chain, already in CI:** `Cargo.lock` and `uv.lock` with `--locked`; cargo-deny for
  advisories, licenses, and sources; the founder-owned dependency registry
  (`docs/dependencies.md`); gitleaks per PR and over full history weekly; workflow actions pinned by
  commit SHA with `permissions: {}` and no secrets in PR jobs (ES-12, ES-14).
- **Supply chain, deferred with triggers:** SBOM, cargo-vet, and cargo-auditable at M6; signing and
  provenance at M11 (ES-14, ES-17).
- **Host hardening:** distroless images, a read-only root filesystem, no shell in production images,
  and non-root users (ES-17 at M11).
- **Threat model ([draft v0.1](../security/threat-model.md), E21-10, DEC-439):** per process type and per deployment mode, with the
  attackers `AGENTS.md` names (a careless user, a bad model, a malicious insider, a bad market
  tick), plus a compromised dependency, a compromised operator laptop, a malicious tenant in a
  shared cell, and a stolen backup. It feeds the M13 penetration test.

---

## 10. Cost model

A model with variables, not prices. The founder sets the hosting choice and budgets (DEC-79).

### 10.1 Per agent, per month

```
C_agent = C_cpu · h_run                         compute for the runtime process
        + C_mem · m_runtime · h_run
        + C_store · (e_day · b_event · 30)       journal growth, hot then cold
        + C_store · (a_day · b_artifact · 30)    prompts, responses, snapshots
        + Σ_models (n_calls · t_call · p_token)  inference, capped by cost_cap_usd_per_day · 30
```

| Variable | Meaning |
|---|---|
| `h_run` | Hours the process runs (always on, about 730) |
| `C_cpu`, `C_mem` | Price per CPU-hour and per GB-hour |
| `m_runtime` | The runtime's memory limit |
| `e_day`, `b_event` | Events per day and bytes per canonical event |
| `a_day`, `b_artifact` | Artifacts per day and bytes per artifact |
| `n_calls`, `t_call`, `p_token` | Model calls, tokens per call, price per token, per model (see `docs/specs/inference.md`) |
| `C_store` | Price per GB-month, with object lock, in two regions |

### 10.2 Per workspace and per account

```
C_workspace = C_executor · n_accounts + C_scheduler + C_gateway + C_control
            + C_pg_share + C_vault_share + C_data
            + C_backup · (WAL_day · 35 + fulls)
```

`C_data` is zero in v1 because each user's data comes through their own Alpaca account.

### 10.3 Per cell and fixed

The global control plane, a cell's Kubernetes control plane, the Postgres cluster with its
synchronous standby, the vault cluster, monitoring, staging, and on-call.

### 10.4 What drives cost

1. **Inference** is likely the largest variable cost, and it is bounded per agent by
   `cost_cap_usd_per_day` (DEC-120).
2. **Storage only grows.** Records are write-once for six years or more and cannot be deleted
   early, in two regions. Storage cost per agent rises every month an agent runs.
3. **Always-on processes:** one runtime per agent (DEC-08) is the isolation the HLD chose; many small
   retail agents share hosts, so the per-agent floor is its memory limit.
4. **The synchronous standby and the second region** roughly double the database and storage
   lines for live deployments; they are required (journal §5.3, §6.2).

Metering emits counts only (agent-hours, events, artifacts, tokens), never content (HLD §10).

---

## 11. Failure walk

Each failure, walked to its exit: what detects it, what agents do, how it ends, and who ends it.

| Failure | Detected by | What agents do | How it ends |
|---|---|---|---|
| **Process crash** | Liveness; the orchestrator | Protection rests at the broker. On restart: replay, reconcile, resume, or `paused` on anything unexplained (HLD §6 D) | Automatic; owner acknowledges with step-up if paused |
| **Node loss** | The orchestrator | The node's processes are rescheduled elsewhere; each new process fences the old epoch (§3.4) and recovers. On a single-host hybrid site, nothing runs until the host returns; protection rests at the broker | Automatic in a cluster; the customer restores the host otherwise |
| **Postgres primary failure** | Failover manager; append `Unavailable` | Appends fail, so no order is sent, exits included (OPS-2, §2.1); protection rests at the broker; writers retry or re-query by `event_id` after promotion (§4.1) | Promotion of the synchronous standby; agents resume without reconciliation gaps |
| **No synchronous standby** | Postgres metrics | Commits wait; agents hold every order, exits included (§2.1); protection rests at the broker; the owner can act directly at the broker | An operator restores a standby (RB-07); never by switching to asynchronous commit |
| **Object store outage** | Exporter and artifact-writer errors | Trading continues on the hot store (the cold store is not in the trade path). An event that needs a new artifact (a model prompt) waits for the artifact, so the research agent and LLM-informed decisions pause; quant decisions, exits, and protection continue (§4.3 ordering). Cold export lags and alerts | The store returns; the exporter catches up. Before live capital, a lag past one minute is SEV-2 |
| **Region outage (managed)** | Fleet health; synthetic probes | Agents in that region stop; protection rests at the broker; the owner can act directly at the broker | The founder or on-call decides to evacuate (RB-17): restore in the second region (§6.3), reconcile, resume only as OPS-9 allows |
| **Vault outage** | Lease renewal failures | Running executors keep their leases until expiry; new processes cannot start, so their agents stay `Recovering` | The vault returns. If a lease expires first, that executor stops sending and its agents are paused; protection rests at the broker |
| **Broker API outage** | Connector errors; `Unknown` orders | Orders in flight become `Unknown` and are looked up when the broker returns (trading §5.7); no new opening while the broker cannot confirm state; the kill switch is journaled and retried | The broker returns; reconciliation runs (trading §11), and mismatches pause |
| **Market data outage** (the vendor feed, or the host-local channel of §3.1) | Staleness checks | The gate refuses openings on stale data; risk exits use the exit price ladder's rules for missing prices (trading §5.6) | Feed returns |
| **Model gateway or provider outage** | Gateway errors; `meter_unavailable` refusals | The output counts as missing; no substitute model (DEC-67); the research agent pauses ideation; exits and the kill switch need no model. A replica that cannot reach the meter holder, or is fenced, refuses the call and sends nothing (§3.1) | Provider returns; the holder is reachable again, or a new holder has taken the epoch and folded the stream |
| **Global control plane outage** | Outbound link | Nothing changes for trading (OPS-12); hybrid approvals use fallback channels; usage reports and updates queue (HLD §4) | Link returns |
| **Clock skew** | The scheduler's offset checks (journal §5.4) | `ClockToleranceExceeded` is journaled and events are flagged; broker `event_time` stays authoritative for executions; the risk clock comes from the scheduler, not each host | Time sync returns; the next measurement within tolerance ends the flag |
| **Bad deploy** | Canary health, crash loops, alert spike | Drained processes hand over normally; a crashing new version leaves agents `Paused` after the crash-loop bound, with protection at the broker | Rollback through the same drain and hand-over (§7.4) |
| **Network partition between executor and Postgres** | Append errors | The executor cannot journal, so it sends nothing, exits included (§2.1); protection rests at the broker; if a second executor is started on the other side, it fences the first | Partition heals; the fenced process exits |
| **Journal integrity failure** | `VerificationRun` with `result: fail` | Affected agents pause; the kill switch still works; control-stream failures freeze mandate and deployment changes (journal §11) | SEV-1 incident; a new writer epoch from `IntegrityIncidentRecorded`; nothing repaired in place |
| **Credential leak suspected** | Secret scanning; vault audit; the owner | Revoke the connection (`ConnectionRevoked`); the executor stops sending for it; protection rests at the broker | Owner reconnects with a new credential; SEV-1 postmortem (07) |

---

## 12. What exists and what is planned

As of 2026-10-03. Most of this document is plan.

| Area | Exists today | Planned |
|---|---|---|
| Environments | Dev and CI. CI runs `fast` and `full` on GitHub Actions with no secrets in PR jobs, and a weekly run (DEC-256). Paper keys come from local configuration (ES-19) | The Phase 1 paper environment as a supervised set of processes; staging from M8; production after the Phase 2 gate |
| Paper/live boundary | Only paper hosts compiled in; the tracer refuses any host but Alpaca's paper host (E7-7); `environment` fixed per stream | Egress allow-lists; the live build, signed, after counsel and the penetration test |
| Processes | `mandate-runtime` (pure state machine and kill switches), `mandate-executor`, `mandate-alpaca` (paper), `mandate-shell` with the one-order `mandate-tracer` binary, whose adapters are still mostly stubs | Long-running runtime, executor, and scheduler processes; health and readiness; the deployment manager; drain and hand-over |
| Journal | `mandate-journal-pg` with `migrations/0001_journal.sql`: append protocol, fencing, append-only roles and triggers, read-time checks; CI runs Postgres tests | Synchronous standby and failover; WAL archiving; the evictor |
| Cold store | `mandate-journal-cold`: manifests, per-range checks, export, and the RFC 3161 imprint check (signature verification Proposed, DEC-265) | Shipping to object storage with object lock, the replica, retention jobs, and drills (E5-7); CLI over cold exports (E5-8) |
| Artifacts | `mandate-artifacts-fs`, the filesystem backend | Object-storage backend |
| Market data | `mandate-marketdata` writing Parquet; `mandate download` and `inspect` | Live ingest and the shared data plane (`docs/specs/data-plane.md`) |
| Secrets | Local paper keys (ES-19); `secrecy` and the log-scan rule (ES-09) | The vault, leases, permission checks, rotation, break-glass; the personal-data vault (E5-5) |
| Observability | `tracing` JSON logs with opaque IDs | Metrics exporter (M6), alerts, SLOs, runbooks |
| Release | Squash merges by label (DEC-175); cargo-deny, the dependency registry, gitleaks | Release builds, reproducibility, SBOM, signing, promotion, installer (Helm, Compose), signed bundles |
| Backups | None | WAL archiving, base backups, restore procedure, drills |
| Kubernetes, cells, global control plane | None | M8 and M11 |

---

## 13. Decisions

[DEC-434](../project/decisions/DEC-434.md) records this document's choices.

**Accepted (agent, DEC-79):** reversible engineering readings that only tighten or that add no
risk: the environment ladder and its paper-only rule (item 1), the process topology (item 2),
fencing as the only at-most-one guarantee (item 3), readiness (item 4), artifact-before-event
ordering (item 5), synchronous-only failover (item 6), drain and hand-over (item 7), rollback and
expand-only migrations (item 8), restores that never auto-resume (item 9), opaque telemetry labels
(item 10), default-deny egress (item 11), and the E21 epic (item 12). Added in v0.2: the host-local
market-data channel and the co-location it requires (item 22), one meter writer per workspace in
the model gateway (item 23), and the journal-spec-first story for backup and drill events (item 24).

**Accepted by the founder (2026-10-03):** items 13 to 20, as recommended, and open question 4.

| Item | Decision | As accepted |
|---|---|---|
| 13 | Hosting provider and regions for managed cells | Decided at M8, not now. Compliance-mode object lock, a key management service, managed Kubernetes, and two US regions are required. Phase 1 stays on the founder's host |
| 14 | Vault product | OpenBao in every mode behind a `SecretSource` interface (§5.6) |
| 15 | Managed Postgres service or self-run | Self-run with a fenced synchronous standby, unless a managed service meets item 15's conditions |
| 16 | Object storage on the edge | The customer's S3-compatible store with object lock; no bundled MinIO |
| 17 | Recovery point, recovery time, and SLO targets | §6.1 and §8.3 as written. The region-loss row takes effect with item 13's second region (§6.1) |
| 18 | Budgets | A monthly cap each for infrastructure and inference, with alerts at 50%, 80%, and 100% |
| 19 | Staging | A minimal staging cell at the start of M8 |
| 20 | On-call | The founder alone until design partners, with a paging tool chosen then |

**Proposed for the founder:** item 21, what an exit does while the journal is unavailable (§2.1).
It touches two non-negotiable rules, so DEC-79 reserves it.

| Option | What it means | Cost |
|---|---|---|
| **(a) Name the hold** (recommended) | Rule 13's list of permitted holds gains "the journal is unavailable", and the trading spec says what happens then: nothing is sent, resting protection is untouched, the hold is alerted, and owed exits go first when appends return | The exposure of §2.1 stays, disclosed to owners; rule 5 keeps no exception |
| (b) A fallback record for exits | A risk exit may be sent after its intent is written to a second durable record (a local write-ahead file), later copied into the journal | Rule 5 gains an exception and the journal a second source of truth, the class of bug DEC-16 rejected; recovery must merge two logs; a duplicate order becomes possible |
| (c) Send exits unjournaled | Rule 13 wins outright during an outage | Breaks rule 5 and the idempotency that OPS-3 rests on |

Until the founder decides, the design proceeds on the most conservative option: never send
without journaling, which is what option (a) would write down.

---

## 14. Backlog

Epic **E21 Operations and infrastructure**, proposed in DEC-434 item 12. E18 is the enterprise
harness; E19 and E20 are left to the sibling specs drafted in parallel. The rows are added to the
[backlog](../project/06-backlog-v1.md#e21-operations-and-infrastructure-proposed-dec-434).

## 15. Open questions

1. Whether a workspace's processes may later spread over nodes for blast radius. v1 co-locates
   them because the market-data channel is host-local (§3.2); spreading needs a data-plane spec
   change first.
2. Lease length for broker credentials versus how long a vault outage can last (§5.2).
3. Whether hybrid customers may keep anchors away from the global plane (journal §13 question 2),
   which changes what step 4 of the restore (§6.3) can compare against.
4. **Decided by the founder (2026-10-03, DEC-434):** an order that carries our `client_order_id`
   prefix but is unknown to a restored journal is external activity (§6.3 step 5), never adopted
   by its prefix.
5. **Now a story:** the backup and drill events are E21-25, journal spec first (§6.4).
6. What an exit does while the journal is unavailable (§2.1): the founder's, DEC-434 item 21.
