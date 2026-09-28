# Backlog: v1

| | |
|---|---|
| **Owner** | Product and project management |
| **Status** | Draft v0.1 |

Epics map to [milestones](02-milestones-and-wbs.md) and [PRD](../product/04-prd-v1.md)
requirements. Priority uses MoSCoW: **Must**, **Should**, **Could**, **Won't (v1)**.
Stories follow "As a … I want … so that …" with acceptance criteria.

## Epic overview

| Epic | Milestone | PRD | Priority |
|---|---|---|---|
| E1 Foundations | M0 | — | Must |
| E2 Market data | M1 | 6.4 | Must |
| E3 Accounting | M2 | 6.4 | Must |
| E4 Simulated execution and backtest | M3 | 6.4 | Must |
| E5 Journal | M4 | 6.7 | Must |
| E6 Agent runtime and risk | M5 | 6.5 | Must |
| E7 Alpaca connector and recovery | M6, M8 | 6.2, 6.5 | Must |
| E8 Escalation and approvals | M7, M10 | 6.6 | Must |
| E9 Identity, tenancy, and policy | M8 | 6.1 | Must |
| E10 Mandate authoring | M8, M9 | 6.3 | Must |
| E11 Web app: dashboard and controls | M9 | 6.8 | Must |
| E12 Audit explorer | M9 | 6.7 | Must |
| E13 Hybrid deployment | M11 | 6.9 | Must |
| E14 Billing | M12 | 6.10 | Must |
| E15 Signal models: LLM and fast models, scorecards | M5 (LLM research, scorecards); Phase 3 (fast models) | 6.3, 6.5 | Must (E15-1, E15-3); Should |
| E16 Kraken Derivatives US connector | Phase 3 | 6.2 (FR-2.5) | Should |
| E17 Research agent and dynamic universe | M5 | 6.3 (FR-3.9), 6.5 | Must |

## Stories

### E1 Foundations

- **E1-1 (Must)** As an engineer, I want a Rust workspace and Python package with CI so that
  every change is built, linted, and tested.
  *Accepted when:* CI runs build, tests, and lint on every push; main is protected.
- **E1-2 (Must)** As an engineer, I want an ADR template and coding conventions so that
  decisions and code stay consistent.
- **E1-3 (Should)** As an engineer, I want the pending-test gate to read a failure's *cause* rather
  than its text, so that no wording in a test can stand in for the stub it is meant to reach
  (DEC-137, gap 1's remainder; the coordinator's round-2 ruling item 4).
  *Accepted when:* `cargo xtask ci pending` accepts only the `Err` value printed after
  `called \`Result::unwrap()\` on an \`Err\` value:`, the `todo!`/`unimplemented!` panic line, or the
  `Err(..)` `Debug` inside a proptest failure, and a marker written into a test's own assertion
  message no longer satisfies it; `BEHAVIOUR_ONLY_TESTS` is retired as E6-6 and E6-8 land, or
  replaced by a rule that reads the cause; and the planted cases of #172's reviews all fail the
  gate. Since DEC-164 a failing property is already read by proptest's report of its minimal
  failure alone, and `mandate-executor`'s three E7-4 properties that passed only on a stub report
  from a case shrinking moved past have their rows; E1-3 narrows that report, and every other
  test's output, to the cause.

### E2 Market data

- **E2-1 (Must)** As a researcher, I want to download historical bars and trades for US stocks,
  ETFs, and crypto (starting with a stock/ETF basket and BTC/USD) for a date range so that I can
  backtest.
  *Accepted when:* `download` fetches Alpaca historical data into Parquet; re-running is idempotent.
- **E2-4 (Must)** As a researcher, I want corporate actions (splits, dividends) and market
  sessions recorded with the data so that stock history and gaps are interpreted correctly.
  *Accepted when:* `inspect` distinguishes session closures from true gaps; split-adjusted and
  raw prices are both available.
- **E2-2 (Must)** As a researcher, I want to inspect a dataset for coverage, gaps, duplicates,
  and summary statistics so that I trust it before using it.
  *Accepted when:* `inspect` reports gaps with exact timestamps; tests cover gap and duplicate detection.
- **E2-3 (Should)** As a researcher, I want order-book top-of-book data so that slippage models
  can use spreads.

### E3 Accounting

- **E3-1 (Must)** As a trader, I want positions, cash, fees, and realized and unrealized P&L
  computed correctly so that every later number is right.
  *Accepted when:* property-based tests and hand-calculated cases pass, including partial fills
  and position flips.
- **E3-2 (Must)** As a trader, I want splits and dividends applied to positions and cash so that
  stock P&L is correct.
- **E3-3 (Must)** As a trader, I want settlement tracked for cash accounts so that the system
  knows which cash is available to trade.

### E4 Simulated execution and backtest

- **E4-1 (Must)** As a researcher, I want market and limit orders filled with configurable
  slippage and fees so that backtests are realistic.
- **E4-2 (Must)** As a researcher, I want a baseline strategy and a metrics report (return,
  volatility, Sharpe, maximum drawdown, turnover, fees, buy-and-hold comparison) so that the
  loop is proven end to end.
  *Accepted when:* identical inputs produce identical outputs.

### E5 Journal

- **E5-1 (Must)** As an auditor, I want every event appended to a hash-chained journal with
  causal links so that history cannot be silently altered.
  *Accepted when:* the [journal test vectors](../specs/reference-cases/journal.yaml) (chain, string
  escaping, decimals, export line, Merkle anchor) reproduce byte for byte; the verification tool
  reports each tamper case's expected first failure; and the append protocol returns each append
  case's expected outcome ([journal spec](../specs/journal.md)).
- **E5-3 (Must)** As an operator, I want the Postgres journal hardened (body stored as exact
  canonical bytes with a hash check, append-only roles and triggers including TRUNCATE, stream
  heads with writer fencing) so that records cannot be altered by application code.
- **E5-2 (Must)** As an engineer, I want large artifacts stored by content hash so that the
  journal stays small and verifiable.
- **E5-5 (Must)** As an owner, I want my personal data (the broker's account number and account
  id, names, emails) held in the workspace's personal-data vault under a per-person key, with each
  journal event carrying only random vault references in `pii_refs`, so that the records can be
  kept and my data erased separately ([journal spec](../specs/journal.md) §3, §6.4). It replaces
  DEC-142's interim: `mandate-alpaca`'s record discards a redacted value and labels it by position.
  *Accepted when:* a recorded exchange's redacted values are in the vault, its `pii_refs` are the
  vault's random references passed into the record as an input (ES-21), sorted, and none is a hash
  of the value.
