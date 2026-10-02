# Task: E6-3 and the risk gate (`mandate-risk`)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15), for **stream G** of the
M5 work graph. It covers the whole of the `mandate-risk` crate, because the gate is one function
and its stories are its checks: splitting them across briefs would hide the one thing that matters,
the order in which the checks run. Each story keeps its own tests, pending marker, implementation
PR, and reference cases.

## Story

- **Stories:** E6-3, E6-4, E6-6, E6-7, E6-8, E6-9, E6-10
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
  - **E6-10** "As an owner, I want the gate to admit crypto **USD pairs only** (trading-domain §3.2
    item 7), so that an agent cannot open a stablecoin-quoted pair the floor was never written for.
    *Accepted when:* a crypto opening in a non-USD pair is denied, a USD pair passes the floor, and
    check 2 is whole for crypto." Its readings are DEC-254, and its tests are
    `crates/mandate-risk/tests/usd_pairs.rs`.
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
7. **E6-10** check 2's "USD pairs only" for crypto (§3.2 item 7), read from
   `InstrumentSnapshot::quote_currency` (DEC-254), which makes check 2 whole for crypto and leaves
   no check owed. No reference case moves in `mandate-risk`; MC-B26 to MC-B28 and the
   trading-domain harness's crypto proposals need the harness to read the pair (DEC-254 item 7).

## Data shapes

The caller's view, written before any logic. These are the tests PR's stubs, in `mandate-risk`.
Every collection is a `BTreeMap` or `BTreeSet` (ES-21), every decimal a `mandate-num` type, every
instant a `mandate_time::UtcNanos`.

