# Task: E6-3 and the risk gate (`mandate-risk`)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15), for **stream G** of the
M5 work graph. It covers the whole of the `mandate-risk` crate, because the gate is one function
and its stories are its checks: splitting them across briefs would hide the one thing that matters,
the order in which the checks run. Each story keeps its own tests, pending marker, implementation
PR, and reference cases.

## Story

- **Stories:** E6-3, E6-4, E6-6, E6-7, E6-8, E6-9
  ([backlog](../06-backlog-v1.md#e6-agent-runtime-and-risk)).
- **Acceptance criteria (verbatim):**
  - **E6-3** "As an owner, I want an independent risk gate enforcing all limits so that no agent
    logic can exceed them. *Accepted when:* simulation fuzzing across random market paths and
    mandates never produces an order outside limits; the mandate invariants MI-1 to MI-11 hold
    under property-based tests; mandate reference cases MC-G01 to MC-G13 and MC-F01 to MC-F04
    pass."
  - **E6-4** "As an owner, I want a daily-loss limit and a drawdown ladder so that losses trigger
    automatic de-risking. *Accepted when:* MC-R01 to MC-R15, MC-T01 to MC-T05, and MC-L01 to MC-L09
    pass."
  - **E6-6** "As an owner, I want US account rules (day-trading regime, settlement, short sales,
    market hours) enforced by the risk gate so that agents never get my account restricted.
    *Accepted when:* simulation tests for each rule pass; blocked orders are journaled with the
    rule."
  - **E6-7** "As an owner, I want an instrument eligibility floor (exchange-listed, no OTC or
    IPO-day, price and liquidity floors, leveraged ETPs only with opt-in) so that agents stay in
    liquid, suitable instruments. *Accepted when:* RC-16 passes."
  - **E6-8** "As an owner, I want market-conduct controls (one working order per side, minimum
    resting time, price collars, participation caps, order-to-fill limits, close-window rules,
    workspace self-trade prevention) and a daily surveillance report, so that agents cannot produce
    manipulation-like patterns."
  - **E6-9** "As an owner, I want account restrictions and trading halts checked before every
    order, so that agents stop adding risk when the broker restricts the account. *Accepted when:*
    RC-15 passes."
- **PRD / HLD / spec anchors:** trading-domain spec §9 (the gate: §9.1 evaluation order and reason
  codes, §9.2 day-trading regime, §9.3 leverage and short sales, §9.4 sessions, §9.5 buying power,
  §9.6 market-conduct controls, §9.7 order-rate limits, §9.8 wash sales), §3.1 and §3.2 (instrument
  fields and the eligibility floor), §3.3 (concentration), §4.3 and §4.4 (sessions, auction
  windows, halts), §5.1 to §5.6 (the v1 order policy, the constraints enforced before submission,
  protective exits, the kill switch, exit pricing), §7.2 to §7.4 (account state and buying power,
  account restrictions, agent modes), §8.2 (risk marks), §14 (RC-09, RC-09B, RC-15, RC-16, RC-22,
  RC-25, and the gate steps of RC-03, RC-08, RC-17, RC-18); [mandate spec](../../specs/mandate.md)
  §1.1 (MI-1 to MI-20), §2.3 (the working universe at runtime), §5.3 (position, exposure, order
  size, count, cooldown), §5.5 (the drawdown ladder and the agent-scoped kill switch), §5.9
  (restrictions and the effective mode), §8.5 (the admission checks the gate re-checks); HLD §6.A
  (the order path) and the risk-gate section.
- **Decisions that apply:** DEC-03 and DEC-05 (the mandate is the contract; reducing risk never
  needs approval), DEC-26 (the account ledger), DEC-28, DEC-31 (the eligibility floor and conduct
  controls), DEC-32 (no short sales), DEC-34 (buying power is the lower of model and broker),
  DEC-37 (extended-hours exits only), DEC-40, DEC-41, DEC-44, DEC-48, DEC-49, DEC-54, DEC-56,
  DEC-63, DEC-70 (the close window and deferred discretionary exits), DEC-77 (brief, tests,
  implementation, status), DEC-79, DEC-80 (no plain comments), DEC-83 (tests PRs hold stubs only),
  DEC-85 (harness interpretations fail loudly until owned), DEC-89 and ES-21 (exact arithmetic,
  `BTreeMap`, no clock, no randomness), DEC-97 and DEC-101 (the working universe and admission),
  DEC-104 (buying power per bucket), DEC-110 (pending tests fail on the stubs), DEC-117 to DEC-126
  (`Proposed (founder)`; a veto reopens this brief), and **DEC-129**, this brief's interpretations.

## Scope

### What `mandate-risk` is

The gate is the one place where a limit is enforced. It is a **pure function**: a mandate, a risk
state, an account snapshot, a working universe, the market context, and one proposed order in; one
decision out. It reads no clock, no file, no network, and no random source; it holds no state
between calls; it never submits, cancels, or journals anything. The caller (the executor, E7-2)
journals the decision and acts on it. That is what "independent of agent logic" means in
[AGENTS.md](../../../AGENTS.md) rule 1: no agent code path can reach a limit, because the limit
lives in a function the agent does not call.

### Reference cases this stream moves from pending to passing

| Case | Story | What it pins |
|---|---|---|
| `mandate::MC-G01` to `MC-G16` | E6-3 | The mandate limits of spec §5.3: the per-instrument cap (usd and fraction), order size, re-entry cooldown across an instrument group, orders per day, agent gross exposure, the working universe, and every exit exemption |
| `mandate::MC-F01` to `MC-F04` | E6-3 | The agent-scoped flatten: mode first, only this agent's `client_order_id`s, never cancel-all or close-position, the sub-ledger quantity, equities deferred outside the regular session, the owner's confirmed-bid floor |
| `trading_domain::RC-16` and its `leveraged_etps_enabled` variant | E6-7 | The eligibility floor in §3.2 list order, and that an exit outside the universe is allowed |
| `trading_domain::RC-15` and its `status_not_active`, `unexplained_403s`, and `external_order_detected` variants | E6-9 | Account restrictions from status and rejects, the mode each implies, and that `account_trading_blocked` holds even a `risk_exit` |
| `trading_domain::RC-09` and its `alpaca_intraday_margin` variant, `RC-09B` | E6-6 | The `legacy_pdt` budget and `required`, that crypto never counts, that exits are never denied for it, and that `intraday_margin` allows what `legacy_pdt` denies |
| `trading_domain::RC-22`, `RC-25` | E6-8 | The collar on aggressiveness only, the passive band, the opposite-fill interval, `exits_only` after a conduct breach, the close window, and the session rules for each exit type |
| `trading_domain::RC-08`, `RC-18` and its `generic_cash_account` variant | E6-3 | Their `propose_order` steps, the last thing keeping them pending after E3-3 ([work tracker](../08-work-tracker.md) M2) |
| `trading_domain::RC-17` | not this stream | Its `deploy_agent` steps belong to E7-5; only its buying-power expectations are gate work, so the case stays pending |

