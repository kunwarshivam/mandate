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
  replaced by a rule that reads the cause; the planted cases of #172's reviews all fail the gate;
  and `mandate-executor`'s three E7-4 properties that pass today only on a discarded case's stub
  report get their rows in the same change (see the row under #196's reviews).

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

- **E8-1 (Must)** As an approver, I want requests with the proposed action, alternatives,
  evidence, risk impact, deadline, and default so that I can decide quickly.
- **E8-2 (Must)** As an owner, I want timeouts to apply the safe default so that silence never
  adds risk.
- **E8-3 (Must)** As an owner, I want approved actions re-validated for drift so that stale
  approvals are not executed blindly.
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
- **E10-6 (Should)** As an owner who already runs my own agent (for example Claude), I want to
  connect it through a Mandate MCP server that exposes the same API Mandate uses to take my input,
  so that my agent can work inside my mandate without a separate path around it
  ([DEC-141](04-decision-log.md#decisions)). Follows the owner-input API work, after the tracer
  bullet (E7-7); no change to milestone order. *Accepted when:* every client request passes through
  the same order builder, autonomy rules, risk gate, account ledger, and journal as the owner's own
  input; the client cannot change an envelope field (it may only propose a mandate version that the
  human confirms with step-up); ASK approvals go only to the human owner, and a client cannot approve
  its own proposal; owner-only privileges (owner exits outside the regular session at a confirmed
  bid, Stop and release, the kill switch) stay with the human; the client authenticates with its own
  scoped, revocable token and no broker credential crosses MCP; every client call is journaled with
  the client's identity; and the owner can revoke the client at any time.

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
- **Blocks E7-4 (the first `AccountWideScope` constructor):** encode a crypto pair's `/` where
  `/v2/positions/{symbol}` is built (`HttpRequest::close_position` and the positions read). `wire`
  holds a broker symbol to one or two segments of letters, digits and `.` (slice a2), so nothing
  hostile reaches the path, but `BTC/USD` still builds two segments where Alpaca takes `BTCUSD` or
  `BTC%2FUSD`. Since #195 the allowlist refuses both the read and the close, and a refused close
  answers `NotSent`, never a broker's rejection, so a crypto close fails loudly rather than going
  to the wrong path; until this lands the account and workspace kill switches cannot close a crypto
  position by close-position, so E7-4 must not construct an `AccountWideScope` before it (#191
  review, round 3; #195 review, round 1, finding 1).
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
- With E1-3, give `mandate-executor`'s three E7-4 properties `BEHAVIOUR_ONLY_TESTS` rows
  (`protective_sell_quantity_never_exceeds_the_position`,
  `every_unprotected_interval_has_a_journaled_start_and_end`,
  `no_interval_exceeds_the_limit_without_an_alert`). Since #196 their shrunk failure is their own
  E7-4 assertion (the protected lead has no bracket). The gate accepts them only because cases
  discarded before shrinking stop at a later E7-3 slice's stub, and `names_a_stub` reads the whole
  output. Since #199, `ci pending` pins their seed (`PENDING_PROPTEST_SEED`), so that verdict is one
  answer rather than red on some runs, but it still rests on incidental evidence. A row now would be
  reported as not needed, because the output names a stub. When E1-3 reads the failure's own
  cause, these three fail away from the stub and need their rows, expiring with E7-4 (#196 review,
  round 1, finding 5; #199 review, round 1, finding 4).
- Pin the calendar roll of the simulated `FeesCharged` event's `day` in `mandate-executor`. The
  hour cutoff is pinned (a fill at 20:00 New York belongs to the next trade date), but every fixture
  fill trades on a Tuesday, so `first_on_or_after(date, is_trading_day)` is the identity and a
  `day` taken straight from the broker's `trade_date`, bypassing the account's calendar, passes the
  suite. A fixture with a Saturday or holiday `trade_date` pins it (#196 review, round 2, finding 1).
- **Decide before E7-3's cash slice:** how an asset-denominated crypto fee is journaled. Two pending
  E7-3 tests fold `FeesCharged` with `family: "crypto_asset"`, no `day`, and an `instrument`:
  `hand::unposted_crypto_asset_fees_explain_the_crypto_difference` and the RC-07 harness reached
  by `refcases::trading_domain_rc_07_unposted_crypto_fees_reconcile`. `crypto_asset` is a `FeeKind`
  payload name, not a `FeeFamily` (`Equities | Crypto`), and `mandate-refcases`'s reference
  implementation refuses it as an unknown fee family, so both die on the fold. Unlike `sec_31` (#197)
  the fix is not a literal swap: §6.3 says asset-denominated fees are not accrued liabilities, which
  suggests that path should not journal a `FeesCharged` at all. Settle the event first (the
  coordinator's call, with a decision-log row), then correct both tests in one reviewed
  tests-correction PR so E7-3's implementer isn't blocked (#197 review, round 1, finding 1).
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
