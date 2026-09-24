# High-Level Design: Autonomous Trading Agents Platform

| | |
|---|---|
| **Status** | Draft v0.1 |
| **Last updated** | 2026-09-24 |
| **Scope** | Production service for deploying autonomous trading agents, managed or on customer edge / on-prem |

## Contents

1. [Summary](#1-summary)
2. [Requirements](#2-requirements)
3. [Core concepts](#3-core-concepts)
4. [Architecture](#4-architecture)
5. [Agent runtime](#5-agent-runtime)
6. [Key flows](#6-key-flows)
7. [Logging and audit](#7-logging-and-audit)
8. [Multi-tenancy and security](#8-multi-tenancy-and-security)
9. [Intelligence layer](#9-intelligence-layer)
10. [Billing](#10-billing)
11. [Technology](#11-technology)
12. [Risks and open decisions](#12-risks-and-open-decisions)

---

## 1. Summary

Users connect their brokerage or exchange accounts, grant scoped permissions, and create
**autonomous agents** by describing a goal and behavior. Each agent runs continuously,
works toward its goal, and makes trading decisions on its own. When it is unsure, or when a
decision exceeds what it is allowed to do alone, it notifies the user (push, SMS, email,
chat), waits with a deadline, and then acts on the response, or on a safe default if nobody
answers. Every observation, analysis, decision, approval, order, and fill is recorded in a
tamper-evident log.

The platform is multi-tenant (**Organization → Workspace → Agents**), supports SSO, bills per
organization, and runs either on our managed infrastructure or on the customer's own
edge / on-prem environment. It serves both retail users (managed) and businesses
(managed or on-prem).

Design principles:

- **The agent spec is the contract.** An agent can never act outside the goal, universe,
  risk limits, and autonomy rules its owner approved.
- **Reducing risk never needs approval; increasing risk beyond agreed limits always does.**
- **Safe defaults everywhere.** Timeouts, outages, and ambiguity resolve to "don't add risk".
- **Record before acting.** Intent is journaled before any order leaves the system.
- **One runtime, many deployment modes.** The same data-plane software runs managed or on the edge.

---

## 2. Requirements

### Functional

- **Tenancy:** Organization → Workspace → Agents. Billing is per organization; the workspace is the tenant.
- **Identity:** SSO via SAML / OIDC and SCIM provisioning for businesses; email, passkey, or social login for retail.
- **Accounts:** connect broker / exchange accounts and grant scoped permissions to specific agents.
- **Agent definition:** goal, behavior, instruments, risk limits, autonomy rules (what it may do alone vs. must ask), lifetime, notification preferences.
- **Deployment:** many agents per workspace, deployable at any time; pause, resume, stop, clone, and version.
- **Lifetime:** run forever, until a date, or until the goal is met.
- **Escalation:** when unsure, notify the user, wait with a deadline, then execute the response or the safe default.
- **Audit:** everything recorded, queryable, and replayable.
- **Deployment modes:** managed SaaS, customer edge / on-prem, or hybrid.

### Non-functional

- **Safety:** hard risk gates that agent logic cannot bypass; kill switches at every level; fail-safe defaults.
- **Latency tiers:** microseconds on the execution path; 30–300 ms for fast decisions; seconds to minutes for research, which runs asynchronously and never blocks trading.
- **Durability:** agents run indefinitely and survive crashes, restarts, and upgrades, with no duplicate orders.
- **Isolation:** tenant data, secrets, and processes separated; no noisy neighbors.
- **Auditability:** tamper-evident logs, configurable retention, export.

---

## 3. Core concepts

```
Organization   billing, SSO config, org-wide policies and limits, audit
 └─ Workspace  THE TENANT: members, connections, agents, data, encryption keys
     ├─ Connection      broker/exchange account + credential reference + granted scopes
     ├─ Agent           versioned definition (the "spec")
     │   └─ Deployment  running instance of one agent version, with a lifecycle
     │       └─ Events  observation → analysis → decision → approval → order → fill → position
     └─ Policy          risk limits + autonomy rules
```

**Policies inherit downward and can only tighten.** An organization sets a maximum; a
workspace may lower it; an agent may lower it further. Nothing below can loosen what is above.

### The agent spec

Users describe an agent in plain language or through a form. An LLM **compiles** the
description into a structured spec, which the user reviews and approves. The spec is the
contract: the agent cannot act outside it.

```yaml
agent: btc-accumulator
goal:
  objective: "Accumulate 2 BTC at an average price below $58k"
  done_when: position >= 2 BTC            # or: forever | a date | a return target
universe: [BTC-PERP, BTC-SPOT]
connection: conn_bybit_testnet
behavior:
  description: "Buy dips, avoid trading 30 min around major macro news"
  advisors: [quant.mean_reversion, fast.news_materiality, llm.research]
  cadence: event-driven + every 15 min
risk:   { max_position_usd: 150000, max_leverage: 1, max_daily_loss_pct: 2, max_drawdown_pct: 8 }
autonomy:
  auto: [reduce_risk, orders_below_usd: 5000]
  ask:  [orders_above_usd: 5000, confidence_below: 0.65, unusual_market_input]
  on_timeout: { after: 10m, default: skip }   # the default is always the safe option
notify: { approvers: [role:approver], channels: [push, sms, email], quiet_hours: "23:00-07:00" }
```

Goal types the spec supports:

- **Accumulate / distribute** a position within price and time constraints.
- **Return target** under risk limits (e.g. target return with a maximum drawdown).
- **Mandate** (hedge, rebalance, or maintain an exposure).
- **Event-driven** (act on defined event classes such as earnings, filings, or macro releases).

---

## 4. Architecture

The system is split into three planes.

```
                ┌──────────────── CONTROL PLANE (ours, multi-tenant SaaS) ────────────────┐
 Web / Mobile ─►│ API gateway · Identity (SSO, SCIM, roles) · Org & workspace management  │
 Public API   ─►│ Agent registry + spec compiler · Policy service · Deployment manager    │
                │ Notifications & approvals · Billing & metering · Audit explorer         │
                │ Connector catalog · Model registry · Edge fleet management              │
                └──────────▲──────────────────────────────────────▲───────────────────────┘
                           │ mutual TLS, outbound connections only │
     ┌─────────────────────┴───────────────┐       ┌───────────────┴───────────────────────┐
     │ DATA PLANE: managed "cell"           │       │ DATA PLANE: customer edge / on-prem   │
     │  per workspace: agent runtimes,      │       │  identical components; credentials    │
     │  risk engine, execution gateway,     │       │  and trading data never leave the     │
     │  event store, secrets vault,         │       │  customer; only health, usage, and    │
     │  model gateway                       │       │  optional summaries go upstream       │
     └──────┬────────────────────▲─────────┘       └───────────────────────────────────────┘
            │ orders              │ optional shared signals
     Brokers / exchanges   SHARED INTELLIGENCE PLANE (managed only): market data,
                           news and filings, judgments on public events, research outputs
```

### Control plane

Identity, permissions, agent definitions, deployments, approvals, billing, and the audit
explorer. It never sits in the path of a trade.

### Data plane

Where agents run and trade. The same software runs in both modes:

- **Managed:** organized into **cells**, each hosting many workspaces. Cells bound the blast
  radius of failures and allow data residency by region. Large customers get a dedicated cell.
- **Edge / on-prem:** the customer's data plane makes **outbound-only** connections to the
  control plane over mutual TLS; no inbound ports are opened. Credentials and trading data stay
  on the customer's side. A fully **air-gapped** variant, including an on-prem control plane,
  is available under an enterprise license.

### Shared intelligence plane

Market data ingestion and judgments about public events (for example, "is this filing
material to this company?") are computed once and fanned out to all subscribed workspaces.
This is the main cost lever for the managed offering. Edge deployments can subscribe to it
or run their own.

---

## 5. Agent runtime

Written in Rust. One process per agent deployment.

```
Perception ─► Memory ─► Advisors ─────────────────────────► Decider ─► Autonomy policy
(market data,  (positions, (quant: in-process            (weights advisors  ├─ AUTO ─┐
 news, account  theses,     fast models: Laya in-process   by track record,   ├─ ASK ──► Escalation manager
 events,        track       or Jev via the model gateway   sizes positions,   └─ DENY    (durable wait) ─┐
 timers)        records)    LLM: asynchronous, never       proposes action               │               │
                            blocks)                        + confidence)                 ▼               ▼
                                                                          HARD RISK GATE ─► Executor ─► Execution gateway ─► Broker
Every step ─► Journal: written BEFORE acting, hash-chained, each event linked to its causes
```

| Component | Responsibility |
|---|---|
| Perception | Subscribes to market data, news, account events, and timers |
| Memory | Positions, theses (why each position exists and what would invalidate it), advisor track records, lessons |
| Advisors | Produce opinions in a common signal format: quant models, fast decision models, LLM research |
| Decider | Weights advisors by earned track record, sizes positions from calibrated confidence, proposes an action |
| Autonomy policy | Classifies each proposed action as AUTO, ASK, or DENY according to the spec |
| Escalation manager | Creates approval requests, waits durably, applies responses or safe defaults |
| Risk gate | Independent code on the execution path that enforces the policy hierarchy |
| Executor | Turns decisions into orders with idempotency keys; reconciles with the broker |
| Journal | Append-only, hash-chained record of every step |

### Isolation and custom logic

- **One process per deployment**, with OS-level CPU and memory limits. This gives real
  isolation and independent lifecycles, and many small retail agents can still share a host.
- **User-supplied logic**, if enabled, runs as **WebAssembly plug-ins** (sandboxed,
  deterministic, language-neutral). Heavier workloads use Firecracker micro-VMs.

### Durability

- **Write-ahead intent:** every order intent is journaled before it is sent, with an
  idempotency key, so a retry after a crash can never double-trade.
- **Event-sourced state:** on restart, the runtime replays its journal, reconciles against the
  broker's actual positions and open orders, and resumes.
- **Durable timers:** long waits (an approval pending for hours, "run until December") use a
  durable-execution engine. Restate is preferred because it ships as a single Rust binary that
  also runs on the edge; Temporal is the established alternative.
- **Venue-side protection:** protective stop orders rest at the exchange, so positions stay
  protected even if the entire platform is down.

---

## 6. Key flows

### A. Onboarding to first deployment

1. User signs in via SSO and creates or joins an organization, then a workspace.
2. User connects an account. At connection time the platform **queries the key's permissions
   from the exchange and rejects any key that allows withdrawals**.
3. User creates an agent: plain-language description → compiled spec → review. Risk and
   autonomy rules are validated against organization and workspace limits.
4. A **backtest and a paper-trading run are required** before the agent may trade live.
5. User deploys; the deployment manager schedules the runtime in the right cell or on the customer's edge.

### B. Decision cycle

1. An event arrives: price move, news, fill, or timer.
2. Advisors produce opinions.
3. The decider proposes an action with a confidence level.
4. The autonomy policy classifies it as AUTO, ASK, or DENY.
5. The risk gate checks it.
6. The executor places the order. Every step is journaled with links to its causes.

### C. Escalation: unsure → ask → wait → act

1. **Triggers:** confidence below threshold, a rule that requires approval, an unusual market
   input (distribution drift), or a trade too large to take alone.
2. The runtime creates an **approval request** containing the proposed action, alternatives,
   supporting evidence, risk impact, deadline, and the default applied on timeout.
3. The agent keeps managing everything else while it waits. **Risk-reducing actions stay automatic.**
4. The notification service fans out according to user preferences and an escalation chain
   (push → SMS → phone call), respecting quiet hours. Large actions can require **two approvers**.
5. Users respond through the app, an SMS reply, or a signed, short-lived email link.
   **High-risk approvals require step-up authentication** (passkey or biometrics).
6. Before executing, the runtime **re-validates** that price and risk have not drifted beyond a
   tolerance since the request was created. If they have, it re-asks or applies the default.
7. On timeout, the safe default is applied.
8. Every step is journaled: who approved, when, through which channel, and how they authenticated.

### D. Crash recovery

The process restarts, replays its journal, reconciles with the broker, and resumes. Any
orders or positions it cannot account for trigger an alert and pause the agent.

---

## 7. Logging and audit

- **Per-workspace event store:** append-only and hash-chained. Each event links to its causes,
  so "why did this order happen?" is a single query from the fill back to the observations
  that triggered it.
- **Large artifacts** (LLM prompts and responses, data snapshots) live in object storage,
  content-addressed by hash; events store the hash.
- **Storage tiers:**
  - Hot (queryable): Postgres, moving to ClickHouse at scale.
  - Cold (retention): Parquet on object storage with write-once (object lock) retention.
  - Anchoring: the chain's root hash is periodically published externally so tampering is detectable.
- **Export** to the customer's SIEM or storage bucket; retention configured per organization.
- **Privacy:** personal data is encrypted with per-user keys. Deleting a key erases the person
  without breaking the immutable log (crypto-shredding).

---

## 8. Multi-tenancy and security

| Concern | Design |
|---|---|
| Tenant boundary | The workspace. Every record carries a workspace ID; row-level security in the database; per-workspace encryption keys; bring-your-own-key for businesses |
| Isolation tiers | Retail: shared cells with logical isolation. Business: dedicated database or dedicated cell. Edge: physically separate |
| Processes | Agent processes are never shared across workspaces |
| Messaging | NATS JetStream; its account model maps to workspaces, and it runs as a single binary on the edge |
| Roles | Org owner, org admin, billing admin, workspace admin, operator, **approver**, viewer, auditor. Optional separation of duties: an agent's creator cannot be its sole approver for large actions |
| Agent identity | Each agent has its own service identity and scoped tokens; every action is attributed to a specific agent |
| Credentials | Managed: vault backed by a key management service. Edge: local vault; **credentials never leave the customer's environment** |
| Step-up authentication | Required to connect accounts, raise limits, approve large trades, or go live |
| Quotas | Per workspace: agents, model spend, API rate, data subscriptions |

**Kill switches at every level:** agent, connection, workspace, organization, and a global
switch for the managed service. Edge customers control their own; the managed global switch
cannot reach into customer deployments.

---

## 9. Intelligence layer

- **Model gateway:** routes calls to Jev (hosted decision model), a Laya pool or in-process
  Laya (open-weight decision model), and LLM providers. Handles deadlines, fallbacks, and
  caching, and meters cost per workspace and per agent. Edge deployments can be restricted to
  **local models only**.
- **Calibration service:** recalibrates each model's confidence against realized outcomes, per
  agent. Calibrated confidence is what determines whether an agent is "unsure".
- **Market data service:** normalized live streams plus a point-in-time historical store.
  Managed deployments ingest once and share; edge deployments connect directly to venues.

Speed tiers:

| Tier | Latency | What runs there |
|---|---|---|
| Hot | microseconds | Market data handling, risk gate, order management, kill switch |
| Fast | 30–300 ms | Decision models (Laya, Jev) answering typed questions with probabilities |
| Slow | seconds–minutes | LLM research: reading filings and news, writing theses; never blocks trading |

---

## 10. Billing

- **Billed entity:** the organization.
- **Components:** plan (seats and agents), usage (agent-hours, decisions, model tokens at cost
  plus margin, data), and edge licenses.
- **Pipeline:** data planes emit usage events → metering pipeline → billing provider
  (Stripe Billing, Orb, or Metronome). Edge deployments send signed usage reports; air-gapped
  deployments use license tiers.
- **Pricing constraint:** never charge per trade or as a percentage of assets. This keeps the
  platform a software business and away from broker-dealer and investment-adviser models.

---

## 11. Technology

| Layer | Choice |
|---|---|
| Agent runtime, risk engine, execution gateway, connectors, market data, journal writer | Rust |
| Research, model training and calibration, LLM tooling, Python SDK | Python (PyO3 bindings, gRPC) |
| Web app / mobile approvals | Next.js / React Native with push notifications |
| Storage | Postgres, object storage (S3, or MinIO on the edge), ClickHouse at scale |
| Messaging / durable execution | NATS JetStream / Restate (or Temporal) |
| Orchestration | Kubernetes cells (managed); Helm chart or single-node k3s (edge) |
| Sandboxing | WebAssembly plug-ins; Firecracker for heavier workloads |
| Observability | OpenTelemetry; agent traces double as part of the audit record |

---

## 12. Risks and open decisions

1. **Managed retail regulation.** Agents trading retail users' accounts is the riskiest
   combination, even when users define the agents. Obtain a legal review before launching
   retail. Business customers on edge are the safest starting point.
2. **NautilusTrader licensing.** It is LGPL-3.0. Distributing it inside on-prem software,
   particularly statically linked Rust, carries relinking obligations. Decide whether to use
   its connectors or write our own.
3. **Market data redistribution.** Passing equities exchange data to tenants requires vendor
   and exchange licenses. Public crypto data is simpler.
4. **Custom code in v1.** Declarative specs only, or also WebAssembly plug-ins?
5. **First connectors.** Crypto exchange testnets first, then Interactive Brokers, then an
   Alpaca connector we would write ourselves.
6. **Approval channels in v1.** A native mobile app for push, or SMS, email, and Slack / Telegram first.
