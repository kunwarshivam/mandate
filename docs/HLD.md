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

```mermaid
erDiagram
    ORGANIZATION ||--o{ WORKSPACE : contains
    ORGANIZATION ||--o{ MEMBER : "has users"
    ORGANIZATION ||--|| POLICY : "sets org limits"
    WORKSPACE ||--o{ MEMBER : "grants roles to"
    WORKSPACE ||--o{ CONNECTION : owns
    WORKSPACE ||--o{ AGENT : owns
    WORKSPACE ||--|| POLICY : "tightens"
    AGENT ||--o{ AGENT_VERSION : "versioned as"
    AGENT_VERSION ||--o{ DEPLOYMENT : "runs as"
    DEPLOYMENT }o--|| CONNECTION : "trades through"
    DEPLOYMENT ||--o{ EVENT : journals
```

| Entity | What it is |
|---|---|
| Organization | Billing, SSO configuration, org-wide policies and limits, audit |
| Workspace | **The tenant.** Members, connections, agents, data, encryption keys |
| Connection | A broker / exchange account, a reference to its stored credential, and the scopes granted to agents |
| Agent / Agent version | A versioned definition (the "spec") |
| Deployment | A running instance of one agent version, with a lifecycle |
| Event | One journaled step: observation, analysis, decision, approval, order, fill, or position change |

**Policies inherit downward and can only tighten.** An organization sets a maximum; a
workspace may lower it; an agent may lower it further. Nothing below can loosen what is above.

```mermaid
flowchart LR
    org["Organization policy<br/>sets the maximum"] -->|can only tighten| ws["Workspace policy"]
    ws -->|can only tighten| spec["Agent spec"]
    spec -->|enforced by| gate["Risk gate<br/>on every order"]
```

Every deployment journals a causal chain of events:

```mermaid
flowchart LR
    obs[Observation] --> ana[Analysis] --> dec[Decision] --> appr["Approval<br/>(if required)"] --> ord[Order] --> fill[Fill] --> pos[Position]
```

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

```mermaid
flowchart TB
    users["Users<br/>web · mobile · public API"]

    subgraph CP["Control plane: ours, multi-tenant SaaS"]
        direction LR
        gw["API gateway"]
        idp["Identity<br/>SSO · SCIM · roles"]
        tenancy["Org & workspace<br/>management"]
        registry["Agent registry<br/>+ spec compiler"]
        policy["Policy service"]
        deploy["Deployment manager"]
        approvals["Notifications<br/>& approvals"]
        billing["Billing & metering"]
        auditx["Audit explorer"]
        catalog["Connector catalog<br/>· model registry"]
        fleet["Edge fleet<br/>management"]
    end

    subgraph MC["Data plane: managed cell"]
        direction LR
        mrt["Agent runtimes<br/>one process per deployment"]
        mexec["Risk engine +<br/>execution gateway"]
        mstore[("Event store<br/>· secrets vault")]
        mmodel["Model gateway"]
    end

    subgraph EDGE["Data plane: customer edge / on-prem"]
        direction LR
        ert["Same components as a managed cell<br/>credentials and trading data stay here"]
    end

    subgraph SI["Shared intelligence plane: managed only"]
        direction LR
        md["Market data"]
        news["News & filings"]
        judg["Public-event judgments<br/>· research outputs"]
    end

    venues[("Brokers / exchanges")]

    users --> gw
    CP <-->|"deployments and policies down; health, usage, events up<br/>over mTLS connections opened outbound by the data plane"| MC
    CP <-->|"same channel; only health, usage,<br/>and optional summaries go up"| EDGE
    SI -->|shared signals| MC
    SI -.->|optional subscription| EDGE
    MC <-->|"orders / market data, fills"| venues
    EDGE <-->|"orders / market data, fills"| venues
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

```mermaid
flowchart TB
    per["Perception<br/>market data · news ·<br/>account events · timers"]
    mem["Memory<br/>positions · theses ·<br/>track records"]

    subgraph ADV["Advisors"]
        direction TB
        quant["Quant models<br/>in-process"]
        fast["Fast decision models<br/>Laya in-process · Jev via gateway"]
        llm["LLM research<br/>async, never blocks"]
    end

    dec["Decider<br/>weights by track record,<br/>sizes, proposes action<br/>+ confidence"]
    pol{"Autonomy<br/>policy"}
    esc["Escalation manager<br/>durable wait"]
    deny["Denied"]
    risk["HARD RISK GATE"]
    exe["Executor<br/>idempotency keys"]
    xgw["Execution gateway"]
    broker[("Broker /<br/>exchange")]
    journal[("Journal<br/>append-only, hash-chained")]

    per --> mem --> ADV --> dec --> pol
    pol -->|AUTO| risk
    pol -->|ASK| esc
    pol -->|DENY| deny
    esc -->|"approved, or<br/>safe default"| risk
    risk --> exe --> xgw --> broker

    dec -.->|record| journal
    esc -.->|record| journal
    deny -.->|record| journal
    exe -.->|"record intent<br/>BEFORE sending"| journal
