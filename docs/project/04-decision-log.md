# Decision Log

| | |
|---|---|
| **Owner** | Project management |
| **Status** | Living document |

Status values: **Accepted**, **Proposed** (recommended, awaiting confirmation), **Open**
(not yet decided), **Superseded** (replaced by a later decision). Changing an accepted decision requires a new entry that supersedes it.

## Decisions

| ID | Decision | Status | Rationale | Alternatives considered |
|---|---|---|---|---|
| DEC-01 | Build a production platform where users deploy autonomous agents on their own accounts | Accepted | Founder direction; software business, no custody | AI-native fund trading own capital; research copilot |
| DEC-02 | Rust for the core (runtime, risk, execution, connectors, journal); Python for research, model tooling, SDK | Accepted | Speed and safety on the hot path; Python ecosystem for research | Python-only; C++ |
| DEC-03 | The mandate is the contract: agents cannot act outside the approved spec | Accepted | Core trust promise | Prompt-level instructions only |
| DEC-04 | LLMs produce opinions; deterministic code decides sizing and places orders | Accepted | Auditability; resistance to prompt injection; speed | LLM makes the final call (TradingAgents style) |
| DEC-05 | Reducing risk never needs approval; increasing risk beyond limits always does | Accepted | Asymmetric safety; enables autonomy | Approve everything; approve nothing |
| DEC-06 | Timeouts and ambiguity resolve to a safe default that never adds risk | Accepted | Fail-safe behavior | Timeout executes the proposed action |
| DEC-07 | Journal intent before sending any order; idempotency keys; event-sourced recovery | Accepted | No duplicate orders; full audit | Best-effort logging |
| DEC-08 | One process per agent deployment | Accepted | Isolation, independent lifecycle | Many agents per process |
| DEC-09 | Organization → Workspace hierarchy; workspace is the tenant; billing per organization | Accepted | Founder requirement; clean isolation boundary | Flat accounts |
| DEC-10 | Thin global control plane; sensitive services run with the data plane; three deployment modes from one installer | Accepted | Strategy and trading intent stay on the customer's site; on-prem as packaging | Full hosted control plane with edge data plane only |
| DEC-11 | Notifications carry only opaque IDs; approval details load from the workspace deployment | Accepted | Privacy of trading intent | Full details in notifications |
| DEC-12 | Never price per trade or on assets or profits | Accepted | Regulatory posture; neutral incentives | Per-trade fees; performance fees |
| DEC-13 | Exchange testnet first, crypto perpetuals first | **Superseded by DEC-23** | Free, 24/7, safe onboarding. Superseded because the major perpetuals venues exclude US persons | US equities first via Interactive Brokers |
| DEC-14 | Build Phase 0 core from first principles, piece by piece | Accepted | Founder direction; deep understanding of trading mechanics | Start directly on NautilusTrader |
| DEC-15 | Working name "Mandate" | Accepted | Matches the core concept; finance-native | Delegate, Autopilot, Principal |
| DEC-21 | Build with the founder plus AI coding agents; no hires planned for v1. The founder approves all merges and holds all production secrets | Accepted | Founder direction; speed and cost | Hire a core engineering team |
| DEC-22 | Serve the United States first | Accepted | Founder direction; largest market | Global crypto venues first |
| DEC-23 | First connector: **Alpaca** (US stocks, ETFs, crypto spot; paper trading; OAuth connections). Second: **Kraken Derivatives US** (CFTC-regulated crypto perpetuals). Later: Coinbase US futures, Interactive Brokers | Accepted | Alpaca: free paper trading on the same API as live, four asset classes in one integration, OAuth for third-party apps, market data through each user's own account. Kraken: the CFTC-regulated perpetuals venue for US users with a self-service demo environment | Kraken perpetuals first; Coinbase first (static sandbox only); Interactive Brokers first (heavier onboarding) |
| DEC-24 | v1 asset scope on Alpaca: US stocks, ETFs, and crypto spot; options later | Proposed | Keeps v1 accounting and risk to spot instruments | Include options in v1 |
| DEC-25 | Trading domain conventions: average-cost P&L; fees recorded separately in USD (asset-denominated fees valued at fill price); backtests on raw prices plus explicit corporate actions; cash accounts use settled cash only; v1 leverage capped at 1× | Accepted (refined by DEC-27; shorting superseded by DEC-32, no short sales; margin buying power by DEC-34) | Simple, auditable, and cannot create account violations by construction ([trading domain spec](../specs/trading-domain.md)) | FIFO P&L; adjusted-price backtests; margin in v1 |
| DEC-26 | One serialized account ledger per broker account; one agent per instrument per account; activity not originated by Mandate switches all agents on the account to exits-only | Accepted | Buying power, orders, and positions are account-level; prevents agents overspending or crossing each other | Per-agent ledgers without coordination |
| DEC-27 | Store signed cost basis, not average cost (average derived); reductions rounded half-even to 12 places | Accepted | Exact conservation; identical results across Rust and Python | Stored average cost |
| DEC-28 | Protective exits via OCO and bracket orders (tranche model), with defined exit and kill-switch sequences | Accepted | Alpaca rejects self-crossing orders but exempts OCO and bracket orders; plain stops block other exits | Plain stop orders with cancel-and-replace only |
| DEC-29 | Opening and increasing orders are limit orders only, within a price collar; market orders only for risk-reducing exits in the regular session | Accepted | Bounds fill prices, so buying power and no-debit rules hold | Market orders for entries |
| DEC-30 | Overnight session disabled for v1 agents | Accepted | Thin liquidity, delayed data on free plans, next-day trade dates, no stop protection | Enable overnight trading |
| DEC-31 | Instrument eligibility floor (exchange-listed, no OTC or IPO-day, price and liquidity floors, leveraged/inverse ETPs only with opt-in) and gate-enforced market-conduct controls | Accepted | Prevents manipulation-like patterns and outsized losses from autonomous flow | Monitoring only |
| DEC-32 | No short sales in v1 | Accepted | Removes Reg SHO, borrow, and margin-deficit exposure | Shorting with safeguards |
| DEC-33 | Mandate never originates trade ideas (inferred fields inactive until confirmed; approvals show the mandate basis, not platform-authored alternatives); trading records retained at least 6 years | Accepted (wording refined by DEC-38) | Protects the software-only posture; supports users' recordkeeping and the platform's defense | Configurable retention without a floor |
| DEC-34 | Margin-account buying power uses total cash (settled + unsettled) ≥ 0; settled-only applies to cash accounts; backtests use the same rule | Accepted | Matches Alpaca at 1× (buying power = cash); using unsettled proceeds is permitted in margin accounts | Settled-only for all accounts |
| DEC-35 | Paper trading uses a defined IEX data profile; live equity agents require consolidated (SIP) data on the user's Alpaca account | Accepted | Paper and free plans receive IEX only; collars and risk marks need a sound reference quote | IEX for live; SIP everywhere |
| DEC-36 | Crypto protection is one simple GTC stop-limit for the whole position; take-profit handled by the runtime; adds and exits use cancel → confirm → act → re-place | Accepted | Alpaca crypto supports only simple orders (no OCO or bracket) | No crypto protection |
| DEC-37 | No opening orders outside the regular session in v1; exits allowed | Accepted | Bracket orders do not support extended hours; stops do not trigger outside regular hours | Extended-hours openings |
| DEC-38 | Principle wording: "Mandate does not choose instruments, strategy, sizing, or limits; these come from user-confirmed mandate fields; advisors are tools the user selects." Templates never ship with platform-chosen instruments or parameters | Accepted | The earlier "never originates a trade idea" overstated, given platform advisors and templates | Keep DEC-33 wording |
| DEC-39 | Each agent has a capital allocation (Σ ≤ account equity); agent equity = allocation + agent P&L at risk marks; allocation changes adjust equity, high-water mark, and day-start equity so they never count as P&L | Proposed | Limits need a per-agent base when several agents share one account; risk marks keep limits conservative | Limits on account equity only; allocation as a hard cash reservation |
| DEC-40 | Risk day starts 00:00 America/New_York for all asset classes; a daily-loss breach puts the agent in `exits_only` until the next risk day, lifted automatically | Proposed | One boundary for equities and 24/7 crypto; automatic lift matches the limit's "daily" meaning | Per-session day; owner acknowledgment to lift |
| DEC-41 | Drawdown from the high-water mark with a ladder (`scale_sizes` with hysteresis, `exits_only`, `flatten_and_pause` at `max_drawdown`); `exits_only` and `flatten_and_pause` lift only by owner acknowledgment with step-up | Proposed | Graduated response before a hard stop; hysteresis prevents flapping; severe states need a human | Single hard stop; drawdown from allocation instead of the high-water mark |
| DEC-42 | Autonomy: built-in AUTO for reduce, close, and protective actions; then ordered first-match rules; then the default (`ask` unless the user confirmed `auto`); approval timeout always skips | Proposed | Enforces DEC-05 structurally; first-match is predictable; skipping on timeout is the safe default | Scored rules; timeout executes; per-rule timeouts |
| DEC-43 | Mandate version = SHA-256 of the canonical JSON; every change is classified risk-increasing, risk-reducing, or neutral (unknown changes are risk-increasing); risk-increasing changes need step-up | Proposed | Deployments pin exact contracts; fail-safe classification keeps loosening deliberate | Semantic version numbers; step-up for every change |
| DEC-16 | Restate for durable execution | Proposed | Single Rust binary; runs on the edge | Temporal |
| DEC-17 | NATS JetStream for messaging | Proposed | Account model maps to workspaces; single binary on the edge | Kafka / Redpanda, Redis Streams |
| DEC-18 | OIDC SSO in v1; SAML and SCIM later | Proposed | Covers Google, Okta, Entra for design partners | SAML from day one |
| DEC-19 | v1 approval channels: web push, email, one chat; SMS and phone next; native mobile later | Proposed | Fastest path; avoids app-store dependency | Native mobile app in v1 |
| DEC-20 | Declarative mandates only in v1; WebAssembly plug-ins later | Proposed | Safety and scope | Plug-ins in v1 |

