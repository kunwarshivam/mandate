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
| E15 Advisors: LLM and fast models, calibration | Phase 3 | 6.3, 6.5 | Should |
| E16 Kraken Derivatives US connector | Phase 3 | 6.2 (FR-2.5) | Should |

## Stories

### E1 Foundations

- **E1-1 (Must)** As an engineer, I want a Rust workspace and Python package with CI so that
  every change is built, linted, and tested.
  *Accepted when:* CI runs build, tests, and lint on every push; main is protected.
- **E1-2 (Must)** As an engineer, I want an ADR template and coding conventions so that
  decisions and code stay consistent.

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

### E6 Agent runtime and risk

- **E6-1 (Must)** As an operator, I want an agent to run a mandate continuously so that it
  trades without supervision.
- **E6-2 (Must)** As an operator, I want every proposed action classified AUTO, ASK, or DENY
  per the mandate so that autonomy matches my rules. *Accepted when:* mandate reference cases
  MC-A01 to MC-A09 and MC-D01 to MC-D08 pass.
- **E6-3 (Must)** As an owner, I want an independent risk gate enforcing all limits so that no
  agent logic can exceed them.
  *Accepted when:* simulation fuzzing across random market paths and mandates never produces
  an order outside limits; mandate reference cases MC-G01 to MC-G07 pass.
- **E6-4 (Must)** As an owner, I want a daily-loss limit and a drawdown ladder so that losses
  trigger automatic de-risking. *Accepted when:* MC-R01 to MC-R03 pass.
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
- **E7-5 (Must)** As an owner, I want one account ledger per broker account and one agent per
  instrument per account, so that agents never overspend or cross each other.
  *Accepted when:* RC-17 passes; external activity switches agents to exits-only (RC-15).

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

### E10 Mandate authoring

- **E10-1 (Must)** As an operator, I want to describe an agent in plain language and get a
  compiled mandate with inferred fields highlighted. *Accepted when:* compiled mandates validate
  against the [mandate spec](../specs/mandate.md) (schema, V-rules, policy hierarchy; reference
  cases MC-S, MC-V, and MC-P pass).
- **E10-2 (Must)** As an operator, I want to edit the mandate as a form or YAML, kept in sync.
- **E10-3 (Must)** As an operator, I want mandates versioned with viewable diffs, and changes that
  increase risk to require step-up. *Accepted when:* the version vector and MC-C01 to MC-C14 pass.
- **E10-4 (Must)** As an operator, I want going live to require a backtest, a paper run, and
  step-up approval.
- **E10-5 (Should)** As a new user, I want templates for common mandates.

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

### E15 Advisors: LLM and fast models, calibration

- **E15-1 (Should)** As an operator, I want an LLM research advisor that writes theses
  asynchronously without blocking trading.
- **E15-2 (Should)** As an operator, I want a fast decision-model advisor with a hard deadline.
- **E15-3 (Should)** As an operator, I want advisor confidence calibrated against outcomes and
  shown in scorecards.
- **E15-4 (Could)** As an operator, I want shadow mode for a new mandate version.

### E16 Kraken Derivatives US connector (Phase 3)

- **E16-1 (Should)** As an operator, I want to connect a Kraken Derivatives US account (demo and
  live) with a trading-only key so that agents can trade CFTC-regulated crypto perpetuals.
  *Accepted when:* keys with withdrawal or transfer permissions are rejected before storage.
- **E16-2 (Should)** As a trader, I want perpetuals accounting (funding every eight hours,
  margin, liquidation thresholds) so that perpetual P&L and risk are correct.
- **E16-3 (Should)** As an operator, I want a funding/carry advisor for perpetuals.

## Won't (v1)

Retail launch; users outside the US; options; Interactive Brokers and Coinbase connectors; native mobile apps; WebAssembly plug-ins; SAML and SCIM;
fully on-prem control plane; shared intelligence plane; strategy marketplace.