- **E5-4 (Must)** As an auditor, I want `mandate-cli journal verify` over an exported stream and
  its artifact store, and a CLI command to put and fetch artifacts, so that I can check a journal
  without writing code (M4's verification tool; deferred from the E5-1 and E5-2 briefs).
  *Accepted when:* for each [tamper case](../specs/reference-cases/journal.yaml) its input can
  express, the command reports the expected first failure; a deleted or altered artifact reports
  `artifact_missing` or `artifact_mismatch` ([journal spec](../specs/journal.md) §11).

### E6 Agent runtime and risk

- **E6-1 (Must)** As an operator, I want an agent to run a mandate continuously so that it
  trades without supervision.
- **E6-2 (Must)** As an operator, I want every proposed action classified AUTO, ASK, or DENY
  per the mandate so that autonomy matches my rules. *Accepted when:* mandate reference cases
  MC-A01 to MC-A11 and MC-B01 to MC-B29 pass.
- **E6-3 (Must)** As an owner, I want an independent risk gate enforcing all limits so that no
  agent logic can exceed them.
  *Accepted when:* simulation fuzzing across random market paths and mandates never produces
  an order outside limits; the mandate invariants MI-1 to MI-11 hold under property-based tests;
  mandate reference cases MC-G01 to MC-G13 and MC-F01 to MC-F04 pass.
- **E6-4 (Must)** As an owner, I want a daily-loss limit and a drawdown ladder so that losses
  trigger automatic de-risking. *Accepted when:* MC-R01 to MC-R15, MC-T01 to MC-T05, and MC-L01
  to MC-L09 pass.
- **E6-5 (Must)** As an owner, I want kill switches per agent, connection, and workspace so
  that I can stop everything immediately.
- **E6-6 (Must)** As an owner, I want US account rules (day-trading regime, settlement, short
  sales, market hours) enforced by the risk gate so that agents never get my account restricted.
  *Accepted when:* simulation tests for each rule pass; blocked orders are journaled with the rule.
- **E6-7 (Must)** As an owner, I want an instrument eligibility floor (exchange-listed, no OTC or
  IPO-day, price and liquidity floors, leveraged ETPs only with opt-in) so that agents stay in
  liquid, suitable instruments. *Accepted when:* RC-16 passes.
- **E6-8 (Must)** As an owner, I want market-conduct controls (one working order per side,
  minimum resting time, price collars, participation caps, order-to-fill limits, close-window
  rules, workspace self-trade prevention) and a daily surveillance report, so that agents cannot
  produce manipulation-like patterns.
- **E6-9 (Must)** As an owner, I want account restrictions and trading halts checked before every
  order, so that agents stop adding risk when the broker restricts the account. *Accepted when:*
  RC-15 passes.
- **E6-10 (Must)** As an owner, I want the gate to admit crypto **USD pairs only** (trading-domain
  §3.2 item 7), so that an agent cannot open a stablecoin-quoted pair the floor was never written
  for. Needs a quote-currency input: `InstrumentSnapshot` carries none and `AssetId` is a UUID, so
  today nothing tells BTC/USD from BTC/USDT. Until it lands the gate keeps a crypto opening owed at
  check 2 and refuses it fail-closed (DEC-129 item 34). *Accepted when:* a crypto opening in a
  non-USD pair is denied, a USD pair passes the floor, and check 2 is whole for crypto.

### E7 Alpaca connector and recovery

- **E7-1 (Must)** As an operator, I want to connect my Alpaca paper and live accounts through
  OAuth, granting trading access only, so that agents can trade without Mandate ever being able
  to move my funds.
  *Accepted when:* the OAuth request contains only trading and account-read scopes; tokens are
  stored only in the workspace vault.
- **E7-2 (Must)** As an owner, I want order intents journaled with idempotency keys so that
  crashes never duplicate orders.
- **E7-3 (Must)** As an owner, I want the agent to reconcile with the exchange after a restart
  so that its state matches reality.
  *Accepted when:* fault injection at every submission step yields zero duplicates and full
  reconciliation; mismatches pause the agent and alert.
- **E7-4 (Must)** As an owner, I want protective exits resting at the broker as OCO or bracket
  orders, with defined exit and kill-switch sequences, so that positions keep protection if the
  platform is down.
  *Accepted when:* RC-14 passes; unprotected windows are journaled and alerted beyond the limit.
- **E7-6 (Must, M8)** As a retail user, I want to connect a Robinhood agentic trading account over
  MCP so that my agent trades US equities and crypto spot with the funds I deposited there and
  nothing else ([DEC-98](04-decision-log.md#decisions),
  [OD-12](04-decision-log.md#open-decisions)).
  *Accepted when:* the connector can place, cancel, and reconcile equity orders in the dedicated
  account only; every MCP exchange is journaled; beta terms are recorded; the connector requests and
  stores only the agentic account's data; account numbers are held by reference (journal spec §6.4)
  and never logged.
- **E7-5 (Must)** As an owner, I want one account ledger per broker account and one agent per
  instrument per account, so that agents never overspend or cross each other.
  *Accepted when:* RC-17 passes; external activity switches agents to exits-only (RC-15).
- **E7-7 (Must, M6)** As the founder, I want one order placed end to end on my Alpaca paper account
  through the real crates, so that integration defects appear before the Phase 1 soak
  ([task brief](tasks/E7-7-tracer-bullet.md), [DEC-138](04-decision-log.md#decisions)).
  *Accepted when:* the whole path — validated mandate, stored market data, the E4-2 moving-average
  baseline as the signal, the order builder's sizing, the risk gate, the runtime's decision cycle with
  journal-before-acting, the executor's idempotent intent, the Alpaca paper connector, the journal
  record, and a reconciliation after a restart — runs in CI against recorded Alpaca paper fixtures and
  touches no network; with any one stage replaced by a stub returning its crate's `Unimplemented`
  error, zero orders reach the connector and nothing is journaled as submitted; and a manual paper run
  places exactly one order, journals its intent before sending it, and refuses any host that is not
  Alpaca's paper host.

### E8 Escalation and approvals

- **E8-1 (Must, M7)** As an approver, I want requests with the proposed action, alternatives,
  evidence, risk impact, deadline, and default so that I can decide quickly
  ([task brief](tasks/M7-escalation-v0.md), [DEC-155](04-decision-log.md#decisions)). "Alternatives"
  means the owner's choices (approve or skip, with the default stated), never platform-authored
  alternative trades ([mandate spec §6.4](../specs/mandate.md#64-approvals), FR-6.2).
  *Follow-up (DEC-165 item 3, #236):* the content's Trigger row still lacks "the rule as the owner
  wrote it". It joins the content object once the M7 spec PR fixes §6.4's list, with a test that
  scans owner-written text apart from the platform's own in
  `the_content_never_carries_advice_wording`, so an owner's rule named `target_weight` is shown as
  written and never read as platform advice.
  *Follow-up (#250 review, minor 1):* `RequestContent.risk_impact` is a `Vec<RiskFigure>`, so a
  caller could list fewer than §6.3's six figures, or one twice. Make it one figure per
  `RiskField` by type (`[RiskFigure; 6]` in `RiskField` order, or a map keyed by field), with a
  tests correction for `every_bound_field_moves_the_content_hash`, which truncates the list.
  *Done (#250 review, major; DEC-165 item 13):* `ApprovalRef::of_requested_event` accepts only a
  ULID-shaped event id, so free text cannot reach a notification through it. The runtime or CLI
  PR that first calls it must prove the id is that `ApprovalRequested` event's own.
- **E8-2 (Must, M7)** As an owner, I want timeouts to apply the safe default so that silence never
  adds risk ([task brief](tasks/M7-escalation-v0.md), [DEC-156](04-decision-log.md#decisions)).
  *Follow-up (#250 review, minor 4):* EI-13's first bound, one pending risk-adding approval per
  agent, is not in `mandate_approval::ask_permit`; it stays in the runtime's
  `awaiting_risk_approval`, and the runtime's tests PR must assert it.
- **E8-3 (Must, M7)** As an owner, I want approved actions re-validated for drift so that stale
  approvals are not executed blindly ([task brief](tasks/M7-escalation-v0.md),
  [DEC-156](04-decision-log.md#decisions)). The same brief covers M7's CLI owner control.
- **E8-4 (Must)** As an approver, I want notifications through web push, email, and a chat
  channel, with escalation chains and quiet hours.
- **E8-5 (Must)** As a fund, I want notifications to carry only opaque IDs, with details loaded
  from our workspace deployment, so that trading intent stays private.
  *Accepted when:* captured relay and provider payloads contain no instrument, size, price, or thesis.
- **E8-6 (Should)** As a fund, I want two approvers above a threshold.
- **E8-7 (Should)** As an approver, I want SMS and phone escalation.

### E9 Identity, tenancy, and policy

- **E9-1 (Must)** As a user, I want to sign in with passkey or OIDC SSO.
- **E9-2 (Must)** As an admin, I want organizations, workspaces, and roles.
- **E9-3 (Must)** As an admin, I want org-level limits that workspaces and agents can only
  tighten.
- **E9-4 (Must)** As a security-conscious user, I want step-up authentication for sensitive
  actions.
- **E9-5 (Should)** As a fund, I want separation of duties between agent creators and approvers.
- **E9-6 (Must)** As a retail user, I want the retail profile (`auto` allowed, LLM ideas allowed,
  protection required, no leveraged ETPs, counsel-set loss ceiling and approval timeout minimum)
  applied to my workspace by default ([DEC-98](04-decision-log.md#decisions)).

### E10 Mandate authoring

- **E10-1 (Must)** As an operator, I want to describe an agent in plain language and get a
  compiled mandate with inferred fields highlighted. *Accepted when:* compiled mandates validate
  against the [mandate spec](../specs/mandate.md) (schema, V-rules, policy hierarchy; reference
  cases MC-S, MC-V, and MC-P pass); proposed envelope values are marked as proposed and no envelope
  field activates unconfirmed ([DEC-97](04-decision-log.md#decisions)).
- **E10-2 (Must)** As an operator, I want to edit the mandate as a form or YAML, kept in sync.
- **E10-3 (Must)** As an operator, I want mandates versioned with viewable diffs, and changes that
  increase risk to require step-up. *Accepted when:* the version vector and MC-C01 to MC-C35 pass.
- **E10-4 (Must)** As an operator, I want going live to require a backtest, a paper run, and
  step-up approval.
- **E10-5 (Should)** As a new user, I want templates for common mandates.
- **E10-6 (Should, M8, pulled forward)** As an owner who already runs my own agent (for example
  Claude), I want to connect it through a Mandate MCP server that exposes the same API Mandate uses
  to take my input, so that my agent can work inside my mandate without a separate path around it
  ([DEC-141](04-decision-log.md#decisions), [DEC-148](04-decision-log.md#decisions)). An optional
  channel and an adoption on-ramp, never the only or the main path: the complete product leads, and
  the platform's research agent brings the ideas. **Placement (DEC-148):** the first M8 story after
  the owner-input API (M8's mandate registry, approval service, and owner controls), E9-1, E9-2,
  E9-4, and E10-3, ahead of M8's other Should stories (E9-5, E10-5); it does not wait for M9 or M10,
  and its earlier dependencies (E5, E6-2, E6-3, E7-2, E7-3, E7-5, and the tracer bullet E7-7) land
  before the Phase 1 gate. No change to milestone order. *Accepted when:* every client request
  passes through the same order builder, autonomy rules, risk gate, account ledger, and journal as
  the owner's own input; the client cannot change an envelope field (it may only propose a mandate
  version that the human confirms with step-up); ASK approvals go only to the human owner, and a
  client cannot approve its own proposal; owner-only privileges (owner exits outside the regular
  session at a confirmed bid, Stop and release, the kill switch) stay with the human; the client
  authenticates with its own scoped, revocable token and no broker credential crosses MCP; every
  client call is journaled with the client's identity; and the owner can revoke the client at any
  time.

### E11 Web app: dashboard and controls

- **E11-1 (Must)** As an operator, I want a dashboard of agents, state, positions, P&L, open
  approvals, and recent decisions.
- **E11-2 (Must)** As an operator, I want pause, resume, stop, and kill switches in the UI.
- **E11-3 (Must)** As an operator, I want alerts for risk rungs, reconciliation mismatches,
  stale data, and paused agents.

### E12 Audit explorer

- **E12-1 (Must)** As an auditor, I want a causal trace from any fill back to its causes.
- **E12-2 (Must)** As an auditor, I want per-agent timelines with filters and JSON/CSV export.
- **E12-3 (Should)** As an auditor, I want to run chain verification from the UI.
- **E12-4 (Could, not yet planned)** As an owner, I want a monthly record of every mandate breach
  and near-breach on my account, derived from the journal and its anchors, so that I can see the
  mandate held ([strategy options §8](../product/10-strategy-options.md#defensible-differentiators),
  DEC-145). Publishing it beyond the owner needs counsel's answer and the founder (DEC-79).

### E13 Hybrid deployment

- **E13-1 (Must)** As an IT admin, I want to install the workspace deployment with Helm or
  Docker Compose using an enrollment token, with outbound-only connectivity.
- **E13-2 (Must)** As an IT admin, I want signed releases and upgrades that preserve agent state.
- **E13-3 (Must)** As an IT admin, I want SSO against our identity provider and a fallback
  approval channel through our own mail server.

### E14 Billing

- **E14-1 (Must)** As an org owner, I want to subscribe to a plan and be billed per
  organization.
- **E14-2 (Must)** As an org owner, I want a hybrid license key tied to my organization.
- **E14-3 (Should)** As an org owner, I want usage metering visible in the app.

### E15 Signal models: LLM and fast models, scorecards

- **E15-1 (Must)** As an operator, I want an LLM research signal model that writes theses
  asynchronously without blocking trading.
- **E15-2 (Should)** As an operator, I want a fast decision model with a hard deadline.
- **E15-3 (Must, Phase 1)** As an operator, I want each signal model's confidence measured against
  outcomes and shown in scorecards ([DEC-99](04-decision-log.md#decisions)).
  *Accepted when:* every thesis is scored after its horizon against the pre-registered baselines
  (buy-and-hold of the eligible basket, and a broad index ETF), net of modeled costs; scores come
  from forward paper trading only, never from historical backtests of LLM theses; the scorecard
  shows each signal model's results beside the baselines.
- **E15-4 (Could)** As an operator, I want shadow mode for a new mandate version.
- **E15-5 (Should, after the Phase 1 exit)** As an owner, I want to run up to three variants of my
  mandate at once, one live and the others in shadow, so that I can compare strategy changes before
  risking money ([strategy option 16](../product/10-strategy-options.md#option-16-mandate-experiments-multi-variant-shadow-mode-founder-2026-09-27)).
  Subsumes E15-4 (one shadow candidate is the one-variant case) and is pulled forward from Phase 3;
  no change to the v1 milestones. *Accepted when:*
  each variant is its own owner-confirmed mandate version that differs only in strategy fields (signal
  models, weights, thresholds, cadence); shadow variants see the same market data and pass the same
  gate, evaluated against their own simulated account state, never consuming or holding the live
  account's buying power, day-trade budget, or reservations (so no shadow variant can hold a live
  exit, rule 13); they keep a simulated book labelled as simulated, any comparison between variants is
  labelled as hypothetical performance, and they send nothing to the broker; each variant's
  hypothesis and success criterion are journaled before it runs; promotion is only an owner action
  that creates a new confirmed mandate version, with no automatic winner-picking; and no mandate holds
  more than three variants.

### E17 Research agent and dynamic universe

Design: [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) ([DEC-97](04-decision-log.md#decisions)).
The mandate spec, schemas, reference implementation, and cases are rewritten first in spec-change PRs.
The first delivery is the [DEC-103](04-decision-log.md#decisions) thin slice: the research agent
runs only in the team's internal paper workspaces, with the research basket (DEC-90) as its fixed
test data universe, every admission `ask`, paper only, and scorecards on. The basket is never a
user's instrument choice. The full E17-3, for users' agents under their own envelopes, follows only
after the DEC-99 evaluation (E17-8) passes on the thin slice.

- **E17-0 (Must, now)** research spike: an LLM loop over news and prices, paper-traded on the
  research basket with fixed sizing, to de-risk E17-2 before it is product code
  ([task brief](tasks/RS-1-research-spike.md); `python/research_spike/`).
  *Accepted when:* a report with hit rate, expectancy versus SPY, and cost per thesis after two to
  three weeks of paper trading.
- **E17-1 (Must)** As an owner, I want the mandate split into envelope fields I confirm and a working
  universe the platform produces at runtime, so that I set the risk and the agent brings the ideas.
  *Accepted when:* the rewritten mandate spec's cases pass; the compiler may propose envelope values,
  each marked as proposed; no envelope field activates unconfirmed.
- **E17-2 (Must)** As an owner, I want a research agent that turns market data, news, filings, and
  the agent's memory into theses (instrument, direction, horizon, evidence, invalidation), journaled
  as `ThesisProposed`, so that the agent has ideas without me.
- **E17-3 (Must)** As an owner, I want instruments admitted into the working universe only through
  the eligibility floor, the policy's asset classes, `max_instruments`, instrument-group claims, and
  my autonomy rules (`new_instrument`, `thesis_confidence`; default `ask`), journaled as
  `UniverseChanged`, and removed to exits-only when a thesis is invalidated.
  *Accepted when:* simulation fuzzing over random theses never admits an ineligible instrument or
  exceeds the envelope; prompt-injection fixtures never reach an order.
- **E17-4 (Must)** As a fund, I want a bring-your-own-strategy mode that pins the universe and
  disables the research agent, so that today's behavior stays available.
- **E17-5 (Must)** As an owner, I want the input-drift detector (`unusual_input`, V-018) so that
  unusual inputs escalate before the research agent acts on them
  ([DEC-101](04-decision-log.md#decisions)).
- **E17-6 (Must)** As an operator, I want to see and stop research-agent flow across the accounts
  of a deployment, so that one thesis cannot concentrate orders from many accounts in one instrument
  unnoticed ([DEC-100](04-decision-log.md#decisions)).
  *Accepted when:* no workspace's gate reads another workspace's state (a two-workspace test shows
  one's positions never change the other's decisions); the aggregate-flow monitor sums research-agent
  exposure per instrument over its deployment's workspaces, in dollars and as a share of average
  daily dollar volume, alerts the operator above the thresholds, writes to no workspace, and sends
  nothing to the global control plane; the operator per-thesis halt, journaled in each workspace as
  a `PlatformOperatorAction`, stops matching research-agent admissions and openings in every
  workspace of the deployment while exits and protection continue, and never permits anything a
  workspace's own limits deny; openings on a new thesis wait for the workspace's deterministic
  stagger offset within the conduct controls; bring-your-own-strategy agents keep per-account
  controls only.
- **E17-7 (Must)** As an owner, I want the research agent to read only vetted sources and to admit an
  instrument only on corroborated evidence, so that one planted source cannot admit an instrument
  ([DEC-101](04-decision-log.md#decisions)).
  *Accepted when:* the source allowlist is versioned configuration; the agent reads no source outside
  it; an admission without corroboration by an independent source, or by market data consistent with
  the thesis, is rejected; the corroboration is recorded in `ThesisProposed`; prompt-injection
  fixtures for every input source never reach an order.
- **E17-8 (Must)** As the founder, I want a forward paper evaluation harness, so that thesis quality
  is judged on outcomes the model cannot have seen ([DEC-99](04-decision-log.md#decisions)).
  *Accepted when:* the evaluation window, metric, and pass threshold come from a recorded decision
  made before the evaluation starts; it runs on the team's internal paper workspaces (DEC-103), so
  no user's results are aggregated; every thesis is scored after its horizon against buy-and-hold of
  the eligible basket and a broad index ETF, net of the cost model; the report states pass or fail
  against the threshold and is reproducible from the journal.
- **E17-9 (Should)** As an owner, I want the research agent to revise a thesis that failed on
  forward paper, with its autopsy recorded, so that the platform improves its ideas without hiding
  its failures ([DEC-111](04-decision-log.md#decisions)). *Accepted when:* a revision is journaled
  as `ThesisRevised` linked to its predecessor and names the failure it addresses; it starts with an
  empty scorecard and is scored only by the E17-8 evaluator; it passes the eligibility floor,
  corroboration, and the autonomy rules like a new thesis and cannot loosen any envelope field; past
  `max_revisions_per_lineage` the lineage is retired and the owner is told. Depends on E17-8 and
  on one completed DEC-99 evaluation on the DEC-103 thin slice.

### E16 Kraken Derivatives US connector (Phase 3)

- **E16-1 (Should)** As an operator, I want to connect a Kraken Derivatives US account (demo and
  live) with a trading-only key so that agents can trade CFTC-regulated crypto perpetuals.
  *Accepted when:* keys with withdrawal or transfer permissions are rejected before storage.
- **E16-2 (Should)** As a trader, I want perpetuals accounting (funding every eight hours,
  margin, liquidation thresholds) so that perpetual P&L and risk are correct.
- **E16-3 (Should)** As an operator, I want a funding/carry signal model for perpetuals.

## Won't (v1)

Live retail trading before counsel signs off; users outside the US; options; Interactive Brokers and Coinbase connectors; native mobile apps; WebAssembly plug-ins; SAML and SCIM;
fully on-prem control plane; shared data plane; strategy marketplace.

## Later (wanted after v1)

- **Custom signals for power users** ([DEC-20](04-decision-log.md#decisions)). Today a mandate chooses the
  platform's registered signal models, their declared parameters, fixed weights, thresholds, universe,
  autonomy, protection and cadence, but it cannot define a new signal formula, combine signals other than
  by fixed linear weights, or change sizing. Two steps, in order:
  1. A **declarative signal-expression layer**: arithmetic and comparisons over the platform's indicators
     and data, stored in the mandate, validated, bounded, and replayed deterministically. It keeps the
     mandate checkable.
  2. **WebAssembly plug-ins**, only if step 1 falls short: sandboxed, resource-limited, versioned by content
     hash, with every output journaled. A plug-in emits only an opinion (a signal or a thesis), which enters
     through the eligibility floor, the autonomy rules and the risk gate like the platform's own ideas. It
     never sizes or places an order (rule 4).

## Enterprise harness (proposed, DEC-149)

Epic **E18**, from the [harness engineering research](../product/11-harness-engineering.md#7-recommendations)
of 2026-09-27. [DEC-149](04-decision-log.md#decisions) makes the harness (gate, autonomy rules,
journal, executor, connectors, conformance suite, and MCP channel) an enterprise product that the
retail platform runs through. Every story here is **(Proposed, not scheduled)**: none is in the epic
overview, none is assigned a milestone, and v1's milestones do not change. **SC** marks a story that
touches a safety-critical path (gate, autonomy, journal, executor, connectors, credentials, auth, or
tenant isolation): the `AGENTS.md` safety-critical rules apply to it (tests written or verified
against approved reference cases first, property tests for its invariants, an independent review by
an agent on a different model, zero missed mutants, and green CI).

- **E18-1 (Proposed, not scheduled; SC)** As a broker or fintech, I want a versioned tenant policy
  overlay on my customers' mandates, with inline tests, so that my house rules apply to every agent.
  *Accepted when:* the overlay can only tighten a mandate, never loosen it (as E9-3's org limits
  do), and property tests prove that for any mandate and overlay the effective limits are at least
  as strict as both.
- **E18-2 (Proposed, not scheduled; SC)** As an enterprise SRE, I want OpenTelemetry GenAI spans
  derived from the journal so that agent runs show in my tracing stack. *Accepted when:* spans are
  derived from journaled events only, content attributes are off by default (no order details,
  positions, or mandate content leave the deployment unless the customer turns them on, rule 6 and
  "Do not"), and the semantic-conventions version is pinned.
- **E18-3 (Proposed, not scheduled; SC)** As a compliance officer, I want SIEM export and a signed
  per-period evidence pack so that I can file the period's record. *Accepted when:* the pack holds
  the chain segment, anchor proofs, mandate versions, gate verdicts, and reconciliation results,
  verifies offline with `journal verify`, and exports in at least one documented SIEM format.
- **E18-4 (Proposed, not scheduled; SC)** As an agent builder, I want the MCP channel to be a policy
  enforcement point so that my agent can act only through Mandate's rules. *Accepted when:* it
  accepts only tokens issued to Mandate and never passes a token through to a broker or another
  service; its tools are intent-shaped (`propose_intent`, `explain_verdict`), never a raw
  `place_order`; scopes start read-only and elevate incrementally; and tool schemas are pinned, failing
  closed on a change. Builds on E10-6.
- **E18-5 (Proposed, not scheduled; SC)** As an owner, I want each mandate version and each ASK
  approval signed with my step-up credential and journaled, in the manner of AP2's signed mandates,
  so that my intent is provable. *Accepted when:* an unsigned or wrongly signed version or approval
  is refused and journaled, and the signature verifies from the journal alone. Any legal wording
  about what a signature means is reserved for the founder (DEC-79).
- **E18-6 (Proposed, not scheduled)** As a connector or agent vendor, I want the conformance suite
  packaged as a certification kit so that I can show my integration is safe to connect.
  *Accepted when:* a third party runs it against its connector or agent and gets a report of pass^k
  over seeded fuzz runs, reproducible from the seed.
- **E18-7 (Proposed, not scheduled)** As a buyer, I want an adversarial bench so that I can see what
  a compromised model can do. *Accepted when:* with injected news, filings, and tool outputs, it
  measures the limit-breach rate given a fully compromised model (every model output adversarial),
  and the pass condition is zero breaches.
- **E18-8 (Proposed, not scheduled; SC)** As a platform operator, I want tenant isolation as a
  checked invariant so that no tenant can read or affect another's state. *Accepted when:* each
  tenant has its own chains and anchors, type or layer rules make cross-tenant access
  unrepresentable where possible, and a cross-tenant fuzz finds no leak.
- **E18-9 (Proposed, not scheduled; SC)** As an enterprise admin, I want OIDC and SAML SSO, SCIM,
  and AUDITOR and SUPERVISOR roles with ASK routing to supervisors so that the harness fits my
  identity and supervision model. *Accepted when:* each role's permissions are tested, and an ASK
  can route to a supervisor without letting anyone approve their own proposal. SAML and SCIM stay
  after v1 ([DEC-18](04-decision-log.md#decisions)).
- **E18-10 (Proposed, not scheduled; SC)** As a regulated firm, I want a self-hosted or VPC
  deployment with a pluggable external anchor (my WORM store or a transparency log) so that my
  records stay under my control. *Accepted when:* anchors written to the customer's store verify
  with `journal verify`. Relates to E13 and strategy option 15; a fully on-prem control plane stays
  Won't (v1).
- **E18-11 (Proposed, not scheduled; SC)** As a quant team, I want a sandbox for my strategy code so
  that it can propose but never trade. *Accepted when:* sandboxed code has no broker egress and no
  vault access and can emit only intents, which enter through the builder, autonomy rules, and gate
  (rule 4). Relates to DEC-20's later WebAssembly plug-ins.
- **E18-12 (Proposed, not scheduled; SC)** As a risk officer, I want sequence and flow policies over
  journaled intents so that order splitting and churn are caught. *Accepted when:* a split order
  that would breach a limit as one order is denied, and the policy reads journal state only.
- **E18-13 (Proposed, not scheduled; SC)** As an auditor, I want `mandate replay <range>` so that I
  can reproduce every gate verdict in a range. *Accepted when:* replay from the journal gives
  byte-identical verdicts, and any difference is reported, never silently accepted.
- **E18-14 (Proposed, not scheduled; SC)** As an agent builder, I want a denial to carry its reason
  codes and the tightest compliant alternative so that my agent can recover without guessing.
  *Accepted when:* the alternative is computed deterministically, passes the gate against the same
  state if proposed, and never adds risk beyond the denied request.
- **E18-15 (Proposed, not scheduled)** As a compliance buyer, I want a published compliance mapping
  so that I can trace each control to evidence. *Accepted when:* each control maps to a test ID and a
  journal event type, and CI fails if a mapped test or event type disappears. Its legal and
  compliance wording is reserved for the founder and counsel (DEC-79).

## Spec follow-ups (minor review findings, deferred by the freeze rule)

From the final review of mandate spec v0.3:

- Tie the loss carry to the broker account rather than `connection_id`; show the carry and its
  expiry at deployment.
- On the `disarm_ladder` confirmation screen, state that the lifetime floor becomes the only
  automated limit, as a percentage of the held position.
- Set a platform or retail minimum for `scale_lift_after_s` (at 0, stepwise lifts happen at once).
- Pace `trim_to_target` sells like discretionary exits (participation caps).
- Define whether the 1.25× floor hard level scales C × f or the remaining loss budget when L > 0.
- Harness default for `first_trade_in_instrument` (position quantity) versus the spec (no prior fill).
- Show the hard-trigger multiple and the 90-day carry window on the confirmation screen.
- Add fuzz coverage for multiple agents, trims, and owner exits.

From the independent reviews of stream J's implementation (`mandate-research`, #158 and #159):

- `reference/mandate/ref.py`'s `lineage_fold` starts from an empty lineage map and ignores the
  `lineages` input, while `fold_theses` continues from the `LineageState` it is given. No case
  exercises the difference; align `ref.py` or record the continuation in DEC-132.
- `ref.py`'s `T()` reads instants to whole seconds while the crate compares nanoseconds, so the two
  differ one nanosecond past a horizon. The crate is right; every fixture uses whole seconds.
- `ref.py`'s `_group_claimed` defaults an ungrouped instrument's group to its asset id, so a group id
  spelling that asset id claims it; DEC-132 item 12 and the merged test admit it. Align `ref.py`.
- `ResearchError::Unimplemented` is returned by no entry point, but stays until
  `crates/mandate-research/tests/rules.rs` stops constructing it (a tests correction).

From the independent review of E10-1's slice-S implementation ([#225](https://github.com/kunwarshivam/mandate/pull/225)
round 1), as the coordinator ruled there:

- `tests/document.rs`'s `two_documents_that_differ_only_in_order_hash_the_same` parses one value twice,
  and a canonical `Object` is a `BTreeMap`, so it pins determinism, not the order-independence its name
  claims. Fix it in the next tests correction that touches the file, from key-shuffled JSON text read
  through `mandate_canon::parse`.
- No live `mandate-spec` test pins an absolute version digest: both canonical tests compare against the
  same `mandate-canon` writer, so a non-canonical writer survives them. E10-3's status PR adds one
  assertion against `btc_accumulator`'s literal `sha256:9fb03f7e…` beside `mandate::version_vector`.

- `tests/vocabulary.rs::every_error_variant_has_its_own_stable_code` lacks `(ParseError::Diverged, "diverged")`.
  The code is pinned by the module test `a_mandate_changed_after_parsing_has_no_version`, but not in the
  table that asserts one code per variant. Add the row in the next tests correction that touches the file
  (#225, the coordinator's note after merge).

From E10-1's slice-V implementation (DEC-161):

- **Stream H, before E6-4's goal implementation: pair the goal tests against constants** (#260 review,
  the coordinator's ruling there). Once slice P makes `tests/goal.rs` reachable past validation, a
  constant `goal::status` answering `Ok(Done{..})` passes 2 of its 9 tests and `Ok(Running)` passes 1.
  Each needs its opposite pair so that no constant passes (#240's standard), in a tests correction
  ahead of the goal implementation.
- **`reference/mandate/ref.py`: `violates` reads a level's `null` as a limit** (#263 round 2). For
  `two_approver_above_usd` a level stating `null` should state nothing (DEC-128 item 30(b)); `ref.py`'s
  `violates` compares against it. Align the reference with the crate.
- **`PolicyOverlay::effective`'s `(Some(ceiling), Absent)` arm returns the ceiling without checking its
  type against the key** (#263 round 2, minor). A wrong-typed ceiling is refused elsewhere
  (`invalid_input`, item 30(d)); this arm should refuse it too.
- **Stream H: a gate holding a `Purpose` narrows only through `AddingPurpose::try_from`** (#263 round 2,
  nit 2). `PolicyOverlay::narrow` takes an `AddingPurpose`, so an exit's built-in AUTO cannot reach it
  (DEC-128 item 30(i)); the builder and the gate must convert with `try_from` and leave an exit's
  decision untouched on `Err`, never map an exit onto `Open` or `Increase`.
- **`ValidationContext::from_journal` (stream F, DEC-169):** implemented; its 17 tests are live.
  Stream L's E7-10 (DEC-168) maps the records to `JournaledFact`: `AccountSnapshotRecorded`,
  `ConnectionEstablished`, `ConnectionRevoked`, `DisclosureAccepted`, `AgentDeployed` and
  `MandateVersionApplied` (both `AgentVersionActive`), `UniverseChanged`, `AgentStopped`,
  `ConfigSnapshotRegistered`, `PlatformOperatorAction` (`model_withdrawn`), `MandateVersionCreated`, and
  `MandateConfirmed`. `AgentFlat` needs a source there too (the account ledger's flat-in-every-instrument
  signal). A record left unmapped is a fact the fold never sees, so the mapper's completeness is what
  covers the facts that only add (DEC-169 item 2).
- **MC-V status PR (stream F, after the E17-1 slice):** V-003, V-034 to V-037, V-039, W-006, and
  `worst_case_stop_distance` landed in their own slice (DEC-161 items 1 and 10), so all 67 MC-V cases pass
  locally; a status-only PR moves them to `passing` (DEC-77 item 3).
- **Confirmation-screen PR: the stop distance and the figures disagree on precision** (#252 review,
  minor 3). `worst_case_stop_distance` sums at a `Ratio`'s 24 places, while the four figures stop at
  `Fraction`'s 9 (`out_of_range` past it, DEC-161 item 3). A document with a 10- to 24-place stop gets a
  distance but no figures. The screen that shows both must pick one boundary.
- **Blocks any production caller of `validate`: an unmentioned envelope path reads as confirmed**
  (#252 review, minor 4; the coordinator's ruling there). `ProvenanceMap::at` defaults an unmentioned
  path to `user_entered` and confirmed, and V-020 and V-022 read only the entries present, so a
  document with no entries passes both, even with `admission: auto`. The fix lands in the
  `ValidationContext::from_journal` implementation PR (DEC-169): an unmentioned envelope path is
  unconfirmed, fires V-020, and an `auto` under it fires V-022. No production caller may use
  `validate` or `ValidatedMandate::new` until then.
- **Stream H:** `ConditionField::is_unit_bounded` and `mandate-builder`'s `well_typed` omit
  `thesis_confidence`, which §6.3 types "decimal in [0, 1]"; `validate` bounds it (DEC-161 item 5), so the
  order path's re-check is looser than the load check. Fix both with the builder's
  `oracle_is_unit_bounded` in one change.
- **`reference/mandate/ref.py`:** `PLATFORM_DEFAULTABLE` gives `leveraged_etp_disclosure_version` the value
  `None`, which the reference reads as "any value"; §7 allows only `null` (DEC-161 item 8). Give the
  reference a sentinel for "any value" so `None` can mean `null`.
- **W-005 misses a wrapped catch-all** (#238 review, round 1, minor 1). `Condition::is_catch_all` reads only
  a top-level `purpose in [increase, open]`, so `{"all": [{"field": "purpose", "op": "in", "value": ["open",
  "increase"]}]}` and `{"all": []}`, which also match every action a later rule could, warn of nothing.
  Warning-only, never blocking; widen it to any condition that holds for both purposes.
- **`validate::tests::oracle_default_allowed` returns `true` for `/environment` whatever its value**
  (#238 review, round 1, minor 1), so the V-020 property never exercises §7's `paper`-only bound there
  (`v020_reads_the_source_the_confirmation_and_the_listed_value` does). Make the oracle check `paper` in a
  later tests correction.
- The worst-case figures multiply by `Fraction`, which holds nine places, so a schema-valid fraction with
  ten or more is `out_of_range` (DEC-161 item 3). Move to an exact `Usd × Ratio` when `mandate-num` has
  one (stream H's `UsdExact` is the candidate).

From the independent review of E4-2's implementation ([#163](https://github.com/kunwarshivam/mandate/pull/163)
round 2, verdict approve), whose first two minors are closed by the third tests correction
(DEC-127 item 26) and whose third waits on another story:

- `NumError::Unimplemented` stays: it is returned by the fourteen `mandate-num::sizing` stubs E6-2 owes,
  so it is live code, not a leftover, and its row in `num::error_codes_are_stable` holds the wire
  spelling those stubs return. Drop the variant and the row together in a tests correction once E6-2's
  sizing is implemented and nothing constructs it.

From the independent review of stream H's tests PR (`mandate-builder`, [#175](https://github.com/kunwarshivam/mandate/pull/175)
round 2, verdict approve), each deferred by the freeze rule and none of them a gap in what the tests
assert:

- **Floor the accumulate goal clip per bound, not as a whole.** `properties::accumulate_coverage_reached`
  requires a goal clip; it does not require each of §8.3 step 4's three bounds to have been the binding
  one. The review measured them over 256 cases: the remaining quantity bound 151 times, the spend 7 and
  the average price 2. A floor at 2 in 256 would be flaky across seeds, so the fix is not a counter on
  its own — the `Accumulate` shape has to make each bound bind reliably first, which today it cannot,
  because it always sets `max_avg_price` to the ask and so leaves `a − max_avg × β` at zero and the
  average-price **clip** off (only its guard runs). Each bound is pinned exactly by
  `hand::accumulate_clipped_to_the_remaining_target_quantity`, `hand::accumulate_clipped_by_max_spend`
  and `hand::accumulate_clipped_by_max_avg_price`, so this buys explicit fuzz evidence, not new coverage.
- **Carry fees in the rational sizing oracle.** It takes `a = ask` and `β = 1`, so the fee arithmetic of
  §8.3 step 4 is pinned by `hand::accumulate_with_fees_counts_the_spend_and_the_quantity_received` and
  `MC-B28` rather than by the oracle. Widening the oracle means widening the generator to fee rates,
  which changes the rational magnitudes the `i128` oracle carries; worth doing deliberately.
- **The coverage counters are process-global statics.** Under `cargo test`'s shared process another
  property's cases could feed the gate. `cargo nextest` gives each test its own process and is what
  both `cargo xtask check` and CI run, so the gate is sound as used; a per-run counter would make it
  sound under either runner.

From the independent reviews of stream K's tests (`mandate-executor`, `mandate-alpaca`, #152):

- Key the reference-case partition check (`refcases::the_fixture_partition_is_driven_plus_dropped`)
  to the scopes the suite actually drives, not to the `executor` and `reconciliation` labels. Five
  of the sixteen driven entries sit outside that filter (`RC-06`'s
  `protective_orders_kept_through_dividend` is `accounting`; `RC-15` and its three variants are
  `gate`), so a new `gate`-scoped case still leaves the suite silently; renames and removals of
  every named entry are already caught (round-3 review finding 3).
- Normalize a crypto symbol `mandate-alpaca` reads back from the broker. Position paths now write
  `BTC/USD` as `BTCUSD` (`http::position_path`, the positions read and the account-wide close), but
  `wire` keeps whatever symbol a response names, so a position reported as `BTCUSD` and an order
  placed as `BTC/USD` would be two instrument ids, where trading-domain spec §2.3 makes them one.
  Only a recorded crypto position shows which form the paper host sends; normalize on
  `asset_class: crypto` before E7-3's reconciliation compares crypto positions.
- Read a working external notional order's exposure. `wire` ingests an external order placed by
  notional with `qty` equal to its `filled_qty`, so a working one understates what it can still
  buy, and its `notional` is read nowhere and is not in `record::RECORDED_FIELDS`. The exposure is
  bounded meanwhile: external activity puts every agent on the account in `exits_only` and blocks
  claiming the instrument until the owner acknowledges it (trading-domain spec §7.1), so no agent
  adds risk beside it (#195 review, round 1, finding 6).
- Make "only the fold writes folded state" a type guarantee in `mandate-executor`. `ExecutorState`'s
  fields are `pub(crate)`, so the rule that only `fold.rs` writes folded state and `step.rs` writes
  only the process-local fields (the epoch, `started`, the latest tick, the unresolved append) is a
  convention the review holds. A `FoldedState` newtype with private fields, written only through the
  fold and read through accessors, moves it to rung 1 (#194 review, round 1, finding 5).
- **E7-7, once stream E registers agent-stream payload schemas:** move `mandate-shell`'s
  committed-draft ledger from `mandate_canon::parse` to `mandate_journal::Draft::parse`, the
  oracle the brief names, and run `verify_events` over the in-module keystone's streams. Today no
  agent-stream event parses there (DEC-157 item 7; #227 review, round 1, minor 3).
- **E7-7, blocking the slice that lets the crossover drive an order:** bound the stored bars'
  staleness. Check the span's last day against the run's `setup.now` (the last completed session
  before it) and refuse coverage that ends earlier. Today `Bars::closes` reads no clock, so a
  months-old dataset is trusted and feeds the signal. That is harmless only while every downstream
  stage refuses (DEC-166; #241 review, round 1, minor 5).
- **E7-7, unowned, blocking `tests/tracer.rs::outlier_close` (PB-15):** a market-data trust rule
  that refuses a close too far from its neighbours. The shell may not judge one, because that is
  price arithmetic (DEC-138 item 3, DEC-166 item 5). The test stays pending until an owner lands the
  rule in `mandate-marketdata`, or until E6-8's mark-and-collar refuses the limit end to end
  (the coordinator's ruling on #171).
- **E7-7, when streams F and H land:** a drift check for
  `crates/mandate-shell/tests/fixtures/tracer/generate.py`, like `reference/mandate/generate.py`'s,
  so the fixture's one share at 255.20, AUTO by `rule:routine`, stays recomputed from the rules
  (#227 review, round 1, minor 4).
- **Before stream G's gate is wired in:** register `startup_reconciliation_pending` in the
  trading-domain `reason_codes` registry, or record why not. It is a third partial-gate reason code
  outside the registry, beside `instrument_not_in_universe` and `broker`, so ES-09's stable reason
  codes do not yet cover what the partial gate journals ([DEC-129](04-decision-log.md#decisions)
  items 23 and 27, ADR-0001 ES-09; #206 review).
- **E7-4 slice 1's tests correction:** close the do-nothing gap in `mandate-executor`'s generator
  properties. 29 of the 33 pass when every reachable stub returns `Ok(())`, so a no-op executor
  would satisfy them; each property must also assert a positive effect a no-op cannot produce
  (#231 review, follow-up a).
- **Before E7-3's buying-power path reads them:** the properties' model broker reports `equity` and
  `buying_power` that follow its `cash_moved`, as `cash` already does; today they stay at 20000
  whatever its fills (#231 review, follow-up b).
- Assert the ready precondition in `mandate-executor`'s properties script: after its start, an
  account has been observed and the startup `ReconciliationRun` recorded, so a script that stops
  starting ready fails at its start rather than at a later assertion; and update `play`'s doc to say
  it starts ready (#231 review, follow-up c).
- **Blocks E7-4 slice 5 (the trading day):** `mandate-executor` must copy the cross-stream facts
  journal spec §2 gives it (`AgentModeApplied` from the agent stream's `AgentModeChanged`,
  `TradingDayStarted`, `ClockAdvanced` crossing midnight America/New_York, `OwnerAcknowledged` from the
  control stream), each with its `causation_id`. Before E7-4 slice 1, `step`'s `Input::Journal(_) => Ok(())` copied
  nothing, silently, so `properties::every_copied_draft_cites_its_origin` sees no copied draft under any
  script and passes vacuously. The slice that adds the producer also adds a generator step (a clock
  advance crossing midnight New York, an owner acknowledgment) and asserts `seen > 0` on scripts
  containing it, shown failing under the do-nothing plant (#244 round 1, finding 3). Until then
  `Input::Journal` answers a loud `Unimplemented { story: "E7-4" }` naming slice 5, landing first in
  E7-4 slice 1 rather than dropping the fact (the coordinator's ruling on #244, 5861479849).
- **E7-4, the slice that reconciles protective legs (stream K):** make `mandate-alpaca`'s `wire.rs`
  keep each leg's `client_order_id` instead of reading `legs[].id` only, with its own `ready()` tests
  correction first, since `BrokerOrder.legs` changes type (#229's pattern). E7-4 slice 1 aligns
  `ClientOrderId::for_protection` to the §2.3 grammar (`{entry}-p{protection}`, legs `-tp` and `-sl`)
  and reads no leg id from a `BrokerOrder`, which an in-module test pins. Until the wire change lands,
  a broker-reported leg is attributed by the single holder or fails closed for openings; exits are
  untouched ([DEC-160](04-decision-log.md#decisions) 3a, #243 round 1, the coordinator's ruling (b)
  on #174, 5861764910).
- **E7-4 slices 2 and 3 (stream K):** `properties::protective_sell_quantity_never_exceeds_the_position_in_any_script`
  wants the `ProtectionChanged placed` at or after the entry's completion with no lag. That is right
  on the normal path (§5.4's legs activate at completion), but a re-placement after a
  cancelled-then-filled entry may lag by up to `max_unprotected_s`. If a slice turns it red there,
  allow that bound rather than loosening the assertion elsewhere (#244 round 3, minor 3).
- **`mandate-executor` fees (stream K), from #259 round 1:** (1) a typed `Environment` in place of
  the stream's environment text, so `paper_only_fee_config` refuses a live stream by its type
  (rung 1) rather than by a string comparison; (2) `mandate_accounting::Config` carries the
  schedule's `effective_from` and refuses to price a trade date before it, where today
  `fee_config` validates the date only against the calendar's range and then drops it;
  (3) the ruling's flat per-order cost for the paper-only overestimate (#174, 5861904579) cannot be
  represented, because `EquityFees` has no per-order field, so the overestimate is carried by the
  per-share and rate figures, each at least ten times the transcribed schedule.
- **E7-4:** gate `mandate-executor`'s `resubmit` for an order with no `intent_id`. It sends again without running the gate; no slice through 6 writes such an order, but protective orders will, so it must be gated before they ship (#202 review, the coordinator's ruling, comment 5857629810).
- Fold `crypto_status` in `mandate-executor`. `AccountStateObserved` journals it, and §7.3 requires it `ACTIVE` for crypto orders, but the fold keeps no field for it until the gate's crypto check reads one; the journal holds it, so the fold can add it without a new event (#198 review, round 1, finding 8a).
- Give a §7.3 account restriction in `mandate-executor` a lift path. §7.3 says a detected
  restriction stands "until the owner acknowledges and the account is refreshed", but
  `state::restriction_for` only yields `reconciliation:{subject}`, so no `OwnerAcknowledged` can
  name `account_trading_blocked`, and a later `ACTIVE` read leaves `account_state` `Blocked` and the
  restriction in place. Pre-existing on main; the cash slice is the first to raise it from every
  reconciliation run (#205 review, round 2, minor 3).
- Pin or drop the two unreachable overflow sites in `ExecutorState::buying_power`: the reservations sum and the final `min(model, broker) − reserved`. `Usd` is signed, so each fails only at the decimal range, which no reservation reaches, and replacing either `None` with zero passes every test; the reachable site, the model's cash, is pinned (#198 review, round 2, finding 3).
- **Blocks running the executor across a session boundary:** fold `TradingDayStarted` and
  `RiskDayStarted` in `mandate-executor`. Since #194's round 1 both answer the later slice's
  `Unimplemented` stub, so the first day rollover stops the executor, failing closed. The slice that
  owns the day fold (protection re-placement at the GTC buffer day, §5.4) must interpret both, move
  them back into `properties::INTERPRETED` with live tests that fail when either arm is stubbed, and
  land before the executor runs across a session boundary (#194 review, round 2).
- Report a safety-critical function whose only mutants are unviable. `ci mutants` counted the one
  mutant of `mandate-executor`'s `every_agent` (a body of `Ok(Default::default())`, which does not
  compile because `EventId` has no `Default`) as unviable, so "0 missed" said nothing about the
  function that enforces §7.1's `exits_only`. Such a function needs a named live test, and the gate
  or the review checklist should list every function whose mutants are all unviable, so a reviewer
  names that test (#196 review, round 1, finding 2).
- Question for the spec owner: does §10's simulated regulatory fee apply to an **unattributed**
  paper equity fill (external activity, §7.1)? §10 books paper's regulatory fees in the shadow
  ledger without distinguishing, and an external fill has no client order for DEC-87's per-order
  TAF cap. Since #196 the executor books one only for a fill attributed to one of its orders,
  stated in `orders::simulated_fee`'s doc; booking it too would lower paper buying power, the
  conservative side (#196 review, round 1, finding 7).
- Pin the calendar roll of the simulated `FeesCharged` event's `day` in `mandate-executor`. The
  hour cutoff is pinned (a fill at 20:00 New York belongs to the next trade date), but every fixture
  fill trades on a Tuesday, so `first_on_or_after(date, is_trading_day)` is the identity and a
  `day` taken straight from the broker's `trade_date`, bypassing the account's calendar, passes the
  suite. A fixture with a Saturday or holiday `trade_date` pins it (#196 review, round 2, finding 1).
- Reconcile journal spec §9's "daily snapshot" with `AccountSnapshotRecorded`'s cadence in
  `mandate-executor`: since the cash slice every reconciliation run that has a base records one
  (at startup, at each `Unknown`, at session boundaries, at fee postings). Either the spec line
  names every run, or the executor records the daily one only and carries the comparison's base
  another way (#205 review, round 1, finding 7a).
- Give `AccountSnapshotRecorded` one shape in `mandate-executor`: a fee posting with no cash base
  records it through `fees()`'s fallback without `model_cash`, `cash_band` or `cash_in_band`,
  while a run with a base records all three (#205 review, round 1, finding 7b).
- Answer an underflow in `mandate-executor`'s `asset_fees` with a typed error: a posted crypto asset
  fee larger than the one held is folded as `checked_sub(charged).unwrap_or(Qty::ZERO)`, which errs
  strict but silently, where the crate's convention is a typed error (#205 review, round 1,
  finding 7c).
- Wire the gate's buying-power port in `mandate-executor`. `ExecutorState::buying_power` and
  `crypto_buying_power` are computed and pinned but only tests read them; the gate still checks no
  buying power. The port must read the equity row for an equity order and the crypto row for a
  crypto order, and treat `None` (no account reported, an incomplete report, or an overflow) as no
  buying power, failing closed (`AGENTS.md` rule 3; #205 review, round 1).
- Make the reproduction line `ci pending` prints runnable as printed: quote the `-E` filterset
  (it contains parentheses, so a shell refuses it) and add `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`,
  which the gate sets and nextest requires for libtest JSON. Both fail loudly today rather than
  giving a different verdict (#199 review, round 2, minor).

From the independent reviews of stream I's implementation (`mandate-runtime`, #151), each deferred by
a coordinator ruling rather than left undone (DEC-131 item 25):

- Match a kill switch's lift to the acknowledgment **of that switch**. A confirmed `RiskLimit` switch
  is lifted today by a looser account-stream copy caused by any `OwnerAcknowledged` the fold has
  seen, because `KillSwitchActivated` does not name the limit that pulled it. When it carries its
  limit, the lift must match the acknowledgment's subject to the switch (round-4 review finding 1).
- Give a kill switch pulled while the agent is already `paused` a way to lift. It writes no
  `AgentModeChanged`, so it has no `mode_event`, nothing on the account stream can confirm it, and
  only a new deployment clears it. It fails closed, but two limits in one cycle then keep the agent
  paused until a redeploy (round-4 review finding 2).
- Shrink `outstanding` on a terminal account-stream outcome (`GateDecided` denial, `OrderAbandoned`,
  a terminal order state), which needs the executor's `client_order_id`-to-intent mapping from M6.
  Until then a restart after any order waits one clean `ReconciliationRun` before the agent can trade
  or propose a discretionary exit (DEC-131 item 25(d), round-2 review minor 3).
- Record an approval response that arrives in the same step as the tightening which cancels its
  approval as `refused` rather than `recorded`; nothing is handed either way, so it is record
  accuracy (DEC-131 item 25(j), round-1 review finding 9).
- Act on a granted ASK. `PendingApproval` binds the instrument, version, deadline, and whether the
  action adds risk, and carries no order body, so the runtime records a response and re-proposes at
  the next evaluation rather than placing the bound order; the bound content is M7's (DEC-131 item
  25(a)).

From the independent review of stream G's mandate limits (`mandate-risk`, #160):

- Give `Computed` the account's own figures (`account_gross`, `account_equity`), so an account-1×
  `gross_exposure_limit` denial journals the figures it compared; today it carries none and is told
  apart from the agent-limit denial only by `computed.gross` being unset. It is an addition to
  #136's public API, so it needs its own ruling.
- Settle what DEC-129 item 2's "`computed` blocks included" means: `ref.py`'s keys only, or every
  figure the gate computed. The flat `Computed` also reports `order_usd`, `instrument_total` and
  `cap` on branches whose reference block omits them, and `compare_computed` checks only the keys a
  case states, so nothing asserts either reading.
- Give §3.3's "organization ceiling" on the per-instrument cap a `GateConfig` field and an owning
  story; check 2 has no value to bound the mandate's cap with today.
- Pin that the **agent's** gross exposure counts only its own working openings
  (`limits.rs`'s `working_cost(input, None, false)`): the substitution `false` → `true` survives
  every live test (#160 round-2 finding 4). It is conservative, since the whole-account sum is a
  superset and can only deny an opening earlier, so it is not a rule-1 breach; one assertion in
  `the_account_bound_counts_other_agents_working_orders` would close it.
- Decide what `agent_flatten` does when `max_exit_offset = 1` (#176 round-1 nit 3): the floor
  `bid × 0` fails `positive()`, so the owner's kill switch returns `GateError` where `ref.py` plans a
  floor of `0`, the one way the function can refuse an owner kill switch (AGENTS.md rule 13, "the
  kill switch is always available"). Unreachable today, since §5.6's fixed tier table sets the
  offset at 0.03 or 0.05 and no code path sets it to 1; a fix either bounds the offset below 1
  where it is configured or plans the flatten without a floor.
- Correct the E6-3 brief's test names (#176 round-1 nit 4): the clause table (line 583) and mutant
  row 33 name `properties::an_agent_flatten_never_touches_another_agent`, which lives in `hand.rs`.
- Retag `mc_g13` in `crates/mandate-risk/tests/refcases.rs` from `pending E6-3` to `pending E6-8`, in
  the next tests correction that touches the file. E6-3 has landed; the test now stops at E6-8's
  fail-closed stub (DEC-129 item 29), and `ci pending` accepts any story's stub, so the stale tag
  misnames what it waits on without failing the gate (#217 review, round 1, minor 2).
- **E6-6 slice 2:** fold the `legacy_pdt` `DayTradeLedger` account-wide in `mandate-risk` from
  every agent's fills on the account (§9.2's window of today plus four prior trading days, shares
  held overnight sold first, each same-day open-then-close once, crypto never, fractional counted;
  DEC-129 item 6), and interpret RC-09's and RC-09B's `regime`, `prior_day_trades`,
  `last_equity`, `multiplier` and `day_trade_count` in `mandate-refcases`. Slice 1 (#221) reads the
  ledger as an input the caller folds, so until slice 2 lands an agent-scoped ledger would
  undercount the account's day trades (#221 review, round 1, minor).

From the independent review of E6-2's autonomy slice ([#216](https://github.com/kunwarshivam/mandate/pull/216)
round 1, verdict approve; minor 2, deferred by the coordinator's ruling):

- **V-023 at load must bound a decimal's precision (stream F).** The schema's `decimal` admits 28
  fractional digits and `Ratio` holds 24, so a rule comparing `order_usd` against a 26-digit value
  passes `mandate-builder`'s `check_rules`/`well_typed` re-check and is refused mid-walk by
  `Condition::matches` with `too_precise`. It is still refused, so rule 3 holds, but DEC-152 (1)
  promises the whole rule set is re-checked before any rule is read. Stream F's V-023-at-load in
  `mandate-spec::validate` refuses such a value up front (landed with E10-1's slice V, DEC-161 item 5),
  and the order path's `well_typed` gains the same bound so both report it by name.
- **E6-6:** drop or pin the `at.opening_auction` clause in `mandate-risk`'s `market_orders_barred`.
  The opening auction is always pre-market, which the clause for a US equity outside the regular
  session already bars, and crypto never has an auction, so the clause changes no decision and no
  test can catch its removal (#228 review, round 1, minor 2; E6-6's bug list when it lands).
- **E6-6:** pin the rest of a re-priced exit and of the close window. The tests assert
  `marketable_limit_required` and the quantity of a market exit re-priced in an auction window but
  not its `limit_price` or `applied`, and `hand::the_close_window_follows_the_early_close_calendar`
  asserts only times inside the window, so it passes against a `close_window` that is always true
  (#228 review, round 1, minor 3; E6-6's bug list when it lands).

From E6-2's builder slice (stream H; found while implementing §8.3, not by a review):

- **§8.3 step 2's "whole position (minus working exits)" has no input (stream H, with stream I).**
  `AccountSnapshot` carries no working exit quantity, so `propose` sells the whole position, as
  `reference/mandate/ref.py` does, and a second discretionary exit while one is working is proposed at
  the full quantity. Until the field lands, the backstop is the risk gate's trading spec §5.3 rule 4
  (`sell_exceeds_available`, merged in #221), which binds every reduction and denies the over-sell,
  so the builder cannot turn a position short on its own. The field arrives by a DEC-77 tests
  correction ahead of the slice that consumes it (every `AccountSnapshot` literal in
  `crates/mandate-builder/tests/` names it); that slice then subtracts working exits in
  `discretionary_exit` and holds when nothing is left to sell (#234 review, round 1, M1).
- **The runtime filters model outputs by instrument before `combine` (stream I).** `combine` refuses
  the whole call on an output for another instrument (`output_instrument_mismatch`, DEC-130), the exit
  branch included, so handing it a tick's unfiltered output buffer would stop a discretionary exit
  the way a crossed quote did before #234 moved that refusal to the buy path. E6-1's evaluation loop
  passes only the outputs whose `instrument_id` is the instrument being sized, with a test that a
  foreign output in the buffer never blocks an exit (#234 review, round 1, m2).
- **`BuilderError::Unimplemented` and `NumError::Unimplemented` are now constructed by nothing.**
  Both stay because `tests/vocabulary.rs` and `num::error_codes_are_stable` pin their codes, and an
  implementation PR may not edit a test. Drop each variant with its row in the next tests correction
  that touches those files (E6-2; the `NumError` half is the E4-2 row above).

From E6-4's V-040 spec change (stream H; the coordinator's ruling on #251, round 1):

- **Add V-040's boundary pair to `mandate.yaml`, with the harness counts, in one approved change
  (founder).** The rule's boundaries are two ladders: factors of 12 places in total (valid) and 13
  (V-040). The 2¹³ × 5¹³ ladder, where the whole product fits and a subset does not, belongs there
  too. For now all three are pinned in `reference/mandate/fuzz.py::fuzz_ladder_precision`, which runs
  on every seed, and as in-module rows in `crates/mandate-spec/src/validate/tests.rs`. They are not
  reference cases because a case changes counts that live `mandate_harness.rs` tests assert (298
  cases, 67 semantic, 202 owned, and the member sweeps), and the spec guard keeps the fixture and
  those tests in separate PRs. The founder-owned YAML (ES-22) and the counts must change together.
- **Three minors from #251's round 2, deferred by the freeze rule.** (1) `docs/specs/mandate.md`'s
  front matter still says a change needs founder approval with no qualification; DEC-167 item 3
  records that a stricter V-rule is agent-accepted under DEC-79, and the front matter should say so
  in one clause. (2) `fuzz_ladder_precision` draws 1 to 4 scale rungs plus two fixed ones: it never
  reaches the five-rung maximum that item 3's "five 2-place rungs still fit" relies on, and at four
  it builds a six-rung ladder `maxItems: 5` forbids. Draw 1 to 3 scale rungs, and pin the
  five-rung, 2-place ladder. (3) The function imports `combinations` inside its body; move the
  import to the top of `reference/mandate/fuzz.py`.