## Open decisions

| ID | Question | Needed by | Inputs |
|---|---|---|---|
| ~~OD-01~~ | ~~First exchange~~ | — | **Resolved by DEC-23** (Alpaca first, Kraken Derivatives US second) |
| OD-02 | First fast decision model: Laya (self-hosted) or Jev (hosted, early access) | Phase 3 (P1 in v1) | Bake-off on labeled financial decisions: accuracy, calibration, latency, cost |
| OD-03 | Adopt NautilusTrader connectors later, or keep our own | Phase 3 | LGPL review; connector coverage |
| OD-04 | Minimum paper-trading duration before live | M9 | Design-partner feedback; risk appetite |
| OD-05 | Mobile access to on-site approval services: customer VPN, end-to-end encrypted relay, or both | M10 | Design-partner security requirements |
| OD-06 | Minimum identity data held by the global control plane in hybrid mode | M8 | Privacy review; billing needs |
| OD-07 | Whether Kraken's self-service demo environment covers the US (Bitnomial-listed) perpetual contracts, or a UAT account is needed | Phase 3 | Kraken documentation and support |
| ~~OD-08~~ | ~~Alpaca day-trading regime; cash accounts~~ | — | **Resolved:** Alpaca applies the intraday margin rule since June 4, 2026; all Alpaca accounts are margin accounts (1× below 2,000 USD equity or by setting) |
| ~~OD-09~~ | ~~Fee rates and rounding~~ | — | **Resolved in approach:** SEC (sells), TAF (sells; cap per execution by default, per order configurable pending spec §15 Q6), CAT (buys and sells), accrued per fill and charged as a daily total rounded up; rates transcribed from Alpaca's fee schedule into effective-dated configuration at M3 |