```rust
pub fn evaluate(input: &GateInput<'_>) -> Result<Decision, GateError>;
pub fn evaluate_cancel(input: &CancelInput<'_>) -> Result<Decision, GateError>;

pub struct GateInput<'a> {
    pub now: UtcNanos,                       // the risk clock's latest tick, never a wall clock
    pub pass: GatePass,                      // First, or BeforeSubmission (§5.3's re-run)
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

pub enum GatePass { First, BeforeSubmission }

pub struct CancelInput<'a> {
    pub now: UtcNanos,
    pub config: &'a GateConfig,
    pub order: &'a WorkingOrder,
    pub resting_since: UtcNanos,
    pub precedes_risk_reducing_order: bool,  // §9.6 exempts such a cancel from resting time
    pub marketable: bool,                    // the rule binds non-marketable opening orders only
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
    RiskEngine, TrimToTarget, StopWatchdog,            // risk, paced by nothing but held by `paused`
    AutomatedKillSwitch,                               // risk, and exempt from `paused`
    OwnerClose,                                        // owner exit, held by `paused`
    OwnerKillSwitch,                                   // owner exit, and exempt from `paused`
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
    pub model_buying_power: Usd,             // `Account::buying_power(reservations)`: already net
    pub broker_buying_power: Usd,            // of reservations and pending charges (DEC-34, DEC-104)
    pub broker_non_marginable_buying_power: Usd,
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
}

pub struct SessionAt {                       // derived inside the gate; never a caller's label
    pub session: Session,                    // Overnight, PreMarket, Regular, AfterHours, Continuous
    pub start: UtcNanos,
    pub end: UtcNanos,                       // the early-close calendar decides it
    pub opening_auction: bool,               // 09:28 to 09:30 ET
    pub close_window: bool,                  // the last `close_window_minutes` of the regular session
}

pub struct WorkingOrder {
    pub agent: AgentId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub max_cost: Usd,                       // what §5.3 and MC-G01 count toward exposure
    pub open_qty: Qty,
    pub protective: bool,
    pub opening: bool,                       // an opening or increasing order, for §9.2's `required`
    pub submitted_on: Date,
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
    now: UtcNanos, config: &GateConfig, mandate: &Mandate, risk: &RiskState,
    agent: &AgentSnapshot, instruments: &BTreeMap<InstrumentId, InstrumentSnapshot>,
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
`WorkingUniverseUnavailable`, `RiskStateStale`, `PurposeUnassignable`, `QuoteUnsane`,
`ConfigOutOfRange`, `InstrumentUnknown`, `DayTradeLedgerOutOfOrder`, plus the
numeric, time, and `mandate-spec` errors it wraps. **An error is never an allow** and never a
deny either: it is a refusal to decide, which the executor treats as the safe default of
[AGENTS.md](../../../AGENTS.md) rule 3 — no new risk, exits still available through the paths that
do not need the failing input.

### Arithmetic added to `mandate-num`

ES-04 keeps exact arithmetic in `mandate-num`, so the gate adds, under claim #123:
`Usd::times_fraction` (exact, no rounding: `max_position_fraction × E` and the collar's
`ask × (1 + x)` share one rule, that a product of two exact decimals is exact or an error),
`Usd::abs` for `Σ |MV|`, built on the already-tested `is_negative` and `negated` so it adds no
comparison of its own, and `Price::collar_bound(Fraction, Adverse)` (one rounding, at the 9 places a
price holds, each bound rounded so the constraint gets **stricter** — a buy's ceiling truncates down
and a sell's floor rounds up — so a bound never admits a price the exact comparison would refuse).
The participation caps need no new function: `Qty::portion(Fraction, ShareIncrement)` is already
`truncate(fraction × self, increment)`, which is the cap this paragraph once called
`Qty::times_fraction`, so E6-8's implementation PR adds nothing to `mandate-num` (DEC-163 item 9).
Nothing here rounds a limit in the order's favour.

## The evaluation order

Trading-domain spec §9.1 fixes it, and **the first failing check's reason code is reported**. The
table below is the gate's whole contract: the left column is §9.1's number, and every row names the
story that implements it and the reason codes it can emit.

| §9.1 | Check | Story | Reason codes |
|---|---|---|---|
| 1 | Account status (§7.3), then agent mode (§7.4) | E6-9 | `account_trading_blocked`, `account_restricted`, `crypto_account_inactive`, `agent_exits_only`, `agent_paused`, `agent_stopped` |
| 2 | The working universe (mandate §2.3, §5.3) — which is also where a `removed_instrument` restriction lands, interpretation 23 — then the eligibility floor (§3.2 items 1 to 7, in list order), then concentration (§3.3 and the mandate per-instrument cap), then mandate order size, then the re-entry cooldown | E6-3 (universe, concentration, size, cooldown), E6-7 (floor), E6-10 (item 7's USD pairs) | `not_in_working_universe`, `not_in_universe`, `ineligible_exchange`, `ipo_not_tradable`, `below_price_floor`, `below_liquidity_floor`, `leveraged_etp_not_enabled`, `crypto_pair_not_usd` (item 7's pair rule, registered by DEC-255; the gate reports it once E6-10's implementation lands), `concentration_limit`, `max_order_size`, `reentry_cooldown` |
| 3 | Session, auction window, halt (§4.3, §4.4) | E6-6 (sessions), E6-9 (halts) | `session_not_allowed`, `extended_hours_opening_not_allowed`, `auction_window` (the opening auction and market orders only — interpretation 18), `instrument_halted`, and the defers `discretionary_exit_regular_session_only` and `owner_confirmation_required` |
| 4 | Order constraints (§5.3 rules 1 to 9, in list order); `GateInput::pass` decides whether rules 4 to 6 exclude the agent's own protective and resting opening orders (`First`) or apply in full (`BeforeSubmission`) | E6-6 | `would_cross_zero`, `sell_exceeds_available`, `working_order_limit`, `add_blocked_by_protective_order`, `unknown_order_in_flight` (**deny** for an opening, **hold** for a reduction — interpretation 22), `market_order_not_allowed`, and — pending the founder's registry entry, Decisions needed item 3 — rule 2's minimum size and increment |
| 5 | Mark freshness and the price collar (§8.2, §9.6) | E6-8 | `stale_mark`, `price_outside_collar` |
| 6 | Market-conduct controls (§9.6), the close window, and the mandate's orders per day | E6-8 (conduct), E6-3 (`max_orders_per_day`) | `min_resting_time` (on a cancel, through `evaluate_cancel`), `opposite_fill_interval`, `conduct_limit_breached`, `close_window`, `max_orders_per_day` |
| 7 | Buying power (§9.5), then gross exposure (§9.3: the account at 1×, then the agent's mandate limit) | E6-6 (buying power), E6-3 (gross exposure) | `insufficient_buying_power`, `insufficient_settled_buying_power`, `gross_exposure_limit` |
| 8 | Day-trade budget (§9.2) | E6-6 | `legacy_pdt_day_trade_budget` |

`order_rate_limited` is **not** in this table: §9.7's platform per-second and per-day counters
belong to the runtime (stream I), which sets the `rate_limit` restriction that reaches check 1 as an
agent mode. Only the mandate's own `max_orders_per_day` is the gate's, at check 6.

**The closing auction window and the close window are the same ten minutes** (§4.3: "closing: the
last 10 minutes of the regular session"; §9.6: the close window, `close_window_minutes: 10`), and
check 3 runs before check 6, so a naive reading reports `auction_window` where `RC-25` step 2
expects `close_window`. Interpretation 18 settles it: check 3's `auction_window` covers the
**opening** auction (09:28 to 09:30 ET) and **market orders** in either window, and the denial of an
opening or increasing order in the closing ten minutes is check 6's `close_window`, which is exactly
how the reason registry annotates the two.

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
| Sell, quantity ≤ the agent's position | `OwnerClose`, `OwnerKillSwitch` | `OwnerExit` |
| Sell, quantity ≤ the agent's position | `OrderBuilder`, `GoalCompletion`, `RemovedInstrument` | `DiscretionaryExit` |
| Sell, quantity > the agent's position | any | denied `would_cross_zero` (§5.3 rule 3) |

A buy is never an exit, whatever the proposer says, and there is no v1 path by which a buy reduces
risk: v1 holds no shorts (DEC-32), so `would_cross_zero` and the sell-side table are the whole of
it. A protective leg's quantity is checked against position + entry quantity (§5.3). A sell in an
instrument the agent holds nothing in is the `quantity > position` row, so it is denied
`would_cross_zero` at check 4 and needs no error of its own.

**Where the assignment is exercised, and where it is not.** `ref.py`'s `gate` takes `prop["purpose"]`
as given and returns `allow` for any reducing purpose before it looks at a position, which is why
`MC-G15` allows a `discretionary_exit` of quantity 1 in an instrument with no `positions_mv` entry.
The `kind: gate` harness therefore passes the case's `purpose` through as an **already-assigned**
purpose and runs only the mandate-limit checks that `gate` implements (interpretation 19). Purpose
assignment and check 4's `would_cross_zero` are exercised by the trading-domain cases — `RC-03`'s
`gate_rejects_zero_crossing_order`, already passing — and by hand tests over `Origin`, side, and
position. Nothing in the `kind: gate` path can therefore turn `MC-G15` into a denial.

### The exemptions, in one place

This is MI-1, [AGENTS.md](../../../AGENTS.md) rule 13, and the reason the gate exists in this shape.

**The mode rule, stated once**, exactly as `ref.py`'s `order_decision` implements it, because
interpretation 2 makes it the fuzz's oracle for MI-1:

```
hold, reason agent_<mode>, when
      mode == stopped
   or mode == paused and not (purpose == Protective
                              or (purpose in {RiskExit, OwnerExit} and from a kill switch))