Every other case keeps its state. RC-14, RC-20, RC-21, RC-24, and RC-04 are executor cases (E7-2 to
E7-4) even where their titles mention the gate; this stream leaves them pending.

Two notes the backlog predates. The backlog's E6-3 line says "MC-G01 to MC-G13" and "MI-1 to
MI-11"; the mandate spec v0.6 rewrite (#109) added MC-G14 to MC-G16 and MI-12 to MI-20, and this
stream takes the larger sets. Its E6-4 line names the R, T, and L families, which are
`kind: risk_state`, `kind: risk_day`, and `kind: goal` cases — stream F's fold, not this crate's
(see [Dependencies](#dependencies)).

### Invariants, and who owns each

| ID | Owner | What `mandate-risk` tests |
|---|---|---|
| MI-1 | **this crate** | The whole of it. `risk_exit` and `protective` are allowed; `owner_exit` is allowed, or deferred outside the regular session until the bid is confirmed; `discretionary_exit` is allowed or deferred, never denied. Only `paused`, `stopped`, an `Unknown` order, or the broker holds an exit. This is the stream's central property, and the fuzz's central assertion |
| MI-5, MI-6 | stream F folds it | The gate reads the effective mode and must never soften it: a property asserts that a stricter mode never turns a deny into an allow |
| MI-8 | **this crate** | The same mandate, state, and proposal give byte-identical decisions, check lists included |
| MI-9 | stream H | Consumed: a property feeds the builder's own proposals through the gate and asserts no mandate size limit denies one. It runs in `mandate-builder`, not here, and this brief only names it so it is not lost |
| MI-15, MI-19, MI-20 | stream F and J fold them | Consumed at the gate: no opening or increasing order in an instrument outside the working universe, whatever the universe's provenance; a removed instrument is exits-only in that instrument only |
| MI-2, MI-3, MI-4, MI-7, MI-13, MI-14 | stream F | Risk-state properties. Not tested here |
| MI-10, MI-11, MI-16, MI-17, MI-18 | streams H and J | Builder and admission properties. Not tested here |

### Crates

- **New:** `mandate-risk` (layer 4 per `xtask/layers.toml`'s plan; `pure = true`;
  `safety_critical = true`; `allowed_external = ["thiserror"]`; a CODEOWNERS line). It depends on
  `mandate-num`, `mandate-time`, `mandate-accounting`, and stream F's `mandate-spec`. Layer 4 sits
  above `mandate-spec` (3) and below `mandate-builder` (5), exactly as ADR-0001 ES-02's dependency
  rule reads, so the builder may call the gate and the gate can never call the builder.
- **Shared-crate additions:** `mandate-num` gains the exact arithmetic the gate needs (below);
  `mandate-refcases` gains the `gate` and `agent_flatten` interpretations for the mandate suite and,
  story by story, the trading-domain `propose_order` and `broker_account_update` steps and the
  `decision`, `agent_mode`, and `day_trade_count` expectations. Both under claim #123.
- **Out of scope:** `mandate-accounting` (read-only; the gate takes a snapshot, never the fold),
  `mandate-sim`, `mandate-marketdata`, `mandate-cli`, and the executor that will call this crate.
- **New dependencies:** none. `thiserror` and `proptest` are registered; the `docs/dependencies.md`
  "Used by" cells gain `mandate-risk` in the tests PR.
- **Size budget:** 400 non-generated lines per PR (ES-13). The tests PR will exceed it for test
  code and states its split; the implementation lands story by story, in the order below.

### Story order for the implementation PRs

Each is one PR on the coordinator's signal, and each ends with the status PR moving its cases:

1. **E6-3** the spine: the input types, the evaluation order, purpose assignment, the exit
   exemptions, the mandate limits of §5.3, and the agent flatten. It earns MC-G01 to MC-G16, MC-F01
   to MC-F04, and the `propose_order` steps of RC-03, RC-08, and RC-18.
2. **E6-9** account restrictions and halts (check 1 and check 3's halt) — RC-15.
3. **E6-7** the eligibility floor (check 2) — RC-16.
4. **E6-6** sessions, the v1 order policy, buying power, and the day-trading regime (checks 3, 4,
   7, 8) — RC-09, RC-09B.
5. **E6-8** marks, the collar, and the conduct controls (checks 5 and 6), and the surveillance
   report — RC-22, RC-25.
6. **E6-4** the ladder and the daily loss as the order path consumes them: the size factor, the
   `trim_to_target` proposals, and the mode effects. No new reference cases (they are stream F's).

## Data shapes

The caller's view, written before any logic. These are the tests PR's stubs, in `mandate-risk`.
Every collection is a `BTreeMap` or `BTreeSet` (ES-21), every decimal a `mandate-num` type, every
instant a `mandate_time::UtcNanos`.

```rust
pub fn evaluate(input: &GateInput<'_>) -> Result<Decision, GateError>;

pub struct GateInput<'a> {
    pub now: UtcNanos,                       // the risk clock's latest tick, never a wall clock
    pub config: &'a GateConfig,              // the organization's settings; `test_default` in the cases
    pub mandate: &'a Mandate,                // stream F: the validated document
    pub risk: &'a RiskState,                 // stream F: E, H, E0, C, L, latched limits, restrictions
    pub account: &'a AccountSnapshot,
    pub agent: &'a AgentSnapshot,
    pub instrument: &'a InstrumentSnapshot,
    pub market: &'a MarketSnapshot,
    pub conduct: &'a ConductState,
    pub universe: &'a WorkingUniverse,
    pub proposed: &'a ProposedOrder,
}

pub enum WorkingUniverse {
    Known { instruments: BTreeSet<InstrumentId>, pinned: bool },
    Unavailable,                             // fails closed: `GateError::WorkingUniverseUnavailable`
}

pub struct ProposedOrder {
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub limit_price: Price,                  // every v1 opening order is a limit order (§5.1)
    pub kind: ProposedKind,                  // Plain, Bracket { take_profit, stop }, Ioc
    pub tif: TimeInForce,
    pub extended_hours: bool,
    pub origin: Origin,                      // who proposed it; the gate types the purpose from it
    pub owner_confirmed_bid: Option<Price>,  // §5.5: an owner exit outside the regular session
    pub client_order_id: ClientOrderId,
}

pub enum Origin {
    OrderBuilder, GoalCompletion, RemovedInstrument,   // discretionary
    RiskEngine, AutomatedKillSwitch, TrimToTarget, StopWatchdog,  // risk
    Owner,                                             // owner close or kill switch
    ProtectiveLeg,                                     // protective
}

pub enum Purpose { Open, Increase, DiscretionaryExit, RiskExit, OwnerExit, Protective }

pub struct AccountSnapshot {
    pub account_type: AccountType,
    pub state: AccountState,                 // Active, ClosingOnly, Blocked (§7.3)
    pub crypto_active: bool,
    pub regime: DayTradeRegime,              // IntradayMargin { maintenance_excess } | LegacyPdt
    pub equity: Usd,
    pub prior_close_equity: Usd,             // the broker's `last_equity` (§9.2)
    pub model_buying_power: Usd,             // `Account::buying_power` (DEC-34, DEC-104)
    pub broker_buying_power: Usd,
    pub broker_non_marginable_buying_power: Usd,
    pub reservations: Usd,
    pub positions: BTreeMap<InstrumentId, SignedQty>,
    pub market_values: BTreeMap<InstrumentId, Usd>,    // signed; the gate takes |MV|
    pub working_orders: BTreeMap<ClientOrderId, WorkingOrder>,  // every agent on the account
    pub unknown_orders: BTreeSet<InstrumentId>,        // §5.3 rule 9
    pub related_account_resting: BTreeMap<InstrumentId, BTreeSet<Side>>,  // §9.6 self-trade
}

pub struct AgentSnapshot {
    pub agent: AgentId,
    pub mode: AgentMode,                     // strictest active restriction (§5.9, §7.4)
    pub instrument_restrictions: BTreeMap<InstrumentId, BTreeSet<InstrumentRestriction>>,
    pub positions: BTreeMap<InstrumentId, SignedQty>, // the agent's sub-ledger
    pub market_values: BTreeMap<InstrumentId, Usd>,
    pub working_orders: BTreeSet<ClientOrderId>,      // this agent's, keys into the account's map
    pub instrument_groups: BTreeMap<InstrumentId, GroupId>,
    pub last_exit_fill_at: BTreeMap<InstrumentId, UtcNanos>,
    pub orders_today: u32,                   // opening and increasing, each client order id once
    pub day_trades: DayTradeLedger,          // §9.2, folded by this crate
}

pub struct InstrumentSnapshot {
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub exchange: Option<Exchange>,          // None for crypto
    pub status_active: bool,
    pub tradable: bool,
    pub fractionable: bool,
    pub ipo: bool,
    pub ptp_no_exception: bool,
    pub etp: EtpClass,                       // Plain | Complex | Unclassified, with an `as_of`
    pub prior_close: Option<Price>,
    pub median_dollar_volume_20d: Option<Usd>,
    pub median_dollar_volume_30d: Option<Usd>,   // crypto
    pub min_order_size: Qty,
    pub min_trade_increment: ShareIncrement,
    pub price_increment: Price,
    pub halted: bool,
    pub status_feed_current: bool,           // a dropped feed is a presumed halt (§4.4)
}

pub struct MarketSnapshot {
    pub quote: Option<SaneQuote>,            // bid, ask, and the instant it was taken
    pub last_trade: Option<(Price, UtcNanos)>,
    pub trailing_5m_volume: Option<Qty>,     // §9.6 order-size participation
    pub adv_20d: Option<Qty>,                // §9.6 daily participation
    pub session: SessionAt,                  // derived from `now` by `mandate-time`, not supplied
}

pub struct ConductState {
    pub filled_today: BTreeMap<InstrumentId, u32>,
    pub orders_today_per_instrument: BTreeMap<InstrumentId, u32>,
    pub last_opposite_fill_at: BTreeMap<(InstrumentId, Side), UtcNanos>,
    pub participation_today: BTreeMap<InstrumentId, Qty>,
    pub resting_since: BTreeMap<ClientOrderId, UtcNanos>,
}

pub struct Decision {
    pub verdict: Verdict,
    pub reason: Option<ReasonCode>,
    pub purpose: Purpose,                    // assigned here, never taken from the proposer
    pub pacing: Option<Pacing>,              // §9.6: a sliced quantity or a collar-bounded price
    pub checks: Vec<CheckOutcome>,           // every check evaluated, in order, for the journal
}

pub enum Verdict { Allow, Deny, Defer, Hold }

pub struct Pacing {
    pub qty: Qty,                            // ≤ the proposed quantity; the remainder is re-proposed
    pub limit_price: Price,                  // within the collar
    pub marketable_limit_required: bool,     // the close window and extended hours
    pub applied: BTreeSet<PacingControl>,
}
```

And the ladder and the flatten, each a pure function of state:

```rust
pub fn size_factor(mandate: &Mandate, risk: &RiskState) -> Result<Fraction, GateError>;
pub fn trim_proposals(
    mandate: &Mandate, risk: &RiskState, agent: &AgentSnapshot,
    instruments: &BTreeMap<InstrumentId, InstrumentSnapshot>, session: SessionAt,
) -> Result<Vec<TrimProposal>, GateError>;
pub fn agent_flatten(input: &FlattenInput<'_>) -> Result<FlattenPlan, GateError>;

pub struct FlattenPlan {
    pub mode_applied_first: AgentMode,       // `paused` for a limit, `stopped` for an owner switch
    pub purpose: Purpose,
    pub cancel_client_order_ids: Vec<ClientOrderId>,  // sorted; only this agent's
    pub cancel_all_endpoint: bool,           // always false for an agent scope
    pub close_position_endpoint: bool,       // always false for an agent scope
    pub sells: Vec<FlattenSell>,
    pub deferred_sells: Vec<DeferredSell>,
}
```

`GateError` names one cause each with a stable code (ES-09):
`WorkingUniverseUnavailable`, `RiskStateStale`, `PurposeUnassignable`, `PositionMissingForExit`,
`QuoteUnsane`, `ConfigOutOfRange`, `InstrumentUnknown`, `DayTradeLedgerOutOfOrder`, plus the
numeric, time, and `mandate-spec` errors it wraps. **An error is never an allow** and never a
deny either: it is a refusal to decide, which the executor treats as the safe default of
[AGENTS.md](../../../AGENTS.md) rule 3 — no new risk, exits still available through the paths that
do not need the failing input.

### Arithmetic added to `mandate-num`

ES-04 keeps exact arithmetic in `mandate-num`, so the gate adds, under claim #123:
`Usd::times_fraction` (exact, no rounding: `max_position_fraction × E` and the collar's
`ask × (1 + x)` share one rule, that a product of two exact decimals is exact or an error),
`Usd::abs` and a signed `Usd::from_signed` for `Σ |MV|`, `Price::collar_bound(Fraction, Side,
Aggressive)` (one rounding, to the instrument's price increment, **against** the order so a bound
never admits a price the exact comparison would refuse), and `Qty::times_fraction` for the
participation caps (truncated to the increment, like `Qty::portion`). Nothing here rounds a limit
in the order's favour.

## The evaluation order

Trading-domain spec §9.1 fixes it, and **the first failing check's reason code is reported**. The
table below is the gate's whole contract: the left column is §9.1's number, and every row names the
story that implements it and the reason codes it can emit.

| §9.1 | Check | Story | Reason codes |
|---|---|---|---|
| 1 | Account status (§7.3), then agent mode (§7.4) | E6-9 | `account_trading_blocked`, `account_restricted`, `crypto_account_inactive`, `agent_exits_only`, `agent_paused`, `agent_stopped` |
| 2 | The working universe (mandate §2.3, §5.3), then the eligibility floor (§3.2 items 1 to 7, in list order), then concentration (§3.3 and the mandate per-instrument cap), then mandate order size, then the re-entry cooldown | E6-3 (universe, concentration, size, cooldown), E6-7 (floor) | `not_in_working_universe`, `not_in_universe`, `ineligible_exchange`, `ipo_not_tradable`, `below_price_floor`, `below_liquidity_floor`, `leveraged_etp_not_enabled`, `concentration_limit`, `max_order_size`, `reentry_cooldown` |
| 3 | Session, auction window, halt (§4.3, §4.4) | E6-6 (sessions), E6-9 (halts) | `session_not_allowed`, `extended_hours_opening_not_allowed`, `auction_window`, `instrument_halted`, and the defers `discretionary_exit_regular_session_only` and `owner_confirmation_required` |
| 4 | Order constraints (§5.3 rules 1 to 9, in list order) | E6-6 | `would_cross_zero`, `sell_exceeds_available`, `working_order_limit`, `add_blocked_by_protective_order`, `unknown_order_in_flight`, `market_order_not_allowed` |
| 5 | Mark freshness and the price collar (§8.2, §9.6) | E6-8 | `stale_mark`, `price_outside_collar` |
| 6 | Market-conduct controls (§9.6), the close window, and the mandate's orders per day | E6-8 (conduct), E6-3 (`max_orders_per_day`) | `min_resting_time`, `opposite_fill_interval`, `conduct_limit_breached`, `close_window`, `max_orders_per_day`, `order_rate_limited` |
| 7 | Buying power (§9.5), then gross exposure (§9.3: the account at 1×, then the agent's mandate limit) | E6-6 (buying power), E6-3 (gross exposure) | `insufficient_buying_power`, `insufficient_settled_buying_power`, `gross_exposure_limit` |
| 8 | Day-trade budget (§9.2) | E6-6 | `legacy_pdt_day_trade_budget` |

**Which failure is journaled first.** The gate evaluates in this order and **stops at the first
failure**, so the journaled reason is the first failing check's, which is what
`docs/specs/reference-cases/trading-domain.yaml`'s header states ("Reason codes follow the gate
evaluation order in spec §9.1; the first failing check wins"). `Decision::checks` still lists every
check the gate reached, in order, each with its verdict and the figures it compared, so the journal
records the evaluation and not only its conclusion (§9.1: "Every decision, including allows, is
journaled with the checks evaluated"). Checks after the failing one are listed as `NotReached`, so a
reader can never mistake "we did not look" for "it passed".

`reference/mandate/ref.py`'s `gate` is the mandate-limit subset of check 2, check 6's
`max_orders_per_day`, and check 7's gross exposure, in exactly that order, and the Rust reproduces
it exactly, including its `computed` blocks. Its `order_decision` is a fuzz composition, not the
normative order — see interpretation 2.

### Purpose is assigned, never taken

§9.1: "**Purpose is assigned by the gate** from side and position, never taken from the proposer."
The gate computes it from `Origin`, `Side`, and the agent's position:

| Side and position | Origin | Purpose |
|---|---|---|
| Buy, no position | any non-protective | `Open` |
| Buy, position held | any non-protective | `Increase` |
| Any side | `ProtectiveLeg` | `Protective` |
| Sell, quantity ≤ the agent's position | `RiskEngine`, `AutomatedKillSwitch`, `TrimToTarget`, `StopWatchdog` | `RiskExit` |
| Sell, quantity ≤ the agent's position | `Owner` | `OwnerExit` |
| Sell, quantity ≤ the agent's position | `OrderBuilder`, `GoalCompletion`, `RemovedInstrument` | `DiscretionaryExit` |
| Sell, quantity > the agent's position | any | denied `would_cross_zero` (§5.3 rule 3) |

A buy is never an exit, whatever the proposer says, and there is no v1 path by which a buy reduces
risk: v1 holds no shorts (DEC-32), so `would_cross_zero` and the sell-side table are the whole of
it. A protective leg's quantity is checked against position + entry quantity (§5.3).

### The exemptions, in one place

This is MI-1, [AGENTS.md](../../../AGENTS.md) rule 13, and the reason the gate exists in this shape.

| Purpose | Exempt from | Paced by | Can be held by |
|---|---|---|---|
| `Protective` | every conduct control, eligibility, the day-trade budget, buying power, and the opening-session rules | nothing | `paused` only for a *new* protective order that is not a re-placement before expiry; `stopped`; an `Unknown` order in the instrument; the broker |
| `RiskExit` (risk engine, automated kill switches, `trim_to_target`, the stop watchdog) | all of the above, and §9.6 entirely | nothing | `stopped`; an `Unknown` order; the broker. A kill switch's own orders are exempt even from `paused` |
| `OwnerExit` | all of the above | the participation caps only | `stopped`; an `Unknown` order; the broker. Outside the regular session an equity sell is **deferred** until the owner confirms the displayed bid, then allowed with a floor price |
| `DiscretionaryExit` | denial: it is **never** denied by a conduct control, the eligibility floor, a day-trade budget, buying power, or an opening-session rule | the collar prices it, the participation caps slice it, the close window makes it a marketable limit, and outside the regular session an equity one is **deferred** | `paused`, `stopped`; an `Unknown` order; the broker |
| `Open`, `Increase` | nothing | nothing | every check above |

A `Defer` is never converted to a `Deny` (§9.1), and a deferred intent is not stored: the order
builder proposes again at the next evaluation and at the regular-session open. The kill switch is
always available, touches only its scope, and does not depend on model state.

## The drawdown ladder as a pure function

E6-4 in this crate is what the order path *does* with the ladder; stream F folds the ladder's state
from the account stream. Three pure functions read that state and nothing else:

1. **`size_factor(mandate, risk)`** — the product of the active `scale_sizes` rungs' factors
   (mandate §5.5), an exact product of `Fraction`s with no intermediate rounding, `Fraction::ONE`
   when no rung is active. `scale_action: limit_buys` multiplies the order-builder target by it
   (stream H applies it; this crate only computes it, so the two cannot disagree).
2. **`trim_proposals(...)`** — for `scale_action: trim_to_target`, one `TrimProposal` per position
   where MV − factor × cap ≥ `rebalance_band` × cap, selling down to factor × cap as a `RiskExit`,
   the quantity **rounded up** to the increment, at most once per evaluation, only once the rung has
   been active for `breach_confirm_s`, only if the order meets the instrument minimum, for equities
   only in the regular session, and never while the goal is `Holding` (DEC-65). The comparison is
   exact; `factor × cap` is one exact product.
3. **`agent_flatten(...)`** — the plan of mandate §5.5 and trading spec §5.5, reproducing
   `ref.py`'s `agent_flatten` exactly: the final mode first (`paused` for a mandate limit, `stopped`
   for an owner kill switch), then only this agent's `client_order_id`s sorted ascending, never the
   cancel-all or close-position endpoint, then a sell of exactly the agent's sub-ledger quantity per
   position (a zero quantity produces no sell). Outside the regular session an automated flatten
   defers equity sells to the regular-session open and sends crypto now; an owner kill switch with a
   confirmed bid sells equities now through the exit price ladder with
   `floor_price = confirmed_bid × (1 − max_exit_offset)` and a remainder that rests at the floor,
   and without confirmation defers them. This function decides; the executor (E7-2) carries it out.

The daily loss is the same shape: stream F latches it and sets the restriction, and this crate's
only part is that the resulting mode reaches the gate's check 1 and that the exemptions above still
hold. A property asserts that for every risk state stream F can produce, a latched limit never
denies a `RiskExit`, a `Protective`, or an `OwnerExit`.

## The conduct controls' state, and how it is fed without a clock

§9.6's controls are all about elapsed time, and the crate reads no clock (ES-21). Every one of them
is a comparison between `GateInput::now` — the risk clock's latest tick, supplied by the caller and
also used by `ref.py`'s `gate` as `st["now"]` — and an instant in `ConductState`, which the executor
folds from the account stream:

| Control | State it needs | Comparison |
|---|---|---|
| One side at a time; one working non-protective order per instrument per account | `AccountSnapshot::working_orders` | Set membership; no time |
| Minimum resting time (2 s) before canceling a non-marketable opening order | `ConductState::resting_since[order]` | `now − resting_since < 2 s` denies the cancel, unless the cancel precedes a risk-reducing order |
| Price collar | `MarketSnapshot::quote` and its instant | `limit ≤ ask × (1 + x)` for a buy; `limit ≥ bid × (1 − x)` for a sell; a passive price within the 20% band is allowed. `x` comes from `GateConfig` by the instrument's tier |
| Order size vs trailing 5-minute volume (≤ 5%) | `MarketSnapshot::trailing_5m_volume` | Supplied, never derived here |
| Daily participation vs 20-day ADV (≤ 5%) | `ConductState::participation_today` and `MarketSnapshot::adv_20d` | Exact `Qty` comparison |
| Order-to-fill ratio (≤ 10 after ≥ 20 orders) | `ConductState::orders_today_per_instrument`, `filled_today` | `orders ÷ max(fills, 1)`, compared without dividing: `orders > 10 × max(fills, 1)` |
| No opening order within 60 s after an opposite-side fill | `ConductState::last_opposite_fill_at` | `now − last < 60 s` |
| Close window (last 10 minutes of the regular session) | `MarketSnapshot::session` | `now ≥ session_end − 10 min`, with the early-close calendar giving `session_end` |
| Self-trade prevention across related accounts | `AccountSnapshot::related_account_resting` | Set membership; the executor supplies the group |

The session, the auction windows, and the close window are **derived by the gate** from `now` and
`mandate-time`'s NYSE calendar, not supplied by the caller: a caller that mislabels the session
could open in an auction window, and a check the caller can defeat is not an independent gate. The
calendar is committed data, so deriving it keeps the function pure and replayable.

## The fuzzing design for E6-3

E6-3's acceptance is "simulation fuzzing across random market paths and mandates never produces an
order outside limits". The fuzz is a `proptest` suite in `crates/mandate-risk/tests/fuzz.rs`:

- **Generators.** A mandate drawn from the three bases in `reference/mandate/bases.py`
  (`two_stock_swing`, `btc_accumulator`, `research_equity`) with its `risk` block replaced by
  randomly drawn but valid limits; a market path of marks over a generated instrument set; a
  sequence of proposals covering every `Origin` and both sides, including quantities and prices
  chosen to sit just inside and just outside each limit; and random account states (modes,
  restrictions, sessions, day-trade ledgers, conduct state).
- **The loop.** Each proposal goes through `evaluate`. An allowed order is then *applied* to a
  shadow ledger as if it filled in full at its limit price, so exposure accumulates across the
  sequence and no single-order check can hide a sequence that walks past a limit by splitting.
- **The assertion, computed independently.** A separate oracle in `tests/oracle.rs` recomputes each
  limit in `i128` at 10⁻⁹ from the shadow ledger's own accumulators — never from `Decision::checks`
  and never by calling the crate's own arithmetic — and asserts, after every allowed order: the
  per-instrument total ≤ min(`max_position_usd`, `max_position_fraction` × E); the order's notional
  ≤ `max_order_usd`; Σ |MV| + working + the order ≤ min(`max_gross_exposure_usd`, E); the count of
  opening and increasing orders ≤ `max_orders_per_day`; no opening order within `reentry_cooldown_s`
  of an exit fill in the group; and the instrument in the working universe. The oracle is shown to
  fail on each of the planted bugs below before it is trusted.
- **MI-1, the other half.** For every generated state and every reducing purpose, assert the
  verdict is never `Deny`, and that it is `Hold` only for the four causes MI-1 names. A generator
  that produces a state where an exit *should* be held is required (a `paused` mode, an `Unknown`
  order), so the property cannot pass vacuously.
- **MI-8.** Every generated input is evaluated twice, and the two `Decision`s compared field by
  field, including the check list's order.
- **Anti-vacuity.** A coverage assertion requires the run to have produced at least one allow, one
  deny of each mandate limit, one defer, and one hold; a fuzz that only ever denies proves nothing.

## Interpretations (recorded as DEC-129)

1. **Crate, layer, and criticality.** `mandate-risk` is layer 4, pure, and safety-critical: it is
   the last thing between an agent's opinion and the owner's money. It gets the lint header, the
   mutation gate, and the DEC-77 sequence. `xtask/layers.toml` and `CODEOWNERS` are founder-owned,
   so the tests PR adds both entries and the founder may veto them.
2. **§9.1 is the normative order; `ref.py`'s `order_decision` is a fuzz composition.** `ref.py`'s
   `gate` reproduces §9.1's mandate-limit checks in §9.1's order, and the Rust reproduces `gate`
   exactly, `computed` blocks included. `order_decision` puts the mode, the instrument restrictions,
   the session rules, and the close window before `gate`, which is §9.1's checks 1, 5, 3, and 6
   before check 2; it is documented in `ref.py` as "the composition used by the fuzz for MI-1", and
   its order changes no verdict, only which code is reported when two checks fail at once. Where the
   two disagree, **§9.1 wins**, and the fuzz oracle compares verdicts with `order_decision` and
   reason codes with §9.1. No reference case exercises the disagreement.
3. **An absent working universe is an error.** `ref.py`'s `gate` indexes `st["working_universe"]`,
   so an absent universe raises rather than allowing. The Rust makes that unrepresentable where it
   can — `WorkingUniverse` has no `Default` and no constructor that guesses — and where a caller
   genuinely does not know it, `WorkingUniverse::Unavailable` returns
   `GateError::WorkingUniverseUnavailable`. An empty universe is a different thing: it is known, and
   it denies every opening (MC-G16). A `GateError` is neither an allow nor a deny; it is a refusal
   to decide, and the executor's safe default applies.
4. **Purpose from `Origin`, side, and position.** §9.1 says the gate assigns the purpose from side
   and position and types an exit by its originating component, but the type of an exit cannot be
   read off a side. `Origin` carries the originating component and nothing else — no verdict, no
   purpose — so the gate still assigns, and a proposer that lies about its origin can only make its
   own exit *stricter* (a `DiscretionaryExit` is paced where a `RiskExit` is not) or claim a
   `RiskExit` it cannot produce, which the executor's own component identity contradicts. The
   mapping is the table in "Purpose is assigned, never taken".
5. **Every trading-domain check is a check of this crate, even those the mandate spec does not
   mention.** The eligibility floor, the sessions, the order constraints, the collar, the conduct
   controls, buying power, and the day-trade budget are all §9.1 checks, so they are one function
   with the mandate limits rather than a second gate somewhere else. Two gates would eventually
   disagree, and one of them would be the one that allowed the order.
6. **The day-trade ledger is folded here.** §9.2's window, count, and `required` are rules of the
   gate, and no other crate has a reason to know them, so `DayTradeLedger` is a pure fold in
   `mandate-risk` over fills and working orders: window = today plus the four prior trading days
   (from `mandate-time`'s calendar), shares held overnight sold first, each same-day open-then-close
   counted once, crypto never counted, fractional day trades counted. The harness's
   `day_trade_count` expectation reads it. `remaining = 3 − count` when prior-close equity is below
   the configured threshold and the account is not already flagged, `0` when it is flagged and below
   the threshold, unlimited otherwise; `required = 1 + (1 if the same security was sold earlier
   today) + open same-day positions + working same-day opening orders in other instruments`.
7. **`intraday_margin` never denies a 1× long-only opening.** §9.2 says an approved order at 1×
   long-only exposure cannot create an intraday margin deficit, and check 7 already enforces 1×. So
   under `intraday_margin` check 8 denies nothing; it only reads the broker's reported maintenance
   excess, and a reported deficit is an account state (`closing_only`, mode `exits_only`) set by
   E6-9's check 1, not a per-order denial. RC-09's `alpaca_intraday_margin` variant is exactly this.
8. **Buying power is the lower of model and broker (DEC-34), compared without dividing.** Check 7
   compares `qty × limit_price + round(estimated fees, 2, ceiling) ≤ buying_power − reservations`,
   every term exact. A cash account's failure is `insufficient_settled_buying_power` and a margin
   account's is `insufficient_buying_power`, which is what RC-08 and RC-18's variant distinguish.
   Crypto uses `min(model, broker non-marginable)` and a fee reservation of 0 (§7.2).
9. **The gate derives the session; the caller does not label it.** See "how it is fed without a
   clock". `mandate-time`'s session module and calendar are committed data, so this keeps the
   function pure, and it removes a whole class of caller mistake. A date outside the committed
   calendar's range is `GateError::ConfigOutOfRange`, never a guessed session.
10. **An unclassified ETP fails closed, and so does a stale classification.** §3.2 item 6 says an
    ETP not yet classified is treated as complex. `EtpClass::Unclassified` therefore behaves exactly
    as `Complex`, and `EtpClass` carries the classification's `as_of`: past
    `GateConfig::etp_classification_max_age_s`, ETP openings are denied with
    `leveraged_etp_not_enabled`. `test_default` has no such field, and no reference case needs one,
    so the crate's default is used for the harness and the field is proposed for the config the next
    spec window touches (see Decisions needed).
11. **Pacing is an output, not a second call.** §9.6 says discretionary exits are "paced, never
    denied": the collar prices them, the participation caps slice them, and in the close window they
    are sent as marketable limits. So a discretionary exit's `Allow` carries a `Pacing` with the
    sliced quantity and the collar-bounded limit price, and the executor sends exactly that. The
    remainder is not stored (a deferred or sliced intent is re-proposed), which is §9.1's rule for
    `defer` applied to the same reasoning. An `Open` or `Increase` is never paced: it is allowed as
    proposed or denied.
12. **`Hold` is a verdict, not a deny.** MI-1 says an exit "may be held only by agent mode `paused`
    or `stopped`, an `Unknown` order, or the broker". A hold is not a denial by a limit, and calling
    it one would make MI-1 false in the code while true in the spec. `ref.py`'s `order_decision`
    already returns `{"verdict": "hold"}` for `paused` and `stopped`, so the Rust keeps the same
    four verdicts. The trading-domain reason registry has `agent_paused` and `agent_stopped` for it.
13. **The surveillance report is a pure function, and it supervises nothing.** E6-8's daily report
    is `fn surveillance(day, workspace_state) -> SurveillanceReport`: self-trade checks,
    order-to-fill ratios, close-window activity, and concentration, with the threshold breaches
    flagged. §9.6 is explicit that "the platform does not supervise users' trading", so the report
    states figures and flags thresholds and makes no judgement, and routing it to the owner and
    journaling the acknowledgment is the runtime's (stream I's), not this crate's.
14. **The gate never journals.** "Journal before acting" ([AGENTS.md](../../../AGENTS.md) rule 5) is
    the executor's obligation; a pure function cannot hold a journal handle without ceasing to be
    pure. `Decision::checks` is the payload the executor journals, which is why it records every
    check reached and marks the rest `NotReached`.
15. **Comparisons are exact and one-sided.** Every limit comparison is `>` against the limit
    (`ref.py`: `if inst_total > cap`), so a value exactly at the limit passes (MC-G02 pins it).
    Nothing rounds before a comparison. Where a bound must be rounded to the price increment (the
    collar), it is rounded **against** the order, so the rounded bound never admits a price the
    exact comparison would refuse.
16. **Fee reservations for buying power** — the follow-up the work tracker assigns to E6-6 — are
    `round(estimated fees, 2, ceiling)` per order from the pinned fee configuration (§9.5), computed
    through `mandate-accounting`'s fee rules rather than a second copy here, and 0 for a crypto buy
    (the fee is paid in the asset). The reservation is an input to check 7 only; the executor holds
    the reservation itself.

## Decisions needed

Neither can be taken by an agent: both would change a founder-owned file
([AGENTS.md](../../../AGENTS.md) rule 9, ES-22). Both are listed as `Proposed (founder)` in the
DEC-129 row.

1. **Two reason codes for one condition: `not_in_universe` and `not_in_working_universe`.**
   [mandate spec §5.3](../../specs/mandate.md#53-position-exposure-order-size-count-and-cooldown)
   gives the working-universe check the code `not_in_working_universe` at §9.1 check 2, and
   `MC-G14` and `MC-G16` expect it. [trading-domain spec §3.2](../../specs/trading-domain.md#32-eligibility-floor-dec-31)
   item 1 is "Instrument in the agent's mandate universe", whose registered code is
   `not_in_universe`, and `RC-16` step 7 expects it. The two checks are the same condition whenever
   `universe.pinned` (mandate spec §2.3: "the pinned universe *is* the working universe"), and both
   `MC-G14`'s base (`two_stock_swing`) and `RC-16`'s agent are pinned — so on the same input the two
   suites expect different codes, and no implementation can satisfy both. **Recommendation:** one
   code, `not_in_working_universe`, the name spec v0.6 introduced with DEC-97; the founder removes
   `not_in_universe` from the registry, drops item 1 from the §3.2 list (the working-universe check
   at the head of check 2 already covers it), and updates `RC-16` step 7. Until that lands, the
   Rust emits `not_in_working_universe`, `RC-16` stays pending, and nothing is quietly bent to pass.
2. **The ETP classification's maximum age has no configured value.** §3.2 item 6: "If the
   classification data is older than the configured age, ETP openings are denied." No field in
   `configs.test_default.gate` carries it, so the rule has no value to compare against and no case
   can exercise it. **Recommendation:** add `etp_classification_max_age_s` to the gate config
   (proposal: 7 days) in the next spec window, and, if the founder wants it pinned by a case, one
   step in `RC-16`. Until then, interpretation 10's crate default stands and the rule is exercised
   by hand tests only.

## Dependencies

- **Stream F (`mandate-spec`)** owns `Mandate`, `RiskState`, `AgentMode`, `InstrumentRestriction`,
  and the Rust harness that reads `docs/specs/reference-cases/mandate.yaml`. This brief needs none
  of it; the **tests PR does**, and F's tests PR may not have merged by then. In that window the
  tests PR defines the minimal shapes it needs behind a single module,
  `crates/mandate-risk/src/spec_types.rs`, holding exactly: `Mandate { risk: RiskLimits, goal,
  universe }` with `RiskLimits { max_position_usd, max_position_fraction, max_order_usd,
  max_gross_exposure_usd, max_orders_per_day, reentry_cooldown_s, drawdown_ladder, max_daily_loss,
  max_loss_from_allocation, rebalance_band }`; `RiskState { agent_equity, high_water_mark,
  day_open_equity, capital_base, inherited_loss, latched: BTreeSet<LatchedLimit>, restrictions,
  active_rungs }`; `AgentMode`; and `InstrumentRestriction`. Every one of them is a field the
  mandate schema already names, so the module is a subset, not a second design. The **first**
  implementation PR after F's tests PR merges deletes the module and takes F's types; it is
  `#[doc(hidden)]` and named in the PR body so the deletion is not forgotten. If F's tests PR merges
  first, the module is never written.
- **Stream H (`mandate-builder`)** consumes `size_factor` and proposes the orders MI-9 asserts the
  gate never denies. The dependency runs H → G (layer 5 depends on layer 4), so nothing here waits
  on H.
- **Stream I (the runtime)** calls the gate and journals the decision; E6-5's kill switches use
  `agent_flatten`'s plan. Nothing here waits on I.
- **`mandate-accounting`** supplies the buying-power figure (DEC-104) and the fee rules for check 7.
  No change to it is needed; if one turns out to be, it goes in its own PR under a shared-crate
  claim, never folded into this stream's.
- **Coordination:** the M5 work graph lists "agent flatten" under both streams F and G, and the R,
  T, and L families under both. This brief takes **`kind: gate` and `kind: agent_flatten`** (MC-G
  and MC-F) and leaves **`kind: risk_state`, `kind: risk_day`, and `kind: goal`** (MC-R, MC-T, MC-L)
  to stream F, because the first two produce orders and the last three fold state. The coordinator
  confirms the split when it merges the second of the two briefs; if it prefers the other division,
  only this brief's scope table and the tracker rows change, not the crate.

## Planted bugs

The tests PR reports, for each bug below, the tests that failed when it was planted one at a time in
a throwaway implementation of the stubs (kept out of the PR, DEC-83). A bug no test catches is a
missing test, and the tests PR does not merge with one.

| # | Bug | Must be caught by |
|---|---|---|
| 1 | The per-instrument cap uses `max_position_usd` alone, ignoring `max_position_fraction × E` | `MC-G05`, `properties::position_cap_is_the_lower_of_both_bounds`, the fuzz oracle |
| 2 | The per-instrument total omits working opening orders in the instrument | `MC-G01`, `MC-G02`, the fuzz oracle after a split order |
| 3 | The comparison is `≥` instead of `>`, so a value exactly at the limit is denied | `MC-G02`, `MC-G13`, `properties::a_value_exactly_at_a_limit_passes` |
| 4 | Gross exposure omits the proposed order | `MC-G04`, `MC-G06`, the fuzz oracle |
| 5 | Gross exposure caps at `max_gross_exposure_usd` without also capping at E | `MC-G06`, `properties::gross_exposure_is_bounded_by_equity` |
| 6 | The re-entry cooldown checks only the proposed instrument, not its group | `MC-G12`, `properties::cooldown_covers_the_whole_group` |
| 7 | An exit is run through the mandate limits instead of returning early | `MC-G08`, `MC-G09`, `MC-G10`, `MC-G15`, `properties::mi1_reduction_is_never_denied` |
| 8 | An absent working universe is treated as "everything allowed" | `hand::an_absent_working_universe_is_an_error`, `properties::mi1_...` cannot catch it, which is why the hand test exists |
| 9 | An empty working universe is treated as absent (an error) rather than as denying | `MC-G16` |
| 10 | The eligibility floor runs its items out of §3.2 order, so `below_price_floor` is reported where `ineligible_exchange` should be | `RC-16` steps 2 and 3, `hand::the_floor_reports_the_first_failing_item` |
| 11 | An unclassified ETP is treated as plain | `hand::an_unclassified_etp_is_complex`, `properties::etp_fails_closed` |
| 12 | The eligibility floor is applied to a `risk_exit` in a held instrument | `RC-16` step 8, `MC-G15`, `properties::mi1_...` |
| 13 | `account_trading_blocked` is treated as `exits_only` rather than `paused`, so a `risk_exit` is allowed | `RC-15::status_not_active`, `hand::a_blocked_account_holds_even_a_risk_exit` |
| 14 | The unexplained-403 counter resets on an unrelated success, so the threshold is never reached | `RC-15::unexplained_403s`, `hand::three_consecutive_unexplained_403s_restrict_the_account` |
| 15 | The day-trade window is the four prior trading days without today | `RC-09`, `hand::the_window_is_today_plus_four` |
| 16 | `required` omits open same-day positions | `RC-09B` step 3, `properties::required_counts_every_component` |
| 17 | Crypto counts as a day trade | `RC-09` step 3, `hand::crypto_never_counts` |
| 18 | The day-trade budget denies an exit | `RC-09B` step 4, `properties::mi1_...` |
| 19 | `intraday_margin` inherits the `legacy_pdt` budget | `RC-09::alpaca_intraday_margin` |
| 20 | The collar is applied to passive prices as well as aggressive ones | `RC-22` step 2, `hand::a_passive_price_inside_the_band_is_allowed` |
| 21 | The collar's tier threshold compares with `>` where the spec says `≥ 50 M` | `hand::a_median_dollar_volume_exactly_at_the_threshold_is_liquid` |
| 22 | The opposite-fill interval is checked in the wrong direction (a same-side fill blocks) | `RC-22` steps 5 and 6, `properties::only_an_opposite_side_fill_starts_the_interval` |
| 23 | The close window is computed from 16:00 on an early-close day | `hand::the_close_window_follows_the_early_close_calendar` |
| 24 | The close window denies a discretionary exit instead of pacing it | `RC-25` step 3, `properties::mi1_...` |
| 25 | A discretionary exit outside the regular session is denied rather than deferred | `RC-25` step 5, `properties::a_discretionary_exit_is_never_denied` |
| 26 | An owner exit outside the session is allowed without a confirmed bid | `RC-25` steps 7 and 8, `hand::an_unconfirmed_owner_exit_defers` |
| 27 | A crypto discretionary exit outside the regular session is deferred (the equity rule applied to crypto) | `hand::crypto_exits_run_at_all_hours`, `properties::...` |
| 28 | Buying power uses the model figure without taking the lower of model and broker | `RC-17`-shaped hand test, `properties::buying_power_is_the_lower_of_the_two` |
| 29 | A cash account reports `insufficient_buying_power` instead of `insufficient_settled_buying_power` | `RC-08` step 3, `RC-18::generic_cash_account` |
| 30 | The fee reservation is omitted from check 7 | `RC-18` step 3, `hand::a_reservation_includes_the_rounded_fee` |
| 31 | `max_orders_per_day` counts fills rather than submitted orders | `MC-G07`, `hand::a_rejected_order_still_counts` |
| 32 | The flatten uses the broker's position quantity rather than the agent's sub-ledger | `MC-F01`, `MC-F02` (broker 15 against the agent's 10) |
| 33 | The flatten uses the cancel-all endpoint | `MC-F01`, `properties::an_agent_flatten_never_touches_another_agent` |
| 34 | The flatten applies the mode after canceling | `MC-F01`, `hand::the_final_mode_is_applied_first` |
| 35 | An automated after-hours flatten sells equities now instead of deferring | `MC-F02` |
| 36 | An owner after-hours flatten without a confirmed bid sells equities now | `MC-F04` |
| 37 | The owner's floor price is `bid × max_exit_offset` rather than `bid × (1 − max_exit_offset)` | `MC-F03` (97, not 3) |
| 38 | `size_factor` sums the rungs' factors instead of multiplying them | `hand::two_active_rungs_multiply`, `properties::the_size_factor_never_exceeds_one` |
| 39 | `trim_to_target` rounds the sell quantity down to the increment | `hand::a_trim_rounds_up_to_the_increment` |
| 40 | The decision is not deterministic: the check list is built from a `HashMap` iteration | `properties::mi8_identical_inputs_give_identical_decisions` (and clippy's `disallowed-types`, which is why the crate is `pure`) |

## Not done

- **Journaling the decision, submitting, canceling, and reserving.** The executor (E7-2) does all
  four; this crate returns a plan and a payload.
- **The order-rate limits of §9.7** beyond reading the resulting restriction: the platform's
  per-second and per-day counters live with the runtime (stream I), which sets the `rate_limit`
  restriction the gate reads at check 1. The mandate's `max_orders_per_day` is this crate's.
- **Wash-sale flagging (§9.8).** Informational and never blocking; it belongs with reporting.
- **The protective-exit sequencing of §5.4** (bracket to OCO, unprotected intervals, the
  re-placement schedule, the stop watchdog's timer): E7-4 owns the sequence; this crate only types
  a `Protective` order and exempts it.
- **The exit price ladder's steps (§5.6).** The gate names the ladder in a flatten plan and bounds
  a price by the collar; walking the ladder with its 5-second steps is the executor's.
- **Approvals.** Autonomy classification (AUTO / ASK / DENY) is E6-2, in `mandate-builder`. The gate
  runs on every proposal whatever its autonomy decision, and an approval never lifts a limit.
- **Reconciliation and crash recovery** (E7-3), and the broker's own rejects: a reject maps to an
  account state that reaches the gate as input, and the mapping table is the connector's.
- **Calibration of any limit.** No code path changes an envelope field
  ([AGENTS.md](../../../AGENTS.md) rule 11).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-risk
cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/mandate::MC-[GF]/)'
cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/trading_domain::RC-(09|09B|15|16|22|25)/)'
cargo mutants -p mandate-risk
```

For the brief PR, which touches no code:

```bash
cargo xtask check
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong (the two found so far are under Decisions needed);
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- a reference case would have to change to make the code pass;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] MC-G01 to MC-G16 and MC-F01 to MC-F04 pass; RC-09, RC-09B, RC-15, RC-16, RC-22, and RC-25
      pass, with their variants; RC-08 and RC-18 pass once their `propose_order` steps are
      interpreted; no case that passed before now fails.
- [ ] Tests came first; every invariant and every "never" or "always" above has a property test
      whose oracle computes the answer its own way and was shown to fail on a planted bug.
- [ ] The fuzz of E6-3 runs several seeds and its coverage assertion holds.
- [ ] New state changes emit journal events (none: the gate is pure; the executor journals the
      `Decision`).
- [ ] Docs updated: the feature map's "Risk gate" entry, the tracker's M5 and Claims rows, and
      `docs/dependencies.md`'s "Used by" cells.
- [ ] `cargo xtask check` is green, with zero missed mutants on the diff (summary in each PR).
- [ ] Each PR description is complete (see the PR template).