```

| Component | Responsibility |
|---|---|
| Perception | Subscribes to market data, news, account events, and timers; fills and position updates from the broker arrive here, closing the loop |
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

### Agent lifecycle

```mermaid
stateDiagram-v2
    [*] --> Draft: spec compiled and reviewed
    Draft --> Backtest: validate against org and workspace limits
    Backtest --> Paper: backtest passes
    Backtest --> Draft: revise spec
    Paper --> Live: paper run passes and owner approves
    Paper --> Draft: revise spec
    Live --> Paused: owner, risk breaker, or anomaly
    Paused --> Live: owner resumes
    Live --> Recovering: crash or restart
    Recovering --> Live: journal replayed and broker reconciled
    Recovering --> Paused: unexplained orders or positions
    Live --> Completed: goal met or end date reached
    Live --> Stopped: owner stops or kill switch
    Paused --> Stopped: owner stops
    Completed --> [*]
    Stopped --> [*]
```

Waiting for an approval is not a separate lifecycle state: a live agent can have pending
approvals for some actions while it keeps managing everything else.

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

```mermaid
flowchart TD
    sso["Sign in via SSO"] --> ws["Create or join org, then workspace"]
    ws --> connect["Connect broker / exchange account"]
    connect --> check{"Key allows<br/>withdrawals?"}
    check -->|yes| reject["Reject key,<br/>ask for a trade-only key"]
    reject --> connect
    check -->|no| describe["Describe agent in plain language"]
    describe --> compile["LLM compiles spec"]
    compile --> review{"Owner reviews spec;<br/>within org and<br/>workspace limits?"}
    review -->|no| describe
    review -->|yes| bt["Backtest"]
    bt --> paper["Paper trading run"]
    paper --> approve{"Owner approves<br/>going live?"}
    approve -->|no| describe
    approve -->|yes| deploy["Deployment manager schedules runtime<br/>in a managed cell or on the edge"]
```

### B. Decision cycle

1. An event arrives: price move, news, fill, or timer.
2. Advisors produce opinions.
3. The decider proposes an action with a confidence level.
4. The autonomy policy classifies it as AUTO, ASK, or DENY.
5. The risk gate checks it.
6. The executor places the order. Every step is journaled with links to its causes.

```mermaid
sequenceDiagram
    autonumber
    participant S as Event source
    participant A as Advisors
    participant D as Decider
    participant P as Autonomy policy
    participant R as Risk gate
    participant E as Executor
    participant B as Broker
    participant J as Journal

    S->>A: Price move, news, fill, or timer
    A->>D: Opinions (signal, conviction, horizon, thesis)
    D->>J: Record decision and its inputs
    D->>P: Proposed action + confidence
    alt AUTO
        P->>R: Check against hard limits
        R->>E: Approved
        E->>J: Record order intent with idempotency key
        E->>B: Place order
        B-->>E: Acknowledgement and fills
        E->>J: Record fills
    else ASK
        P->>J: Record approval request (see flow C)
    else DENY
        P->>J: Record denial and reason
    end
```

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

```mermaid
sequenceDiagram
    autonumber
    participant A as Agent runtime
    participant J as Journal
    participant N as Notification service
    participant U as Approver
    participant R as Risk gate
    participant B as Broker

    A->>A: Confidence below threshold, or rule requires approval
    A->>J: Record approval request (action, alternatives, evidence, risk impact, deadline, default)
    A->>N: Request approval
    N->>U: Push notification
    opt No response at this escalation step
        N->>U: SMS, then phone call (respecting quiet hours)
    end
    Note over A: Keeps managing other positions.<br/>Risk-reducing actions stay automatic.
    alt Approver responds before the deadline
        U->>N: Approve or reject (step-up auth for high-risk actions)
        N->>A: Response
        A->>J: Record who, when, channel, and auth method
        A->>A: Re-validate price and risk drift since the request
        alt Approved and drift within tolerance
            A->>R: Submit action
            R->>B: Place order
            B-->>A: Fills
            A->>J: Record order and fills
        else Rejected
            A->>J: Record rejection
        else Drift beyond tolerance
            A->>N: Re-ask, or apply the safe default
        end
    else Deadline passes
        A->>J: Record timeout
        A->>A: Apply the safe default
    end
```

### D. Crash recovery

The process restarts, replays its journal, reconciles with the broker, and resumes. Any
orders or positions it cannot account for trigger an alert and pause the agent.

```mermaid
flowchart TD
    crash["Process crashes or restarts"] --> replay["Replay journal to rebuild state:<br/>positions, theses, open intents, pending approvals"]
    replay --> reconcile["Reconcile with broker:<br/>actual positions and open orders"]
    reconcile --> match{"Everything<br/>accounted for?"}
    match -->|yes| resume["Resume trading;<br/>durable timers and pending approvals continue"]
    match -->|no| pause["Pause agent and alert owner"]
    pause --> review["Owner reviews in the audit explorer"]
```

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