```

So `Protective` is **never** held by a mode — protection that cannot be placed leaves a position
unprotected, which is the opposite of the safe default — `RiskExit` and `OwnerExit` **are** held by
`paused` unless they are a kill switch's own orders — `Origin::AutomatedKillSwitch` or
`Origin::OwnerKillSwitch`, which is why `Origin` distinguishes a kill switch from a risk-engine exit
and from an owner's ordinary close (trading spec §5.5: "Kill-switch orders are exempt from the
agent's mode"; an owner closing one position under `paused` is not a kill switch and is held) — `DiscretionaryExit` is held by `paused`, and everything is held by
`stopped`. Trading spec §7.4's prose is narrower for the protective case ("no new orders except
re-placing protection before expiry"); interpretation 17 records why the crate follows `ref.py`.

| Purpose | Exempt from | Paced by | Held by mode | Also held by |
|---|---|---|---|---|
| `Protective` | every conduct control, eligibility, the day-trade budget, buying power, and the opening-session rules | nothing | `stopped` only | an `Unknown` order in the instrument; the broker |
| `RiskExit` (risk engine, automated kill switches, `trim_to_target`, the stop watchdog) | all of the above, and §9.6 entirely | nothing | `stopped`, and `paused` unless it is a kill switch's own order | an `Unknown` order; the broker |
| `OwnerExit` | all of the above | the participation caps only | `stopped`, and `paused` unless it is a kill switch's own order | an `Unknown` order; the broker. Outside the regular session an equity sell is **deferred** until the owner confirms the displayed bid, then allowed with a floor price |
| `DiscretionaryExit` | denial: it is **never** denied by a conduct control, the eligibility floor, a day-trade budget, buying power, or an opening-session rule | the collar prices it, the participation caps slice it, the close window makes it a marketable limit, and outside the regular session an equity one is **deferred** | `paused`, `stopped` | an `Unknown` order; the broker |
| `Open`, `Increase` | nothing | nothing | `exits_only`, `paused`, `stopped` | every check below |

A `Defer` is never converted to a `Deny` (§9.1), and a deferred intent is not stored: the order
builder proposes again at the next evaluation and at the regular-session open. The kill switch is
always available, touches only its scope, and does not depend on model state.

**What MI-1 does and does not promise.** MI-1's own words are "risk reduction is never denied by a
**mandate limit, conduct control, session rule, or instrument restriction**", and its list of things
that may hold an exit ends with "or the broker". An account the broker has blocked (§7.3 status
other than `ACTIVE`, `trading_blocked`, `account_blocked`, `trade_suspended_by_user`) is that broker
arm: check 1 denies **every** purpose with `account_trading_blocked`, a risk exit included, which is
what `RC-15`'s `status_not_active` variant expects. That is not the platform denying an exit by a
limit; it is the platform reporting that the venue is shut to this account, and sending the order
anyway would only collect a reject. The `closing_only` state is the opposite case and still allows
every exit (`RC-15` step 4). The fuzz property is scoped to MI-1's own words accordingly.

## The drawdown ladder as a pure function

E6-4 in this crate is what the order path *does* with the ladder; stream F folds the ladder's state
from the account stream. Three pure functions read that state and nothing else:

1. **The size factor** — the product of the active `scale_sizes` rungs' factors (mandate §5.5) —
   is **read from the risk snapshot, not computed here**: it is folded risk state, stream F's
   `Snapshot` carries it, and one number folded in two crates is one number two crates can disagree
   about (the coordinator's ruling on the #136 review; see Dependencies). `scale_action:
   limit_buys` multiplies the order-builder target by it, which stream H applies.
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
   position (a zero quantity produces no sell). **Every sell it does emit is priced by the session
   alone** — `market_or_ladder` in the regular session, `exit_price_ladder` outside it — which is
   `ref.py` exactly and is what separates `MC-F01`'s crypto sell (regular, `market_or_ladder`) from
   `MC-F02`'s and `MC-F03`'s (after hours, `exit_price_ladder`). Outside the regular session an
   automated flatten defers equity sells to the regular-session open and sends crypto now; an owner
   kill switch with a confirmed bid sells equities now through the exit price ladder with
   `floor_price = confirmed_bid × (1 − max_exit_offset)`, or an explicit `owner_floor_price` when
   the owner supplied one (`ref.py` takes the override first), and a remainder that rests at the
   floor, and without confirmation defers them. This function decides; the executor (E7-2) carries it out.

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
| No opening order within 60 s after an opposite-side fill | `ConductState::last_opposite_fill_at` | `now − last ≤ 60 s` (the interval includes its last instant, DEC-163 item 3) |
| Close window (last 10 minutes of the regular session) | the `SessionAt` the gate derives from `now` | `now ≥ session_end − close_window_minutes`, with the early-close calendar giving `session_end` |
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
- **MI-1, the other half,** scoped to MI-1's own words. For every generated state and every
  reducing purpose, assert the verdict is never `Deny` **by a mandate limit, a conduct control, a
  session rule, an instrument restriction, the eligibility floor, a day-trade budget, or buying
  power** — the property matches on the reason code, so a deny carrying `account_trading_blocked`
  (the broker arm of MI-1's own list, `RC-15`'s `status_not_active`) is the one permitted denial and
  is asserted to be reachable, not merely tolerated. Assert further that a `Hold` carries only
  `agent_paused`, `agent_stopped`, or `unknown_order_in_flight`; that the two `agent_*` holds follow
  the mode rule above exactly (the mode rule has no `Unknown`-order arm, which is interpretation
  22's, at check 4); and that a `DiscretionaryExit` is never denied at all. Generators must produce a `paused` mode, a `stopped`
  mode, an `Unknown` order, and a blocked account, so no arm of the property passes vacuously.
- **MI-8.** Every generated input is evaluated twice, and the two `Decision`s compared field by
  field, including the check list's order.
- **Anti-vacuity.** A coverage assertion requires the run to have produced at least one allow, one
  deny of each mandate limit, one defer, and one hold; a fuzz that only ever denies proves nothing.

## Spec clause → test

One row per clause the crate must hold, with the tests the tests PR writes for it. Property names
are `crates/mandate-risk/tests/properties.rs` unless another file is named; `hand` is `tests/hand.rs`,
`fuzz` is `tests/fuzz.rs`, and a case id is the reference case itself.

| Spec clause or invariant | Test |
|---|---|
| §9.1 the eight checks run in order and the **first** failure's code is reported | `properties::the_first_failing_check_decides`, `hand::two_simultaneous_failures_report_the_earlier_check`, `RC-16` (floor before concentration), `RC-25` (close window after the session) |
| §9.1 every decision, allows included, carries the checks evaluated | `properties::every_decision_lists_the_checks_it_reached`, `hand::checks_after_the_failure_are_not_reached` |
| §9.1 purpose is assigned by the gate, never taken from the proposer | `hand::purpose_is_assigned_from_origin_side_and_position` (one case per row of the purpose table), `properties::a_buy_is_never_an_exit` |
| §9.1 verdicts are allow, deny, or defer, and a defer is never converted to a deny | `properties::a_discretionary_exit_is_never_denied`, `RC-25` steps 5, 7, 8 |
| MI-1 risk reduction is never denied by a mandate limit, conduct control, session rule, or instrument restriction | `fuzz::mi1_reduction_is_never_denied_by_a_limit`, `MC-G08`, `MC-G09`, `MC-G10`, `MC-G15`, `RC-09B` step 4, `RC-15` step 4, `RC-16` step 8, `RC-25` steps 4 and 6 |
| MI-1 an exit is held only by `paused`, `stopped`, an `Unknown` order, or the broker | `properties::a_hold_follows_the_mode_rule_exactly`, `hand::a_kill_switch_order_is_exempt_from_paused`, `hand::a_protective_order_is_never_held_by_a_mode`, `hand::an_unknown_order_denies_an_opening_and_holds_a_reduction`, `RC-15::status_not_active` (the broker arm) |
| MI-6 a stricter mode never turns a deny into an allow | `properties::a_stricter_mode_is_never_more_permissive` |
| MI-8 identical inputs give identical decisions | `fuzz::mi8_identical_inputs_give_identical_decisions` |
| MI-15, MI-19, MI-20 nothing opens outside the working universe, and a removed instrument is exits-only in that instrument | `MC-G14`, `MC-G15`, `MC-G16`, `properties::an_opening_needs_the_working_universe`, `hand::a_removed_instrument_restricts_only_itself` |
| mandate §2.3 an absent working universe is an error, an empty one denies | `hand::an_absent_working_universe_is_an_error`, `MC-G16` |
| mandate §5.3 per-instrument cap = min(usd, fraction × E), counting position + working + proposed | `MC-G01`, `MC-G02`, `MC-G05`, `properties::position_cap_is_the_lower_of_both_bounds`, `fuzz` oracle |
| mandate §5.3 order size, orders per day, gross exposure bounded by E | `MC-G03`, `MC-G04`, `MC-G06`, `MC-G07`, `properties::gross_exposure_is_bounded_by_equity`, `hand::a_rejected_order_still_counts` |
| mandate §5.3 re-entry cooldown across the instrument group | `MC-G11`, `MC-G12`, `MC-G13`, `properties::cooldown_covers_the_whole_group` |
| mandate §5.3 these limits never deny an exit | `MC-G08` to `MC-G10`, `MC-G15`, `fuzz::no_allowed_sequence_ever_exceeds_a_drawn_mandates_limits` |
| mandate §5.5, trading §5.5 the agent flatten: mode first, own order ids, no cancel-all or close-position, sub-ledger quantity | `MC-F01`, `properties::an_agent_flatten_never_touches_another_agent`, `hand::the_final_mode_is_applied_first` |
| mandate §5.5 automated flattens defer equity sells outside the regular session; crypto goes now | `MC-F02`, `hand::a_crypto_sell_never_waits_for_a_session` |
| trading §5.5 an owner kill switch prices to a floor once the bid is confirmed, and defers without it | `MC-F03`, `MC-F04`, `hand::an_explicit_owner_floor_price_overrides_the_computed_one` |
| mandate §5.5 the size factor is the product of the active rungs | `hand::two_active_rungs_multiply`, `hand::two_active_rungs_multiply` |
| mandate §5.5 `trim_to_target` sells down to factor × cap, rounded up, regular session only, never while Holding | `hand::a_trim_rounds_up_to_the_increment`, `hand::a_trim_waits_for_the_regular_session`, `hand::no_trim_while_holding`, `properties::a_trim_never_sells_below_the_target` |
| mandate §5.9 the effective mode is the strictest restriction, and openings stop at `exits_only` | `RC-22` last step, `properties::a_stricter_mode_is_never_more_permissive` |
| trading §3.2 the eligibility floor, items 1 to 7 in list order | `RC-16` steps 1 to 7, `hand::the_floor_reports_the_first_failing_item` |
| trading §3.2 item 7 a crypto opening needs a USD pair: another or an unstated quote currency is denied at check 2, after items 1 and 3 and before the 30-day volume; an exit in any pair and a US equity are never judged by it (DEC-254) | `usd_pairs::a_crypto_opening_in_a_pair_not_quoted_in_usd_is_denied_at_check_2`, `usd_pairs::a_crypto_opening_in_a_usd_pair_passes_the_floor`, `usd_pairs::check_2_is_whole_for_crypto`, `usd_pairs::a_crypto_exit_in_any_pair_is_never_denied_by_the_pair_rule`, `usd_pairs::a_us_equity_is_not_judged_by_its_quote_currency`, `usd_pairs::an_inactive_crypto_account_is_reported_before_the_pair`, `usd_pairs::the_pair_rule_denies_exactly_a_crypto_opening_not_quoted_in_usd` |
| trading §3.2 item 6 an unclassified or stale ETP fails closed | `hand::an_unclassified_etp_is_complex`, `hand::a_stale_classification_denies_an_etp_opening`, `properties::etp_fails_closed`, `RC-16::leveraged_etps_enabled` |
| trading §3.2 the floor never applies to a risk-reducing order in a held instrument | `RC-16` step 8, `fuzz::no_allowed_sequence_ever_exceeds_a_drawn_mandates_limits`, `hand::the_floor_never_blocks_an_exit_in_a_held_instrument`, and in-module `gate::tests::no_floor_breach_denies_an_exit_from_any_origin` (every sell origin, all ten, against every universe and floor breach) |
| trading §4.3 sessions: regular-session openings, extended-hours exits as limit orders, nothing overnight | `hand::an_opening_outside_the_regular_session_is_denied`, `hand::an_extended_hours_opening_needs_a_limit`, `RC-25` |
| trading §4.3 a market opening in either auction window is `auction_window` and a market exit there is re-priced (DEC-159); the closing ten minutes are `close_window` | `RC-25` step 2, `hand::an_auction_window_denies_a_market_opening_and_reprices_a_market_exit`, `hand::the_close_window_follows_the_early_close_calendar` |
| trading §4.4 a halt, and a dropped status feed as a presumed halt | `hand::a_halted_instrument_denies_an_opening`, `hand::a_dropped_status_feed_is_a_presumed_halt` |
| trading §5.3 rules 1 to 9 in list order, and the first-pass exclusion of the agent's own orders | `hand::order_constraints_report_the_first_failing_rule`, `hand::the_first_pass_excludes_the_agents_own_protective_orders`, `RC-03::gate_rejects_zero_crossing_order` |
| trading §7.3 account states: `blocked` denies every purpose, `closing_only` allows exits | `RC-15` and its three variants, `hand::a_blocked_account_holds_even_a_risk_exit`, `hand::three_consecutive_unexplained_403s_restrict_the_account` |
| trading §8.2 an opening order needs a fresh quote-based risk mark; a reducing one takes any source | `hand::an_opening_needs_a_fresh_quote`, `hand::a_risk_exit_accepts_a_last_trade_mark`, `properties::mark_freshness_never_blocks_a_reduction` |
| trading §9.2 the `legacy_pdt` window, count, `remaining`, and `required` | `RC-09`, `RC-09B`, `hand::the_window_is_today_plus_four`, `hand::crypto_never_counts`, `properties::required_counts_every_component` |
| trading §9.2 `intraday_margin` denies no 1× long-only opening | `RC-09::alpaca_intraday_margin`, `hand::a_reported_deficit_is_an_account_state_not_a_denial` |
| trading §9.3 1× gross exposure including the proposed order; no short sales | `MC-G04`, `MC-G06`, `hand::a_sell_above_the_position_is_would_cross_zero` |
| trading §9.5 buying power is the lower of model and broker, with the fee reservation, subtracted once | `RC-08` step 3, `RC-18` step 3, `RC-18::generic_cash_account`, `properties::buying_power_is_the_lower_of_the_two`, `hand::a_reservation_includes_the_rounded_fee` |
| trading §9.6 the collar binds aggressive prices only, within the passive band, by tier; an unknown median takes the narrower tier | `RC-22` steps 1 and 2, `hand::a_passive_price_inside_the_band_is_allowed`, `hand::a_median_dollar_volume_exactly_at_the_threshold_is_liquid`, and in-module `gate::tests::an_unknown_median_volume_takes_the_narrower_collar` (DEC-163 item 2) |
| trading §9.6 a discretionary exit whose collar cannot be computed is routed whole at its own limit, paced by the other controls alone; an opening over such a collar keeps the collar's error, and a proposal of zero is refused as `zero_quantity` before any check (`AGENTS.md` rule 13, DEC-327, DEC-383, DEC-401) | `properties::an_exit_over_extreme_figures_is_still_routed`, and in-module `gate::tests::an_exit_whose_collar_cannot_be_computed_is_paced_by_the_other_controls_alone`, `gate::tests::an_opening_whose_collar_cannot_be_computed_is_never_allowed`, `gate::tests::a_zero_quantity_exit_over_an_uncomputable_collar_is_not_routed`, `hand::a_proposal_of_zero_is_refused_by_name_whatever_else_it_would_be` and `conduct::tests::only_the_two_diagnosed_errors_are_skipped` |
| trading §9.6 the opposite-fill interval, its last instant included | `RC-22` steps 5 and 6, `properties::only_an_opposite_side_fill_starts_the_interval`, and in-module `gate::tests::the_opposite_fill_interval_includes_its_last_instant` (DEC-163 item 3) |
| trading §9.6 minimum resting time on a cancel, exempt before a risk-reducing order | `hand::a_cancel_inside_the_resting_window_is_denied`, `hand::a_cancel_before_a_risk_reducing_order_is_exempt` |
| trading §9.6 participation caps slice a discretionary exit rather than denying it; a zero cap slices nothing and no slice is below `min_order_size` | `properties::a_participation_cap_slices_and_never_denies`, `hand::a_sliced_exit_reports_what_it_applied`, and in-module `gate::tests::a_zero_cap_slices_no_exit_of_any_kind`, `gate::tests::a_cap_below_the_minimum_slices_at_the_minimum` and `gate::tests::an_allowed_exit_is_never_below_its_minimum_or_zero` (DEC-163 item 4) |
| trading §9.6 order-to-fill after 20 orders, compared without dividing | `hand::the_order_to_fill_ratio_needs_twenty_orders`, `properties::the_ratio_is_compared_without_dividing` |
| trading §9.6 self-trade prevention across related accounts | `hand::an_opposite_side_rest_in_a_related_account_blocks_an_opening` |
| trading §9.6 the daily surveillance report states figures and makes no judgement | `hand::the_surveillance_report_matches_a_hand_computed_day`, `properties::the_report_flags_every_threshold_it_crosses` |
| §2.1, ES-04 the new arithmetic is exact or an error, one rounding per formula, never in the order's favour | `mandate-num`'s `num::hand_calculated_gate_bounds`, `num::a_collar_bound_rounds_against_the_order`, `properties::a_value_exactly_at_a_limit_passes` |
| ES-21 determinism: no clock, no randomness, `BTreeMap` only | `fuzz::mi8_identical_inputs_give_identical_decisions`, and clippy's `disallowed-types` on a `pure` crate |
| E6-3 acceptance: fuzzing never produces an order outside limits | `fuzz::no_allowed_sequence_ever_exceeds_a_limit` against the shadow-ledger oracle, with the coverage assertion |

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
   **Where `order_decision` is not the oracle at all.** It models neither §5.3 rule 9 nor an
   `Unknown` order: it takes no such input, and for a reducing purpose it falls through to `gate`,
   whose first line returns `allow` for anything in `REDUCING`. So on an input with an `Unknown`
   order in the instrument it answers `allow` where interpretation 22 requires `Hold`, and the
   generator does produce such inputs. For those inputs the oracle is **§5.3 rule 9 and MI-1
   directly** — deny `Open` and `Increase`, hold every reduction, code `unknown_order_in_flight` —
   not `order_decision`, and the fuzz selects the oracle on whether the generated state has an
   `Unknown` order in the proposed instrument. The same bound applies to any other §9.1 check
   `order_decision` does not model; it is the oracle for the mode, session, close-window, and
   instrument-restriction arms it does.
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
8. **Buying power is the lower of model and broker (DEC-34), and reservations are subtracted
   once.** `Account::buying_power(reservations)` already subtracts them, and the pending fee
   charges besides, so check 7 compares
   `qty × limit_price + round(estimated fees, 2, ceiling) ≤ min(model, broker)` and subtracts
   nothing further; §9.5's "≤ buying power − existing reservations" is the same figure, written
   before DEC-104 moved the subtraction into the fold. `AccountSnapshot` therefore carries no
   separate `reservations` field for check 7 to use, so the double subtraction is unrepresentable
   rather than merely avoided. Every term is exact. A cash account's failure is `insufficient_settled_buying_power` and a margin
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
    **The harness mapping:** schema v3's expectation vocabulary is `allow | deny | defer` with no
    hold, so the trading-domain harness compares a `Hold` against a case's `deny` carrying the same
    reason code. No case in the file expects a verdict for an exit under `paused`, `stopped`, or an
    `Unknown` order, so the mapping is exercised by nothing today and is recorded here so the tests
    PR does not have to invent it. The `kind: gate` harness needs no mapping: `ref.py` has the same
    four verdicts.
13. **The surveillance report is a pure function, and it supervises nothing.** E6-8's daily report
    is `fn surveillance(day: Date, input: &SurveillanceInput<'_>) -> Result<SurveillanceReport,
    GateError>`, where `SurveillanceInput` is the day's folded figures and nothing else:
    `orders: BTreeMap<(AgentId, InstrumentId), OrderCounts { submitted, filled, cancels_excluded }>`
    for the order-to-fill ratios, `opposite_side_rests: BTreeMap<InstrumentId, BTreeSet<AccountId>>`
    for the self-trade checks, `close_window_orders: BTreeMap<(AgentId, InstrumentId), u32>`,
    `end_of_day_market_values: BTreeMap<(AgentId, InstrumentId), Usd>` with each agent's equity for
    concentration, and the `GateConfig` thresholds. Every field is a fold of account-stream events
    the executor already journals, so "pure function of journaled events" is a shape, not a claim:
    self-trade checks, order-to-fill ratios, close-window activity, and concentration, with the
    threshold breaches flagged. §9.6 is explicit that "the platform does not supervise users' trading", so the report
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
    (the fee is paid in the asset). The reservation is a term of check 7's left-hand side only; the
    executor holds the reservation itself, and the fold has already netted the existing ones out of
    buying power (item 8).
17. **A protective order is never held by a mode; a risk or owner exit is held by `paused` unless a
    kill switch sent it.** This is `ref.py`'s `order_decision` exactly. Trading spec §7.4's prose is
    narrower for the protective case ("no new orders except re-placing protection before expiry"),
    but nothing in the gate's input distinguishes a re-placement from a first placement, `ref.py`
    draws no such line, and holding protection under `paused` would leave a position unprotected —
    the opposite of rule 3's safe default. MI-1 is satisfied either way: it says an exit *may* be
    held only by those four things, not that each of them always holds one. The rule is written once
    in "The exemptions, in one place" and the fuzz compares against `order_decision`.
18. **`auction_window` is a market opening in either auction window; the closing ten minutes are
    `close_window` for every other opening** (amended by DEC-159). §4.3's closing auction window and
    §9.6's close window are the same interval and both bar opening orders, and check 3 runs before
    check 6, so without this the first code would always win and `RC-25` step 2 could not pass.
    Check 3 therefore emits `auction_window` only for a **market opening** (open or increase) in the
    09:28 to 09:30 opening auction or in the closing ten minutes, and the denial of a limit opening
    or increase in the closing ten minutes is check 6's `close_window`, which is how the reason
    registry annotates the two. A market-order **exit** of any purpose in either window is not
    denied: it is allowed and sent as a marketable limit (`Pacing::marketable_limit_required`), as
    items 28 and 31 do under a halt, because §4.3's "exits in them use limit orders, never market
    orders" constrains an exit's form and MI-1 forbids a session rule to deny it (DEC-159). Check 3
    reads the session first, so any opening at 09:28 is in pre-market and its session rule denies
    it as `session_not_allowed`; the reachable `auction_window` denial is a market opening in the
    closing ten minutes.
19. **The `kind: gate` harness runs `ref.py`'s `gate`, not the whole of §9.1** (DEC-85 style). Those
    cases carry a `purpose`, a `state`, and a `proposed` order and nothing that could feed checks 1,
    3, 4, 5, or 8 — no session, no quote, no account status — so the harness passes the case's
    `purpose` through as already assigned and runs only the mandate-limit checks `gate` implements.
    Purpose assignment, `would_cross_zero`, sessions, the collar, and the day-trade budget are
    exercised by the trading-domain cases and by hand tests. Anything a mandate case states that
    this path does not read stays "not interpreted until <story>".
20. **A cancel is a gate decision too.** §9.6's minimum resting time is a rule about canceling a
    non-marketable opening order, and its exemption ("does not apply to cancels that precede a
    risk-reducing order") is a risk judgement, not an executor convenience. So the crate exposes
    `evaluate_cancel` beside `evaluate`, and the executor asks before it cancels. Putting the rule
    in the executor would place a conduct control outside the independent gate, which item 5 already
    rules out for every other control.
21. **An agent flatten prices every sell by the session alone:** `market_or_ladder` in the regular
    session, `exit_price_ladder` outside it. This is `ref.py`'s `agent_flatten` exactly, and it is
    the whole difference between `MC-F01`'s crypto sell (regular) and `MC-F02`'s and `MC-F03`'s
    (after hours). The asset class decides whether a sell is *deferred*, never how it is priced.
22. **An `Unknown` order denies an opening and holds a reduction, under one code.** §5.3 rule 9 says
    "no new orders in that instrument (`unknown_order_in_flight`) until resolved", and MI-1 lists an
    `Unknown` order among the things that may **hold** an exit, beside `paused` and `stopped`. Both
    are true at once only if the verdict depends on the purpose: `Deny` for `Open` and `Increase`,
    `Hold` for every reducing purpose, with `unknown_order_in_flight` as the code either way. Any
    other reading breaks something — denying the exit contradicts MI-1's own wording, and letting it
    through contradicts rule 9 and risks doubling a position whose true size is unknown. So the
    fuzz's `Hold` codes are `agent_paused`, `agent_stopped`, and `unknown_order_in_flight`, while
    `account_trading_blocked` stays the only denial a reducing purpose may carry.
23. **A `removed_instrument` restriction is the working-universe check, not a separate one.** An
    instrument becomes `removed_instrument` exactly when it leaves the working universe (mandate
    §2.3's state machine and §5.9), so the same fold produces both and check 2 reports it as
    `not_in_working_universe`, which is registered. That matters because `removed_instrument`
    itself is **not** in the trading-domain reason registry, and inventing a code would break
    ES-09's promise that a reason code is stable. `stale_mark` is the other instrument restriction
    and is registered: it is evaluated at check 5 with mark freshness, which is where §9.1 puts it.
    `ref.py`'s `order_decision` returns the restriction's own name because it models the two as one
    parameter; the split by code is this crate's, and it changes no verdict.
24. **A real halt and a presumed halt are different denials, under different codes.** §4.4 says a
    halted or paused instrument takes "no new opening orders", while a stale quote or dropped status
    feed is "treated as a presumed halt: no market orders; exits use marketable limit orders". The
    two effects differ, so the codes do: a halt is `instrument_halted` at check 3 and bars the
    opening outright; a presumed halt leaves openings alone and refuses only market orders, which is
    check 4's `market_order_not_allowed`. Collapsing them onto one code would either bar openings a
    presumed halt permits or permit market orders a halt does not.
28. **A presumed halt re-prices an exit; it never denies one.** §4.4's sentence has two clauses —
    "no market orders; exits use marketable limit orders" — and §5.6 lists presumed halts among the
    conditions where an exit that must be marketable takes the exit price ladder. So a risk or
    owner exit proposed as a market order under a dropped status feed or a stale quote is allowed
    with `Pacing::marketable_limit_required` set, while a market order to open or increase is
    denied at check 4 as `market_order_not_allowed`. Reading only the first clause would put an
    instrument restriction in front of a risk exit, which MI-1 names outright. Numbered 28 rather
    than beside 24 because renumbering accepted items invalidates references already written
    against them.
29. **A partial gate fails closed for adding risk** (the coordinator's ruling on #157). While any
    check, or part of one, is owed by a later PR or story, an opening or increasing order every
    implemented check would allow returns `GateError::Unimplemented("evaluate", <story>)`, naming
    the story that completes the first owed check; a denial or hold from an implemented check
    reports first, and an owed check is listed `NotReached` for every purpose, since the gate did
    not look. A reducing purpose passes a check that
    does not exist yet (`AGENTS.md` rule 13; the broker is the backstop). A test that needs an
    allowed opening stays pending until the PR that completes the last check it passes through.
30. **Every buy is an opening and every sell above the position is an opening, a protective leg's
    included** (DEC-32: v1 holds no short). The purpose table's "any side, `ProtectiveLeg`" row is
    therefore a *sell within the position*: a protective "buy" adds risk, and a protective sell
    above the position would open a short if it triggered, so it is typed `Open` and check 4 denies
    it `would_cross_zero`, which is §9.1's own rule ("a sell above the position is denied").
    §5.3's "bracket protective legs are checked against position + entry quantity" sits with rules
    4 to 6 and is rule 4's (`sell_exceeds_available`, E6-6). Under `paused` or `stopped` such a sell is held at check 1 before check 4.
31. **An exit is re-priced under a real halt as well as a presumed one** (the coordinator's ruling on
    #186). §4.4, §5.6 and item 28 attach re-pricing to the presumed halt, and §5.1 permits a market
    exit "with current status data", which a real halt has. The gate still sends a market-order exit
    in a halted instrument as a marketable limit, because a market order into a halt reopens at an
    unknown price. It is rule 3's safe direction and never a denial: the exit is allowed and only
    its form is constrained, so MI-1 and rule 13 hold.

## Decisions needed

None can be taken by an agent: each would change a founder-owned file
([AGENTS.md](../../../AGENTS.md) rule 9, ES-22). The three below are DEC-129 items 25, 26, and 27,
each `Proposed (founder)`.

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
3. **§5.3 rule 2 denies an order with no registered reason code.** "Quantity ≥ `min_order_size`
   and a multiple of the increment, except a sell closing the full position" is a check-4 denial,
   but the `reason_codes` registry in
   [`trading-domain.yaml`](../../specs/reference-cases/trading-domain.yaml) has no code for either
   half, so the gate would have to invent one and ES-09's "stable reason code" would not be stable.
   No case exercises it today. **Recommendation:** register `below_min_order_size` and
   `quantity_off_increment` under "order constraints" in the next spec window. Until then the tests
   PR's hand tests assert the *verdict* and leave the code unasserted rather than minting one.

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
- **The first implementation PR takes `mandate-domain`'s and `mandate-spec`'s types and deletes
  `spec_types.rs`** (the coordinator's ruling on the #136 review round 2). Stream F's #140 confirms
  the shapes match: `WorkingUniverse` is `mandate-domain`'s exactly, F's `Snapshot` carries the same
  field names, and `mandate-domain` also owns `Purpose`, `Side`, `AgentMode` and `AssetClass`, which
  this crate currently takes from `mandate-accounting`. The two differences are **settled**: F's
  `size_factor` is a `Ratio` and F folds `active_rungs` as a `BTreeMap<u8, u64>` of rung to active
  seconds, both of which `spec_types.rs` now mirrors, so the implementation PR only deletes the
  module rather than converting anything; F's `AssetId::parse` is a strict uuid, which this crate
  takes over its interim non-empty-string `new`. A map rather than a set beside a second map is the
  better shape anyway: it cannot hold a rung that is active with no duration, or a duration for a
  rung that is not.
- **`size_factor` is stream F's, not this crate's** (the coordinator's ruling on the #136 review,
  amending interpretation 1's reading of E6-4). It is folded risk state and F's `Snapshot` already
  carries it, so computing it here as well would let two crates disagree about the same number.
  `RiskSnapshot` therefore **reads** `size_factor`, and reads each rung's active seconds from F's
  `active_rungs` map — which §5.5's `breach_confirm_s` trim guard needs — rather than keeping a
  second field of its own.
- **Stream H (`mandate-builder`)** consumes `size_factor` and proposes the orders MI-9 asserts the
  gate never denies. The dependency runs H → G (layer 5 depends on layer 4), so nothing here waits
  on H. It also consumes the **gate dry run**: `ref.py`'s `builder` calls `gate` on its own proposal
  and records the verdict as `gate_dry_run`, skipping the proposal (`autonomy: skipped, by
  gate_dry_run`) when it denies, which the 20 `MC-B` cases that state a `gate_dry_run` pin. The dry
  run is **`evaluate` itself** — the gate is pure and has no side effects, so there is no separate
  entry point and no risk of the dry run and the real call disagreeing. Stream H's DEC-130 item 2
  passes the verdict in as a value rather than depending on this crate; either way the value must be
  one `evaluate` produced, and MI-9 is the property that it is never a deny.
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
| 7 | An exit is run through the mandate limits instead of returning early | `MC-G08`, `MC-G09`, `MC-G10`, `MC-G15`, `properties::mi1_reduction_is_never_denied_by_a_limit` |
| 8 | An absent working universe is treated as "everything allowed" | `hand::an_absent_working_universe_is_an_error`, `properties::mi1_reduction_is_never_denied_by_a_limit` cannot catch it, which is why the hand test exists |
| 9 | An empty working universe is treated as absent (an error) rather than as denying | `MC-G16` |
| 10 | The eligibility floor runs its items out of §3.2 order, so `below_price_floor` is reported where `ineligible_exchange` should be | `RC-16` steps 2 and 3, `hand::the_floor_reports_the_first_failing_item` |
| 11 | An unclassified ETP is treated as plain | `hand::an_unclassified_etp_is_complex`, `properties::etp_fails_closed` |
| 12 | The eligibility floor is applied to a `risk_exit` in a held instrument | `RC-16` step 8, `MC-G15`, `properties::mi1_reduction_is_never_denied_by_a_limit` |
| 13 | `account_trading_blocked` is treated as `exits_only` rather than blocking the account, so a `risk_exit` is allowed | `RC-15::status_not_active`, `hand::a_blocked_account_holds_even_a_risk_exit` |
| 14 | The unexplained-403 counter resets on an unrelated success, so the threshold is never reached | `RC-15::unexplained_403s`, `hand::three_consecutive_unexplained_403s_restrict_the_account` |
| 15 | The day-trade window is the four prior trading days without today | `RC-09`, `hand::the_window_is_today_plus_four` |
| 16 | `required` omits open same-day positions | `RC-09B` step 3, `properties::required_counts_every_component` |
| 17 | Crypto counts as a day trade | `RC-09` step 3, `hand::crypto_never_counts` |
| 18 | The day-trade budget denies an exit | `RC-09B` step 4, `properties::mi1_reduction_is_never_denied_by_a_limit` |
| 19 | `intraday_margin` inherits the `legacy_pdt` budget | `RC-09::alpaca_intraday_margin` |
| 20 | The collar is applied to passive prices as well as aggressive ones | `RC-22` step 2, `hand::a_passive_price_inside_the_band_is_allowed` |
| 21 | The collar's tier threshold compares with `>` where the spec says `≥ 50 M` | `hand::a_median_dollar_volume_exactly_at_the_threshold_is_liquid` |
| 22 | The opposite-fill interval is checked in the wrong direction (a same-side fill blocks) | `RC-22` steps 5 and 6, `properties::only_an_opposite_side_fill_starts_the_interval` |
| 23 | The close window is computed from 16:00 on an early-close day | `hand::the_close_window_follows_the_early_close_calendar` |
| 24 | The close window denies a discretionary exit instead of pacing it | `RC-25` step 3, `properties::mi1_reduction_is_never_denied_by_a_limit` |
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
| 38 | `size_factor` sums the rungs' factors instead of multiplying them | `hand::two_active_rungs_multiply`, `hand::two_active_rungs_multiply` |
| 39 | `trim_to_target` rounds the sell quantity down to the increment | `hand::a_trim_rounds_up_to_the_increment` |
| 40 | The decision is not deterministic: the check list is built from a `HashMap` iteration | `properties::mi8_identical_inputs_give_identical_decisions` (and clippy's `disallowed-types`, which is why the crate is `pure`) |
| 41 | `paused` holds a protective order, or lets a non-kill-switch `risk_exit` through | `hand::a_protective_order_is_never_held_by_a_mode`, `hand::a_paused_agent_holds_a_plain_risk_exit`, `properties::a_hold_follows_the_mode_rule_exactly` |
| 42 | A kill switch's own `risk_exit` is held by `paused` | `hand::a_kill_switch_order_is_exempt_from_paused`, `properties::a_hold_follows_the_mode_rule_exactly`. Not `MC-F02`: `agent_flatten` takes no mode and never runs the mode rule, so no `kind: agent_flatten` case can catch this |
| 43 | The closing ten minutes report `auction_window` instead of `close_window` | `RC-25` step 2, `hand::an_auction_window_denies_a_market_opening_and_reprices_a_market_exit` (the other half: a market opening at 15:55 ET still reports `auction_window`) |
| 44 | Check 7 subtracts reservations a second time from `Account::buying_power`'s figure | `RC-18` step 3 (449.97 after the reservation, not 449.97 less it again), `properties::buying_power_is_the_lower_of_the_two` |
| 45 | `evaluate_cancel` applies the resting-time rule to a cancel that precedes a risk-reducing order | `hand::a_cancel_before_a_risk_reducing_order_is_exempt`, `properties::a_cancel_that_precedes_a_reduction_is_never_denied` |
| 46 | The `kind: gate` harness reassigns the case's `purpose` from side and position, turning `MC-G15` into a denial | `MC-G15`, `harness::a_gate_case_purpose_is_passed_through` |
| 47 | An owner's ordinary close is treated as a kill switch, so `paused` lets it through | `hand::a_paused_agent_holds_an_owner_close_but_not_an_owner_kill_switch`, `properties::a_hold_follows_the_mode_rule_exactly` |
| 48 | A flatten prices every sell `market_or_ladder`, ignoring the session | `MC-F02`, `MC-F03` (both crypto sells), `hand::a_flatten_prices_by_session_alone` |
| 49 | E6-10: an unstated quote currency (`None`) is admitted as USD | `usd_pairs::a_crypto_opening_in_a_pair_not_quoted_in_usd_is_denied_at_check_2`, `usd_pairs::the_pair_rule_denies_exactly_a_crypto_opening_not_quoted_in_usd` |
| 50 | E6-10: only an unstated quote currency is denied, so a stablecoin pair (`Other`) opens | the same two |
| 51 | E6-10: the pair rule judges a US equity | `usd_pairs::a_us_equity_is_not_judged_by_its_quote_currency`, `usd_pairs::the_pair_rule_denies_exactly_a_crypto_opening_not_quoted_in_usd` |
| 52 | E6-10: the pair rule runs after item 7's 30-day volume, so an illiquid non-USD pair reports `below_liquidity_floor` | `usd_pairs::a_crypto_opening_in_a_pair_not_quoted_in_usd_is_denied_at_check_2` |
| 53 | E6-10: the pair rule runs before item 3, so an `ipo` non-USD pair reports the pair | `usd_pairs::a_crypto_opening_in_a_pair_not_quoted_in_usd_is_denied_at_check_2` |
| 54 | E6-10: the pair rule reports another registered code (`ineligible_exchange`) | `usd_pairs::a_crypto_opening_in_a_pair_not_quoted_in_usd_is_denied_at_check_2`, `usd_pairs::the_pair_rule_denies_exactly_a_crypto_opening_not_quoted_in_usd` |
| 55 | E6-10: the pair rule is placed at check 1 for every purpose, so it denies a crypto exit | `usd_pairs::a_crypto_exit_in_any_pair_is_never_denied_by_the_pair_rule`, `usd_pairs::an_inactive_crypto_account_is_reported_before_the_pair`, `usd_pairs::check_2_is_whole_for_crypto`, and the two above |
| 56 | E6-10: check 2 is left owed for crypto after the rule exists | `usd_pairs::a_crypto_opening_in_a_usd_pair_passes_the_floor`, `usd_pairs::check_2_is_whole_for_crypto`, `usd_pairs::the_pair_rule_denies_exactly_a_crypto_opening_not_quoted_in_usd`, `hand::crypto_never_counts` |

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

- the spec is ambiguous or seems wrong (the three found so far are under Decisions needed);
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
