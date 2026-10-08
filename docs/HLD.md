# Mandate: High-Level Design

| | |
|---|---|
| **Status** | Draft v0.2 |
| **Last updated** | 2026-09-24 |
| **Scope** | Production service for deploying autonomous trading agents: fully managed, hybrid, or fully on-prem / air-gapped |

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
organization, and runs in three modes: fully managed on our infrastructure, hybrid (a thin
hosted control plane with everything sensitive on the customer's side), or fully on-prem /
air-gapped. It serves both retail users (managed) and businesses (any mode).

Design principles:

- **The agent spec is the contract.** An agent can never act outside the goal, universe,
  risk limits, and autonomy rules its owner approved.
- **Reducing risk never needs approval; increasing risk beyond agreed limits always does.**
- **Safe defaults everywhere.** Timeouts, outages, and ambiguity resolve to "don't add risk".
- **Record before acting.** Intent is journaled before any order leaves the system.
- **One installer, three deployment modes.** The same software runs managed, hybrid, or fully
  on-prem. In hybrid and on-prem modes, strategy, approvals, audit data, and credentials never
  leave the customer's site.

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
- **Deployment modes:** fully managed; hybrid (thin hosted control plane, everything sensitive on the customer's side); fully on-prem / air-gapped.

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
description into a structured spec: it extracts the values the user stated and may propose the
rest of the envelope (capital, goal, limits, autonomy rules, asset classes), each marked as
proposed; the user confirms every envelope field. The working universe and its theses are
produced by the research agent at runtime within that envelope
([DEC-97](project/04-decision-log.md#decisions), [ADR-0002](adr/0002-autonomous-ideation-and-retail.md)).
The spec is binding: the agent cannot act outside it. The exact format is the [mandate spec](specs/mandate.md); an abbreviated example:

```yaml
name: btc-accumulator
environment: paper
connection_id: conn_alpaca_paper_01
capital: { allocation_usd: "10000", max_loss_from_allocation: "0.1" }
goal: { type: accumulate, instrument: BTC/USD, target_qty: "0.15", max_avg_price: "58000",
        max_spend_usd: "9000", end_date: "2026-12-31" }   # or: continuous | profit_stop
universe: { instruments: [BTC/USD], leveraged_etps_enabled: false }
behavior:
  description: "Buy dips; avoid trading 30 minutes around major macro releases."
  signal_models: [{ id: quant.mean_reversion, version: 1.0.0, weight: "1", params: [...] }]
  cadence: { interval_s: 900, event_sources: [news, price] }
  sizing: { method: conviction_linear, entry_threshold: "0.3", exit_threshold: "0.3", rebalance_band: "0.05" }
protection: { enabled: true, stop_distance: "0.08", crypto_stop_limit_offset: "0.005" }
risk: { max_position_usd: "10000", max_order_usd: "1000", max_daily_loss: "0.02", max_drawdown: "0.08",
        drawdown_ladder: [...], breach_confirm_s: 60, reentry_cooldown_s: 3600, ... }
autonomy:
  rules:                                   # first match wins; risk reduction is always automatic
    - { id: large_orders, when: { field: order_usd, op: gt, value: "900" }, then: ask }
    - { id: low_score, when: { field: combined_score, op: lt, value: "0.65" }, then: ask }
    - { id: routine, when: { field: purpose, op: in, value: [increase, open] }, then: auto }
  default: ask
  approval: { timeout_s: 600, on_timeout: skip, approvers: [role:approver] }   # timeout always skips
notifications: { channels: [email, web_push], quiet_hours: { start: "23:00", end: "07:00" } }
```

Goal types the spec supports:

- **Continuous:** trade the universe until stopped or an end date.
- **Accumulate** a position within quantity, spend, average-price, and time constraints.
- **Profit stop:** trade the universe and stop, flat, when agent return reaches a user-set level
  (a stopping level, not a target).
- Later: maintain an exposure (hedge, rebalance) and event-driven goals.

---

## 4. Architecture

The control plane is split in two. A **thin global control plane** holds only non-sensitive
metadata. Everything that reveals strategy or trading intent lives in the **workspace
deployment**, next to the agents, wherever that deployment runs.

```mermaid
flowchart TB
    subgraph GCP["Global control plane: thin, metadata only"]
        direction LR
        dir["Org directory<br/>& identity federation"]
        lic["Licensing, billing<br/>& metering"]
        fleet["Fleet health<br/>& update distribution"]
        relay["Notification relay<br/>opaque IDs only"]
        cat["Connector catalog<br/>· model registry"]
    end

    subgraph SITE["Workspace deployment: managed cell, or customer edge / on-prem"]
        direction TB
        subgraph WCS["Workspace control services"]
            direction LR
            spec["Agent registry<br/>+ spec compiler"]
            pol["Policy service"]
            dep["Deployment manager"]
            appr["Approval service<br/>holds approval content"]
            audx["Audit explorer<br/>backend"]
            conn["Connection manager"]
        end
        subgraph DP["Data plane"]
            direction LR
            rt["Agent runtimes<br/>one process per deployment"]
            exec["Risk engine +<br/>execution gateway"]
            store[("Journal · state ·<br/>secrets vault")]
            mg["Model gateway"]
        end
        WCS --> DP
    end

    users["Users<br/>web · mobile · public API"]
    idp["Customer identity provider"]
    phone["Approver's phone"]
    venues[("Brokers / exchanges")]
    SI["Shared data plane<br/>managed, optional"]

    users --> WCS
    WCS -->|"SSO"| idp
    SITE -->|"outbound mTLS: health,<br/>usage counts, version"| GCP
    GCP -->|"signed updates,<br/>licenses"| SITE
    appr -->|"'approval needed'<br/>+ opaque ID"| relay
    relay -->|push| phone
    phone <-->|"fetch details and respond,<br/>end-to-end encrypted"| appr
    DP <-->|"orders / market data, fills"| venues
    SI -.->|shared data| DP
```

### Global control plane (thin)

Holds only what is safe to keep outside the customer's site:

- **Org directory and identity federation:** organizations, workspace IDs, user IDs, and roles,
  for seat counts and routing. Authentication itself happens against the customer's identity
  provider, directly from the workspace deployment.
- **Licensing, billing, and metering:** usage counts, never content.
- **Fleet health and update distribution:** versions, heartbeats, signed release bundles.
- **Notification relay:** delivers push notifications to phones through Apple's and Google's
  push services. Payloads carry only an opaque ID and generic text.
- **Connector catalog and model registry:** downloadable connectors and model artifacts.

It never sits in the path of a trade, and it never holds strategy, approval content, audit
data, positions, or credentials.

### Workspace deployment

Workspace control services and the data plane always run together:

- **Workspace control services:** agent registry and spec compiler, policy service,
  deployment manager, approval service, audit explorer backend, connection manager.
- **Data plane:** agent runtimes, risk engine, execution gateway, journal and state, secrets
  vault, model gateway.

In managed mode, workspace deployments are grouped into **cells**, each hosting many
workspaces. Cells bound the blast radius of failures and allow data residency by region.
Large customers get a dedicated cell. On the customer's side, the deployment makes
**outbound-only** connections to the global control plane over mutual TLS; no inbound ports
are opened.

### Deployment modes

| Component | Managed | Hybrid | Fully on-prem / air-gapped |
|---|---|---|---|
| Global control plane | Ours | Ours | Customer's (same software) |
| Workspace control services | Ours | Customer's | Customer's |
| Data plane | Ours | Customer's | Customer's |
| Shared data | Ours | Optional subscription | Optional offline feed, or none |
| Typical customer | Retail, small teams | Most businesses | Banks, funds with strict policies |

What changes in fully on-prem / air-gapped mode:

- **Identity:** SSO directly against the customer's identity provider.
- **Notifications:** through the customer's own channels (mail server, Teams or Slack, their
  SMS gateway), or push through our relay if they allow that single outbound connection.
- **Billing:** a signed license file plus periodic usage reports delivered offline.
- **Updates:** signed release bundles the customer installs.

**Engineering rule:** every component ships from the same installer and runs in our cloud or
theirs. No hosted-only dependency is allowed in a core path, so fully on-prem is a packaging
choice, not a separate product.

### Where data lives

| Data | Sensitivity | Managed | Hybrid | Fully on-prem |
|---|---|---|---|---|
| Org directory: org, workspace, and user IDs, roles | Low | Ours | Ours (IDs only) | Customer's |
| Usage counts, versions, heartbeats | Low | Ours | Ours | Customer's; offline reports |
| Agent specs: goals, instruments, limits, behavior | **High (strategy)** | Ours | Customer's | Customer's |
| Approval requests and responses | **High (trading intent)** | Ours | Customer's | Customer's |
| Journal, audit data, positions, orders, fills | **High** | Ours | Customer's | Customer's |
| Model prompts and outputs | **High** | Ours | Customer's | Customer's |
| Broker / exchange credentials | **Critical** | Our vault | Customer's vault | Customer's vault |
| Notification payloads through the relay | Low (opaque IDs) | Ours | Ours | Customer's, or our relay if allowed |

External model calls (Jev, hosted LLMs) send prompt content to those providers. Each
workspace's policy decides which providers are allowed; hybrid and on-prem deployments can
be restricted to local models only.

### Behavior when the global control plane is unavailable

- **Trading continues** in every mode; the global control plane is never in the trade path.
- **Approvals continue** in hybrid mode through the customer-side channels configured as
  fallbacks (email via their mail server, Teams or Slack); only relay-delivered push is lost.
  Approvers authenticate against the customer's identity provider, not against us.
- **Deferred:** usage reports and updates queue until the connection returns.

### Shared data plane

Public data (SEC filings and XBRL facts, calendars, reference lists whose terms allow sharing) and
its factual classification (filing type, entity tagging) are computed once and published as whole
datasets that workspaces pull. It carries **no exchange market data and no licensed vendor text in
v1**: each workspace reads market data and news through its own broker connection (§12 risk 3;
[DEC-433](project/decisions/DEC-433.md) items 3, 9, and 15). It emits no directional views: any
directional output is a signal model the user selects and pins
([mandate spec §8.1](specs/mandate.md#81-signal-model-contract-dec-52-dec-97);
[DEC-62](project/04-decision-log.md#decisions)). This is the main cost lever for the managed
offering. Hybrid and on-prem deployments can subscribe to it or run their own. The
[data plane spec](specs/data-plane.md) (draft) defines it, the workspace data service, and the
point-in-time store.

---

## 5. Agent runtime

Written in Rust. One process per agent deployment.

```mermaid
flowchart TB
    per["Perception<br/>market data · news ·<br/>account events · timers"]
    mem["Memory<br/>positions · theses ·<br/>track records"]
    res["Research agent<br/>ideas → theses →<br/>universe admission"]

    subgraph ADV["Signal models"]
        direction TB
        quant["Quant models<br/>in-process"]
        fast["Fast decision models<br/>Laya in-process · Jev via gateway"]
        llm["LLM research<br/>async, never blocks"]
    end

    dec["Order builder<br/>user-selected sizing,<br/>clips to limits, proposes<br/>action + combined score"]
    pol{"Autonomy<br/>policy"}
    esc["Escalation manager<br/>durable wait"]
    deny["Denied"]
    risk["HARD RISK GATE"]
    exe["Executor<br/>idempotency keys"]
    xgw["Execution gateway"]
    broker[("Broker /<br/>exchange")]
    journal[("Journal<br/>append-only, hash-chained")]

    per --> mem --> res --> ADV --> dec --> pol
    res -.->|record theses,<br/>admissions| journal
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
| Memory | Positions, theses (why each position exists and what would invalidate it), and signal-model track records, kept for the user's review and fed to the research agent ([DEC-97](project/04-decision-log.md#decisions)) |
| Research agent | LLM-driven ideation, asynchronous and never blocking trading: ingests market data, news, filings, screens, and memory; proposes theses (instrument, direction, horizon, evidence, invalidation) journaled as `ThesisProposed`; admits instruments into the working universe through the eligibility floor, `max_instruments`, instrument-group claims, and the autonomy rules (`UniverseChanged`); its theses are its signal-model outputs to the order builder, combined at the user-confirmed weight with the other configured models. Disabled in bring-your-own-strategy mode ([ADR-0002](adr/0002-autonomous-ideation-and-retail.md)). May revise a thesis that failed on forward paper as a new version in its lineage (`ThesisRevised`), scored from zero and capped per lineage, only after the forward-paper evaluator exists ([DEC-111](project/04-decision-log.md#decisions)) |
| Signal models | Produce outputs in a common format (conviction, confidence, horizon, thesis): quant models, fast decision models, LLM research. They never place orders |
| Order builder | Combines model outputs with the user's fixed weights, sizes with the user-selected method, clips to limits, and proposes an action with a combined score |
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
- **Durable timers:** long waits (an approval pending for hours, "run until December") are
  journal-backed: a timer is a journaled event, read back behind the `TimerSource` trait, and it
  fires into the runtime as a journaled input. There is no external durable-execution engine, so
  the journal stays the single source of truth ([DEC-16](project/04-decision-log.md#decisions)).
- **Venue-side protection:** protective exits rest at the broker as OCO or bracket orders, so
  positions keep protection if the platform is down, within limits: equity stops trigger only in
  the regular session, crypto stop-limits can miss on gaps, and exits briefly remove protection
  while they run ([trading domain spec §5.4](specs/trading-domain.md#54-protective-exits-dec-28-dec-36)).

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
2. User connects an account. **The platform never holds permissions that can move funds:**
   OAuth connections (Alpaca) request trading and account-read scopes only; for API-key
   connections (Kraken Derivatives US), the platform queries the key's permissions from the venue
   and rejects any key that allows withdrawals or transfers.
3. User creates an agent: plain-language description → compiled spec → review. Risk and
   autonomy rules are validated against organization and workspace limits.
4. A **backtest and a paper-trading run are required** before the agent may trade live.
5. User deploys; the deployment manager in the workspace deployment starts the runtime, in a managed cell or on the customer's site.

```mermaid
flowchart TD
    sso["Sign in via SSO"] --> ws["Create or join org, then workspace"]
    ws --> connect["Connect account<br/>Alpaca via OAuth, or venue API key"]
    connect --> check{"Access can<br/>move funds?"}
    check -->|yes| reject["Reject; request<br/>trading-only access"]
    reject --> connect
    check -->|no| describe["Describe agent in plain language"]
    describe --> compile["LLM compiles spec"]
    compile --> review{"Owner reviews spec;<br/>within org and<br/>workspace limits?"}
    review -->|no| describe
    review -->|yes| bt["Backtest"]
    bt --> paper["Paper trading run"]
    paper --> approve{"Owner approves<br/>going live?"}
    approve -->|no| describe
    approve -->|yes| deploy["Deployment manager starts runtime<br/>in a managed cell or on the customer's site"]
```

### B. Decision cycle

1. An event arrives: price move, news, fill, or timer.
2. Signal models produce outputs.
3. The order builder proposes an action with a combined score.
4. The autonomy policy classifies it as AUTO, ASK, or DENY.
5. The risk gate checks it.
6. The executor places the order. Every step is journaled with links to its causes.

```mermaid
sequenceDiagram
    autonumber
    participant S as Event source
    participant A as Signal models
    participant D as Order builder
    participant P as Autonomy policy
    participant R as Risk gate
    participant E as Executor
    participant B as Broker
    participant J as Journal

    S->>A: Price move, news, fill, or timer
    A->>D: Outputs (conviction, confidence, horizon, thesis)
    D->>J: Record decision and its inputs
    D->>P: Proposed action + combined score
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

1. **Triggers:** the user's autonomy rules, for example a combined score below a threshold, an
   order above a size, or an unusual market input (distribution drift). Orders the risk gate would
   deny are never sent for approval.
2. The runtime creates an **approval request** containing the proposed action, the rule that
   triggered it, the model outputs with their authorship, the deadline, and the default applied on
   timeout (skip). It never contains platform-authored alternatives or profit estimates.
3. The agent keeps managing everything else while it waits. **Risk-reducing actions stay automatic.**
4. The **approval service**, which runs in the workspace deployment, sends notifications
   to every configured push channel at once, plus one reminder when a quarter of the time remains
   ([DEC-438](project/decisions/DEC-438.md) item 6), respecting quiet hours; an ordered push → SMS
   → phone chain waits for SMS and phone (E8-7) and a mandate schema field. **Notifications carry only an opaque ID and generic text** ("An agent in your
   workspace needs your approval"), never the trade itself or the agent's name
   ([notifications spec](specs/notifications.md)). Push goes through our relay; SMS,
   email, and chat can go through the customer's own gateways. Large actions can require
   **two approvers**.
5. The approver opens the request. The app fetches the details **directly from the approval
   service**, over the customer's VPN or through the relay with end-to-end encryption, so the
   relay cannot read them. The approver authenticates against the customer's identity provider;
   **high-risk approvals require step-up authentication** (passkey or biometrics).
6. An approval binds the quantity, limit price, and mandate version. Before executing, the risk
   gate runs again on current state; if it denies, the action is skipped.
7. On timeout, the safe default is applied.
8. Every step is journaled: who approved, when, through which channel, and how they authenticated.

```mermaid
sequenceDiagram
    autonumber
    box Customer site in hybrid and on-prem modes
        participant A as Agent runtime
        participant J as Journal
        participant S as Approval service
        participant R as Risk gate
    end
    participant L as Notification relay
    participant U as Approver's phone
    participant B as Broker

    A->>A: An autonomy rule requires approval
    A->>J: Record approval request (action, rule, model outputs, deadline, default)
    A->>S: Create approval request (content stays on site)
    S->>L: Notify with opaque ID and generic text only
    L->>U: Push notification
    opt No response at this escalation step
        S->>U: SMS, email, or call via configured channels (no trade details)
    end
    Note over A: Keeps managing other positions.<br/>Risk-reducing actions stay automatic.
    alt Approver responds before the deadline
        U->>S: Fetch request details (VPN, or relay with end-to-end encryption)
        S-->>U: Proposed action, evidence, risk impact
        U->>S: Approve or reject (customer SSO, step-up auth for high-risk actions)
        S->>A: Response
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
            A->>S: Re-ask, or apply the safe default
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
  - Cold (retention): canonical JSON Lines segments on object storage with write-once (object lock,
    compliance mode) retention, so bytes remain verifiable ([journal spec §6.2](specs/journal.md#62-cold-store-segments-and-retention)).
  - Anchoring: the chain's root hash is periodically published externally so tampering is detectable.
- **Location:** the event store and the audit explorer backend run in the workspace
  deployment, so in hybrid and on-prem modes audit content never leaves the customer's site.
  The audit explorer UI reads directly from that backend.
- **Export** to the customer's SIEM or storage bucket; retention has a platform floor (6 years after
  the later of creation and closing of the supported position, lot, or account) that organizations
  may extend but not shorten.
- **Privacy:** personal data is encrypted with per-user keys. Deleting a key erases the person
  without breaking the immutable log (crypto-shredding).

---

## 8. Multi-tenancy and security

| Concern | Design |
|---|---|
| Tenant boundary | The workspace. Every record carries a workspace ID; row-level security in the database; per-workspace encryption keys; bring-your-own-key for businesses |
| Isolation tiers | Retail: shared cells with logical isolation. Business on managed: dedicated database or dedicated cell. Hybrid and on-prem: physically separate |
| Processes | Agent processes are never shared across workspaces |
| Messaging | NATS JetStream; its account model maps to workspaces, and it runs as a single binary on the edge |
| Roles | Org owner, org admin, billing admin, workspace admin, operator, **approver**, viewer, auditor. Optional separation of duties: an agent's creator cannot be its sole approver for large actions |
| Agent identity | Each agent has its own service identity and scoped tokens; every action is attributed to a specific agent |
| Credentials | Managed: vault backed by a key management service. Hybrid and on-prem: local vault; **credentials never leave the customer's environment** |
| Data location | Sensitive data stays in the workspace deployment; the global control plane holds only IDs, counts, and versions (see [Where data lives](#where-data-lives)) |
| Authentication | Workspace deployments authenticate users directly against the customer's identity provider, so logins and approvals keep working without the global control plane |
| Step-up authentication | Required to connect accounts, raise limits, approve large trades, or go live |
| Quotas | Per workspace: agents, model spend, API rate, data subscriptions |

**Kill switches at every level:** agent, connection, workspace, organization, and a global
switch for the managed service. Hybrid and on-prem customers control their own; the managed
global switch cannot reach into customer deployments.

**Correlated research-agent flow** ([DEC-100](project/04-decision-log.md#decisions)): each
workspace's risk gate decides from that workspace's own state only. Two operator controls run in
the workspace deployment, outside the trade path and never in the global control plane. An
aggregate-flow monitor sums research-agent exposure per instrument over the workspaces of its
deployment (a managed cell or a customer site) and alerts the operator; it writes to no workspace.
An operator per-thesis halt stops research-agent admissions and openings in a named instrument,
optionally for one research-agent version, in every workspace of the deployment, while exits and
protection continue; it only removes permissions and is journaled in each workspace. As with the
global switch, the managed halt cannot reach into customer deployments, whose operators run their
own monitor and halt from the same installer.

---

## 9. Intelligence layer

- **Model gateway:** routes calls to Jev (hosted decision model), a Laya pool or in-process
  Laya (open-weight decision model), and LLM providers. Handles deadlines and caching, and meters
  cost per workspace and per agent. A fallback may route only to another endpoint serving the
  identical pinned model; otherwise the call fails and the output counts as missing. The gateway
  never substitutes a different model ([DEC-67](project/04-decision-log.md#decisions)). Hybrid and on-prem deployments can be
  restricted to **local models only**.
- **Model scorecards:** measure each signal model's realized hit rate and calibration for the
  user's review. They never change weights or approval routing; in v1 weights are fixed by the
  user and there is no calibration ([DEC-47](project/04-decision-log.md#decisions)).
- **Market data service:** normalized live streams plus a point-in-time historical store.
  In every mode each workspace reads market data through its own broker connection; nothing is
  ingested once and shared in v1, because that would be redistribution (§12 risk 3;
  [data plane spec](specs/data-plane.md), DEC-433 items 3 and 15).

Speed tiers:

| Tier | Latency | What runs there |
|---|---|---|
| Hot | microseconds | Market data handling, risk gate, order management, kill switch |
| Fast | 30–300 ms | Decision models (Laya, Jev) answering typed questions with typed scores |
| Slow | seconds–minutes | LLM research: reading filings and news, writing theses; never blocks trading |

---

## 10. Billing

- **Billed entity:** the organization.
- **Components:** plan (seats and agents), usage (agent-hours, decisions, model tokens at cost
  plus margin, data), and hybrid / on-prem licenses.
- **Pipeline:** workspace deployments emit usage counts (never content) → metering pipeline in
  the global control plane → billing provider (Stripe Billing, Orb, or Metronome). Hybrid
  deployments send signed usage reports over their outbound connection; air-gapped deployments
  use a signed license file plus periodic offline usage reports.
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
| Messaging / durable execution | Postgres `LISTEN`/`NOTIFY` and journal tailing (DEC-17); durable timers are journal-backed (DEC-16) |
| Orchestration | Kubernetes cells (managed); Helm chart or single-node k3s (customer site) |
| Packaging | One installer deploys the global control plane, workspace control services, and data plane in any mode; signed release bundles for air-gapped sites |
| Sandboxing | WebAssembly plug-ins; Firecracker for heavier workloads |
| Observability | OpenTelemetry API with a Prometheus pull exporter (M6); the journal is the audit record, never telemetry ([DEC-73](project/04-decision-log.md#decisions)) |

### Where the code stands (2026-10-03)

This document describes the target design. Most of it is not built yet. The Rust crates in
`crates/` cover these components today; `xtask/layers.toml` records each crate's layer, and
[ADR-0001](adr/0001-engineering-setup.md) gives the crate rules.

| Component in this document | Crates today |
|---|---|
| Agent runtime | `mandate-runtime` (the pure state machine and kill switches); `mandate-shell` (the process shell and the `mandate-tracer` binary) |
| Risk engine | `mandate-risk` (the gate); `mandate-spec` (the mandate, its validation, policy, risk state, change classification); `mandate-builder` (autonomy and the order builder) |
| Execution gateway and connectors | `mandate-executor` (the idempotent executor, reconciliation, protective exits); `mandate-alpaca` (the Alpaca paper connector) |
| Journal and state | `mandate-journal`, `mandate-journal-pg` (the Postgres hot store), `mandate-journal-cold` (the cold store and export), `mandate-artifacts-fs` (the artifact store), `mandate-accounting` (the account fold) |
| Approval service | `mandate-approval` (the pure approval core only) |
| Research agent (§9) | `mandate-research` (the thesis contract and its admission checks) |
| Market data | `mandate-marketdata` (Alpaca historical data into Parquet) |
| Backtesting | `mandate-sim` (the fill model), `mandate-backtest` (the loop and its report) |
| Shared foundations | `mandate-num`, `mandate-time`, `mandate-canon`, `mandate-domain`; `mandate-refcases` is the reference-case test harness |
| Command line | `mandate-cli` |
| Web app | `web/` (Next.js, DEC-200); there is no mobile app |

Planned, with no code yet: the whole global control plane (directory, licensing and billing,
fleet health, notification relay, connector catalog and model registry); the agent registry and
spec compiler, policy service, deployment manager, audit explorer backend, and connection manager
as services; the secrets vault; the model gateway; the shared data plane; the Python SDK and PyO3
bindings; object storage and ClickHouse; Kubernetes, Helm, and the installer; and WebAssembly and
Firecracker sandboxing. Python today is repository tooling (`python/mandate_tools`) and a research
spike (`python/research_spike`).

---

## 12. Risks and open decisions

1. **Retail regulation (decided, [DEC-98](project/04-decision-log.md#decisions)).** Retail is in
   scope from the start, with the platform originating ideas, so the working assumption is that
   Mandate may be an investment adviser. Counsel is engaged during Phase 0
   ([DEC-102](project/04-decision-log.md#decisions)) and no user trades live until counsel signs off.
2. **NautilusTrader licensing.** It is LGPL-3.0. Distributing it inside on-prem software,
   particularly statically linked Rust, carries relinking obligations. Decide whether to use
   its connectors or write our own.
3. **Market data redistribution.** Passing equities exchange data to tenants requires vendor
   and exchange licenses. In v1, each user's market data comes through their own Alpaca
   account, so Mandate does not redistribute it; any shared data offering needs licensing first.
4. **Custom code in v1.** Declarative specs only, or also WebAssembly plug-ins?
5. **First connectors (decided, [DEC-23](project/04-decision-log.md#decisions), reordered by
   [DEC-98](project/04-decision-log.md#decisions)).** Alpaca first (US stocks, ETFs, crypto spot;
   paper trading; OAuth), Robinhood Agentic Trading second (retail US equities and crypto spot over
   MCP into the customer's dedicated agentic account), Kraken Derivatives US third (CFTC-regulated
   crypto perpetuals), then Interactive Brokers and Coinbase US futures. The platform serves the
   United States first.
6. **Approval channels in v1 (decided, [DEC-19](project/04-decision-log.md#decisions)).** Web push,
   email, and one chat channel; SMS and phone next; a native mobile app later. The staging by
   milestone is Proposed in [DEC-438](project/decisions/DEC-438.md).
7. **Mobile access to on-site approval services.** Whether approvers reach the customer's
   approval service through the customer's VPN, through our relay with end-to-end encryption,
   or both; and how the mobile app is distributed to firms that require device management.
8. **Directory data in hybrid mode.** The minimum identity data the global control plane needs
   for seat billing and routing (for example, hashed user IDs instead of names and emails).
