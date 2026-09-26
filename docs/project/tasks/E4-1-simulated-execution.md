# Task: E4-1 Simulated execution (backtest fill model)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. This story opens milestone M3; E4-2 (baseline backtest and metrics) builds on it.

## Story

- **Story:** E4-1 ([backlog](../06-backlog-v1.md#e4-simulated-execution-and-backtest))
- **Acceptance criteria (verbatim):** "As a researcher, I want market and limit orders filled with
  configurable slippage and fees so that backtests are realistic." The backlog gives no separate
  acceptance line; RC-10, RC-12, and RC-19 and the invariants below are the acceptance test.
- **PRD / HLD / spec anchors:** trading domain spec §6.4 (the backtest fill model, rules 1 to 9),
  §2.1 (numbers: prices at most 9 places; backtest fill prices not tick-rounded), §4.1 (a bar
  carries its session), §4.2 (a missing bar means no trade), §4.3 (sessions), §4.4 (halts),
  §5.1 and §5.2 (which order types and sessions v1 uses), §6.1 (fill record), §14 (RC-10, RC-12,
  RC-19); HLD §6.A step 4 (a backtest is required before an agent trades live) and "Agent
  lifecycle" (Draft → Backtest → Paper).
- **Decisions that apply:** DEC-29 and DEC-37 (v1 order policy), DEC-35 (data profiles), DEC-72
  (ADR-0001), DEC-77 (tests PR, implementation PR, status PR), DEC-79, DEC-80 (no plain comments),
  DEC-83 (tests PRs hold stubs only), DEC-85 (harness interpretations fail loudly until owned),
  DEC-97 (backtests are the evidence an owner sees before go-live), and DEC-106 (recorded by this
  story).

## Scope

- **Reference cases that must move from pending to passing:** `trading_domain::RC-10`,
  `trading_domain::RC-12`, `trading_domain::RC-19`. They are pending today on the case-level keys
  `bars`, `orders`, and `isolation`, which `crates/mandate-refcases/src/trading_domain.rs` already
  assigns to E4-1. Every other case keeps its state.
- **Fixture check before any code:** each of the 14 expected fills in the three cases was
  recomputed by hand from §6.4 with `test_default` (s = half-spread 1 bps + impact 2 bps = 3 bps,
  volume cap 10% of the previous bar, zero latency). All 14 match. For example, RC-10
  `volume_cap`: bar 1 caps at 10% × 5000 = 500 at min(100.60, 100.00 × 1.0003) = 100.03; bar 2
  caps at 800 and fills the remaining 500 at min(100.60, 100.40 × 1.0003) = 100.43012. RC-12
  `stop_gap_through`: open 98.50 ≤ 99.00, so the fill is 98.50 × 0.9997 = 98.47045.
- **Invariants touched** (each gets a named property test in the tests PR, with an oracle that
  computes the answer its own way):

  | Clause | Planned test |
  |---|---|
  | §6.4.1 nothing fills before the first bar starting at or after decision + latency (+ approval latency), and never on the bar that produced the decision | `properties::no_fill_before_eligibility` |
  | §6.4.3 per bar and instrument, Σ fills over the account's orders ≤ truncate(fraction × reference volume, increment), allocated in submission order; 0 when the reference is unavailable | `properties::fills_never_exceed_the_shared_volume_cap` |
  | Σ fills of an order ≤ its quantity; every fill is positive and a multiple of the increment | `properties::orders_never_overfill` |
  | §6.4.5 a touch is not a fill (resting limit, extreme equal to the limit) | `properties::a_touch_is_never_a_fill` |
  | §6.4.5 a buy limit never fills above its limit and a sell limit never below it | `properties::limit_prices_are_never_violated` |
  | Slippage and rounding never flatter a fill: a buy never pays less, and a sell never receives more, than the slippage-free price (DEC-106) | `properties::slippage_and_rounding_are_adverse` |
  | §6.4.2, §6.4.6 fills only in sessions the order may trade in; equity stops trigger only in the regular session | `properties::fills_respect_sessions` |
  | §6.4.2 a day order has no fill after its last eligible session ends | `properties::day_orders_expire_at_session_end` |
  | §6.4.8 at most one OCO leg fills; the first fill of either leg cancels the other (DEC-106) | `properties::oco_fills_at_most_one_leg` |
  | Replay: the same inputs give identical fills | `properties::identical_inputs_give_identical_fills` |

  **Oracle.** `properties.rs` will hold a separate simulator. It works in `i128` at 10⁻⁹ for prices
  and quantities, computes slippage as integer basis points, and walks bars and orders in a
  different loop structure (order-major rather than bar-major). It will be shown to fail on planted
  bugs before it is trusted: a fill at the touch, the decision bar filling, the cap not shared
  across orders, sell slippage with the sign reversed, a stop triggering pre-market, and both OCO
  legs filling. Hand tests in `hand.rs` reproduce each case of RC-10, RC-12, and RC-19, with the
  arithmetic in each doc comment.

- **Crates in scope:** new `mandate-sim` (layer 6 per `xtask/layers.toml`'s plan; `pure = true`;
  `safety_critical = true`, DEC-106 item 1; `allowed_external = ["thiserror"]`; CODEOWNERS line).
  It depends on `mandate-num`, `mandate-time`, and `mandate-accounting` (for `InstrumentId`, `Side`,
  `Liquidity`, and `AssetClass`, so fills map one to one onto `Execution`). `mandate-refcases`
  gains the interpretations for `bars`, `orders`, `isolation`, `first_bar_reference_volume`,
  `resting_since_bar`, and `resting_since`, for RC-10, RC-12, and RC-19 only.
- **Crates out of scope:** `mandate-accounting` (read-only use; E3-3 owns its changes),
  `mandate-marketdata` (E2-4 records sessions with the data; this story takes the session from the
  bar), `mandate-cli`.
- **New dependencies allowed:** none. `thiserror` and `proptest` are registered; the
  `docs/dependencies.md` "Used by" cells gain `mandate-sim`.
- **Safety-critical:** yes (DEC-106 item 1). DEC-77 sequence: tests PR (crate, API stubs,
  pending tests, harness interpretations), implementation PR (test files change only by deleting
  `#[ignore = "pending E4-1"]` lines), status PR (`status.toml` only).
- **Size budget:** 400 non-generated lines per PR (ES-13). The tests PR may exceed this for test
  code; the PR states its split.

## Data shapes

The caller's view, written before any logic. Names are the tests PR's stubs.

```rust
pub struct SimConfig {
    pub decision_latency: Nanos,
    pub approval_latency: Nanos,
    pub slippage: Slippage,
    pub volume_cap_fraction: Fraction,
}
pub enum Slippage {
    Fixed { half_spread_bps: Bps, impact_bps: Bps },
    Sqrt { half_spread_bps: Bps, coefficient_bps: Bps },
}
pub enum Session { Overnight, PreMarket, Regular, AfterHours, Continuous }
pub struct SimBar {
    pub start: UtcNanos,
    pub open: Price, pub high: Price, pub low: Price, pub close: Price,
    pub volume: Qty,
    pub session: Session,
    pub session_start: UtcNanos,
    pub auction: bool,
}
pub struct SimOrder {
    pub id: OrderRef,
    pub side: Side,
    pub qty: Qty,
    pub kind: OrderKind,
    pub tif: TimeInForce,
    pub extended_hours: bool,
    pub eligible_from: Eligibility,
}
pub enum OrderKind {
    Market,
    Limit { limit: Price },
    Stop { stop: Price },
    StopLimit { stop: Price, limit: Price },
    Oco { take_profit: Price, stop: Price },
}
pub enum Eligibility { DecidedAt { at: UtcNanos, approval_required: bool }, RestingBefore }
pub struct SimFill {
    pub order: OrderRef, pub leg: Option<OcoLeg>, pub bar: usize,
    pub qty: Qty, pub price: Price, pub liquidity: Option<Liquidity>,
}
pub struct Instrument { pub asset_class: AssetClass, pub increment: QtyIncrement }

pub fn simulate(
    config: &SimConfig,
    instrument: &Instrument,
    bars: &[SimBar],
    coverage_start: UtcNanos,
    first_bar_volumes: &dyn FirstBarVolumes,
    orders: &[SimOrder],
) -> Result<SimOutcome, SimError>;
```

`SimOutcome` lists fills in bar order, then submission order, plus each order's end state
(`Filled`, `Canceled { at_bar }` for an expired day order or a canceled OCO leg, `Open`).
`FirstBarVolumes` supplies the §6.4.3 median for a session's first bar: the E4-2 runner computes
it from the dataset, and the harness reads `first_bar_reference_volume`. The sim itself reads no
clock, does no I/O, and keeps no state between calls.

## Interpretations (recorded as DEC-106)

1. **Crate and criticality.** `mandate-sim` is safety-critical. A fill model that flatters results
   misleads the owner at go-live (DEC-97), so the crate gets the lint header, the mutation gate,
   and the DEC-77 sequence.
2. **Fill-price scale.** Prices hold at most 9 places (§2.1), and fills are not tick-rounded. A fill
   price with more places rounds adversely at 9 places: buys up (ceiling), sells down (floor). No
   RC-10, RC-12, or RC-19 price needs rounding: the longest, 98.47045, has 5 places.
3. **`sqrt` impact.** `impact_bps = coefficient_bps × √(fill qty ÷ reference volume)`. The
   coefficient is in basis points, like `impact_bps` in the `fixed` model. The square root is taken
   at 18 places, rounded up, so the impact is never understated. The fill quantity is decided by
   the cap and the remaining quantity before the price is computed, so the rule is not circular.
4. **Reference volume.**
   - The previous bar in the same session is the most recent earlier bar of that session on that
     trading day in the input. After a no-trade minute (§4.2), that is the last bar that traded.
   - A bar with no earlier bar in its session is the session's first bar only if the session
     started at or after `coverage_start`. Its reference volume is then the §6.4.3 median from
     `FirstBarVolumes`. The runner counts a prior session with no bar at that minute as volume 0,
     and with fewer than 20 prior sessions in the dataset the median is unavailable.
   - A bar whose session started before `coverage_start` has no known previous bar. Its reference
     is unavailable, so its cap is 0.
   - This is why no fixture fills on bar 0 of RC-10, RC-12, or RC-19 unless the case supplies
     `first_bar_reference_volume`.
5. **Auction bars.**
   - `SimBar::auction` is an input. It is true for the first regular-session bar of a trading day
     whose session start is covered by the data, and for the first bar after a halt reopens when a
     trading-status dataset is supplied (§6.4.5). E2-4 and the runner compute it; the harness sets
     it for a bar starting at 09:30 ET.
   - A resting limit that the auction open gaps through fills at the open, with no slippage (RC-12)
     and `liquidity = None`. An auction execution is neither maker nor taker, equity fees do not
     depend on liquidity, and crypto has no auction.
6. **Liquidity.** Market fills, stop fills, stop-limit fills at trigger, and marketable-limit fills
   are taker. Resting-limit fills are maker, including a stop-limit resting as a limit (§6.4.7).
7. **Stop-limit that rests.** When the trigger bar opens beyond L, the order rests as a limit at L
   from that same bar, as rule 5 rests a limit "from the first eligible bar". The order triggered at
   the open, so any later print through L in that bar came after the trigger.
8. **OCO with a volume cap.** The first fill of either leg, partial or full, cancels the other leg,
   as the broker does. The filled leg's remainder continues under its own rule on later bars.
9. **Sessions.** An order trades in the regular session, plus the extended sessions when
   `extended_hours` is set (limit orders only, §5.2). Equity stops and stop-limits trigger only on
   regular-session bars (§6.4.6). Crypto bars are `Continuous`. A day order's remainder is canceled
   after the last bar of its last eligible session.
10. **Harness (DEC-85 style).** `resting_since_bar: n` makes an order eligible from bar n as already
    resting (never marketable on arrival). `resting_since: previous_trading_day` does the same from
    bar 0. `isolation: per_order` runs each order alone against a fresh cap. Liquidity is compared
    only where the case states it. Everything else stays "not interpreted until <story>".

## Not done

- GTC expiry after 90 days (§5.2) and broker-initiated replacements: the E4-2 runner owns order
  lifetimes across a run.
- Fees: fills carry liquidity and feed `mandate-accounting`, which computes fees from the pinned
  configuration (E3-1); E4-2 wires the two together.
- Halts from a trading-status dataset: the input flag supports them, but no status dataset exists
  yet (E2-4).
- Journal events: none. The sim is a pure function with no state; E4-2 records the run
  (`BacktestRunRecorded`, journal spec §12).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-sim
cargo test -p mandate-refcases -- --include-ignored trading_domain::RC-1
cargo mutants -p mandate-sim
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong (the ones found so far are DEC-106);
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] RC-10, RC-12, and RC-19 pass, and no case that passed before now fails.
- [ ] Tests came first; each invariant above has a property test whose oracle is independent and
      was shown to fail on a planted bug.
- [ ] New state changes emit journal events (none: the sim is pure).
- [ ] Docs updated: the feature map gains a "Backtest fill model" entry; the tracker's M3 row.
- [ ] `cargo xtask check` is green (summary in each PR).
- [ ] Each PR description is complete (see the PR template).
