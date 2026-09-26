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
- **Decisions that apply:** DEC-29 and DEC-37 (v1 order policy), DEC-30 (no orders in the
  overnight session), DEC-35 (data profiles), DEC-72
  (ADR-0001), DEC-77 (tests PR, implementation PR, status PR), DEC-79, DEC-80 (no plain comments),
  DEC-83 (tests PRs hold stubs only), DEC-85 (harness interpretations fail loudly until owned),
  DEC-97 (backtests are the evidence an owner sees before go-live), DEC-106 (the fill model's
  interpretations, recorded by this story), and DEC-108 (the tests PR's crate, arithmetic, and harness
  shapes, recorded in the tests PR).

## Scope

- **Reference cases that must move from pending to passing:** `trading_domain::RC-10`,
  `trading_domain::RC-12`, `trading_domain::RC-19`. They are pending today on the case-level keys
  `bars`, `orders`, and `isolation`, which `crates/mandate-refcases/src/trading_domain.rs` already
  assigns to E4-1. Every other case keeps its state.
- **Fixture check before any code:** each of the 15 expected fills in the three cases (RC-10: 5, RC-12: 7, RC-19: 3) was
  recomputed by hand from §6.4 with `test_default` (s = half-spread 1 bps + impact 2 bps = 3 bps,
  volume cap 10% of the previous bar, zero latency). All 15 match. For example, RC-10
  `volume_cap`: bar 1 caps at 10% × 5000 = 500 at min(100.60, 100.00 × 1.0003) = 100.03; bar 2
  caps at 800 and fills the remaining 500 at min(100.60, 100.40 × 1.0003) = 100.43012. RC-12
  `stop_gap_through`: open 98.50 ≤ 99.00, so the fill is 98.50 × 0.9997 = 98.47045.
- **Invariants touched** (each gets a named property test in the tests PR, with an oracle that
  computes the answer its own way):

  | Clause | Test |
  |---|---|
  | §6.4.1 nothing fills before the first bar starting at or after decision + latency (+ approval latency), and never on the bar that produced the decision (a bar starting exactly at `decided_at` is eligible, as RC-10's `market_sell_exit` shows) | `properties::no_fill_before_eligibility`, `hand::latency_moves_eligibility_and_approval_latency_applies_only_when_approval_was_required` |
  | §6.4.2 fills only in sessions the order may trade; the extended sessions take limit orders alone | `properties::fills_respect_sessions`, `hand::a_market_order_waits_for_the_regular_session`, `hand::an_exit_marked_for_extended_hours_fills_after_hours`, `hand::rc_12_an_equity_stop_does_not_trigger_in_extended_hours` |
  | §6.4.2, DEC-30 nothing ever fills overnight | `hand::an_overnight_bar_never_fills` |
  | §6.4.2 a day order has no fill after its last eligible session ends | `properties::day_orders_expire_at_session_end`, `hand::a_day_orders_remainder_is_canceled_after_its_last_eligible_session` |
  | §6.4.3 per bar and instrument, Σ fills over the account's orders ≤ truncate(fraction × reference volume, increment), allocated in submission order; 0 when the reference is unavailable | `properties::fills_never_exceed_the_shared_volume_cap`, `hand::a_bars_volume_cap_is_shared_across_orders_in_submission_order`, `hand::a_sessions_first_bar_caps_on_the_twenty_session_median_and_on_zero_without_one`, `num::a_volume_cap_is_the_truncated_product_and_never_above_it` |
  | Σ fills of an order ≤ its quantity; every fill is positive and a multiple of the increment | `properties::orders_never_overfill` |
  | §6.4.4 a market order fills at the slipped open of its first tradable bar | `hand::rc_10_a_market_sell_fills_at_the_first_eligible_open_less_slippage`, `num::hand_calculated_slippage_and_volume_caps` |
  | §6.4.4 the `sqrt` impact model | `hand::sqrt_impact_takes_the_root_of_the_filled_share_of_reference_volume`, `num::hand_calculated_sqrt_impacts`, `num::sqrt_impact_is_monotone_and_bounded_by_its_coefficient` |
  | §6.4.5 marketable on arrival, and a remainder that becomes resting and stays resting | `hand::rc_10_a_marketable_limit_buy_fills_at_the_slipped_open_inside_its_limit`, `hand::rc_19_a_marketable_remainder_becomes_resting_when_a_bar_opens_beyond_the_limit`, `hand::a_resting_remainder_does_not_become_marketable_again` |
  | §6.4.5 a touch is not a fill (resting limit, extreme equal to the limit) | `properties::a_touch_is_never_a_fill`, `hand::rc_10_a_touch_is_not_a_fill_and_the_next_bar_through_the_limit_is`, `hand::rc_19_a_touch_on_the_arrival_bar_is_not_a_fill`, `hand::rc_19_a_resting_limit_can_fill_on_its_arrival_bar` |
  | §6.4.5 continuous trading never prints through a resting limit; the auction bar is the exception | `hand::rc_12_a_resting_limit_gapped_through_in_continuous_trading_fills_at_the_limit`, `hand::rc_12_a_resting_limit_gapped_through_at_an_auction_fills_at_the_open` |
  | §6.4.5 a buy limit never fills above its limit and a sell limit never below it | `properties::limit_prices_are_never_violated` |
  | Slippage and rounding never flatter a fill: a buy never pays less, and a sell never receives more, than the slippage-free price (DEC-106 item 2) | `properties::slippage_and_rounding_are_adverse`, `hand::a_fill_price_beyond_nine_places_rounds_against_the_order`, `num::slippage_moves_a_price_against_the_order_by_a_rounded_up_amount` |
  | §6.4.6 stops: a gap fills at the open, a reach fills at the stop, equities in the regular session only | `hand::rc_12_a_stop_reached_inside_the_bar_fills_at_the_stop_less_slippage`, `hand::rc_12_a_stop_gapped_through_fills_at_the_open_less_slippage`, `hand::a_buy_stop_reached_by_the_high_fills_at_the_stop_and_is_not_tick_rounded`, `hand::a_crypto_stop_triggers_on_a_continuous_bar` |
  | §6.4.7 a triggered stop-limit fills at max(L, min(open, S) × (1 − s)) for a sell, symmetric for a buy | `hand::a_triggered_stop_limit_fills_at_the_better_of_its_limit_and_the_triggered_price` |
  | §6.4.7 a stop-limit whose trigger bar opens beyond L fills nothing in that bar and rests from the next (DEC-106 item 7) | `hand::a_stop_limit_that_gaps_beyond_its_limit_does_not_fill_in_its_trigger_bar`, `hand::rc_12_a_stop_limit_that_gaps_past_its_limit_rests_and_fills_at_the_limit` |
  | §6.4.8 at most one OCO leg fills; the first fill of either leg cancels the other (DEC-106 item 8) | `properties::oco_fills_at_most_one_leg`, `hand::rc_12_an_oco_with_both_legs_reachable_fills_the_stop_first`, `hand::rc_12_an_oco_whose_open_reaches_the_take_profit_fills_that_leg`, `hand::a_partial_oco_fill_cancels_the_other_leg_and_leaves_a_market_remainder` |
  | §6.4.8 a take-profit leg is never marketable on arrival, and its remainder keeps resting | `hand::an_ocos_take_profit_is_never_marketable_on_arrival` |
  | DEC-106 item 10 a triggered stop's remainder, an OCO's included, is a market order | `hand::a_triggered_stops_remainder_is_a_market_order`, `hand::a_partial_oco_fill_cancels_the_other_leg_and_leaves_a_market_remainder` |
  | §6.4.9 fill prices are not tick-rounded (only the 9-place adverse rounding of DEC-106 item 2 applies) | `hand::a_buy_stop_reached_by_the_high_fills_at_the_stop_and_is_not_tick_rounded`, `hand::rc_10_a_marketable_limit_fills_across_two_bars_under_the_volume_cap` (100.43012) |
  | §5.1, §5.2 an order outside the v1 policy is rejected, never simulated | `hand::the_model_rejects_bars_and_orders_it_cannot_simulate` |
  | Replay: the same inputs give identical fills | `properties::identical_inputs_give_identical_fills` |
  | The whole model, fills, cancellations, and end states, against the oracle | `properties::fills_match_the_independent_simulator` |
  | §2.1, ES-04 the new arithmetic is exact or an error, with one rounding per formula | `num::fractions_run_from_zero_to_one_inclusive`, `num::adding_quantities_and_basis_points_is_exact` |
  | The harness reads every key the backtest cases state (DEC-85, DEC-106 item 11) | `harness::rc_10_and_rc_19_pass_and_a_wrong_fill_or_decision_time_fails`, `harness::rc_12_passes_and_its_session_labels_median_and_canceled_leg_are_read` |

  **Oracle.** `crates/mandate-sim/tests/properties.rs` holds a second fill model, written
  order-major on purpose: each order in submission order consumes what it can from every bar's
  remaining cap before the next order is considered, in `i128` integers at 10⁻⁹ with its own integer
  ceiling arithmetic. Rule 3 gives every order strict priority over later ones on every bar, so the
  two traversals must agree. Every property runs the model through one helper that first compares the
  number of fills with the oracle's, so no property can pass on an empty fill list. Generated
  scenarios use the `fixed` impact model; an independent oracle for the 18-place root would need
  arbitrary-precision integers, so the `sqrt` model is pinned by hand-computed digits instead.

  **Planted bugs.** Thirty, each broken in a throwaway implementation of the stubs (kept out of the
  PR, DEC-83), one at a time, and every one caught. The full list, with the tests that failed for
  each, is the module doc of `crates/mandate-sim/tests/properties.rs`. Writing that implementation
  also found two defects in the tests' own first reading of §6.4, both caught by the reference cases:
  the marketable-on-arrival test compares the open with the limit in the opposite direction from a
  stop's trigger, and an order that is already resting must never take that test at all.

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

The caller's view, written before any logic. These are the tests PR's stubs, in `mandate-sim`.

```rust
pub struct Nanos(u64);                       // from_millis; §6.4 configures latency in milliseconds
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
    pub trade_date: Date,                    // with `session`, names the session instance
    pub session: Session,
    pub session_start: UtcNanos,
    pub auction: bool,
}
pub struct Instrument { pub asset_class: AssetClass, pub increment: ShareIncrement }
pub struct OrderRef(usize);                  // assigned by `simulate` from the submission index
pub struct SimOrder {
    pub side: Side,
    pub qty: Qty,
    pub kind: OrderKind,
    pub tif: TimeInForce,
    pub extended_hours: bool,                // only a limit order may carry it (§5.2)
    pub eligible_from: Eligibility,
}
pub enum OrderKind {
    Market,
    Limit { limit: Price },
    Stop { stop: Price },
    StopLimit { stop: Price, limit: Price },
    Oco { limit: Price, stop: Price },       // the limit leg is the take-profit
}
pub enum OcoLeg { Limit, Stop }
pub enum TimeInForce { Day, Gtc }
pub enum Eligibility {
    DecidedAt { at: UtcNanos, approval_required: bool },
    Resting { from_bar: usize },
}
pub struct SimFill {
    pub order: OrderRef, pub leg: Option<OcoLeg>, pub bar: usize,
    pub qty: Qty, pub price: Price, pub liquidity: Option<Liquidity>,
}
pub struct CanceledLeg { pub order: OrderRef, pub leg: OcoLeg, pub bar: usize }
pub enum OrderEnd { Filled, Expired { at_bar: usize }, Open }
pub struct SimOutcome {
    pub fills: Vec<SimFill>,
    pub canceled_legs: Vec<CanceledLeg>,
    pub ends: Vec<OrderEnd>,                 // indexed by `OrderRef`
}

pub fn simulate(
    config: &SimConfig,
    instrument: &Instrument,
    bars: &[SimBar],
    coverage_start: UtcNanos,
    first_bar_volumes: &dyn FirstBarVolumes,
    orders: &[SimOrder],
) -> Result<SimOutcome, SimError>;
```

An order is identified by its `OrderRef`, its index in `orders`, which is also its submission order.
`SimOutcome` lists fills in bar order, then submission order, each canceled OCO leg with the bar of
the fill that took it, and each order's end: `Filled`, `Expired { at_bar }` (a day order past its
last eligible session), or `Open`. `SimError` names one cause each, with a stable code (ES-09):
`BarsOutOfOrder`, `InconsistentBar`, and `BarBeforeItsSession` for the bars; and, for an order
outside the v1 policy of §5.1 and §5.2, `ZeroQuantity`, `QuantityOffIncrement`,
`ExtendedHoursNeedsALimit`, `StopLimitCrossed`, `OcoLegsCrossed`, `OcoOnAFractionalInstrument`,
`DayOrderOnAContinuousInstrument`, and `RestingBarOutOfRange`, plus the numeric and time errors it
wraps.

The price and cap arithmetic lives in `mandate-num`, whose exact arithmetic is crate-private
(ES-04), as additions only, under shared-crate claim #62: `Fraction` (with `ZERO` and `ONE`),
`Qty::checked_add`, `Qty::portion` (truncate(fraction × qty, increment)),
`Price::slipped(Bps, Adverse)`, `Bps::checked_add`, and `Bps::sqrt_impact`. DEC-106 item 2's adverse
rounding needs no new rounding mode: rounding the slippage **amount** up once at 9 places moves a buy
up and a sell down, and equals ceil(p × (1 + s), 9) and floor(p × (1 − s), 9) exactly, because a price
already sits on the 9-place grid (DEC-108 item 2).

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
7. **Stop-limit that rests.** When the trigger bar opens beyond L, rule 7 says "no fill": nothing
   fills in that bar, even if it later prints through L, and the order rests as a limit at L from
   the next bar. This is the literal reading and the conservative one; no reference case
   distinguishes it from resting in the trigger bar itself.
8. **OCO with a volume cap.** The first fill of either leg, partial or full, cancels the other leg,
   as the broker does. A take-profit remainder keeps resting at its limit; a stop remainder is a
   market order (item 10).
9. **Sessions.** An order trades in the regular session, plus pre-market and after-hours when
   `extended_hours` is set (limit orders only, §5.2). Nothing fills in the overnight session
   (DEC-30). Equity stops and stop-limits trigger only on
   regular-session bars (§6.4.6). Crypto bars are `Continuous`. A day order's remainder is canceled
   after the last bar of its last eligible session.
10. **Lifecycles.** A trigger or a change of phase happens on the bar that causes it, even when
    the cap lets nothing fill there: a stop that triggers becomes a market order; a limit whose
    open is beyond its price starts resting. A triggered stop's remainder, and an OCO's remainder
    after its stop leg fills, is a market order, filled at later regular-session opens with
    slippage. An OCO leg is canceled only by a fill of the other leg.
11. **Harness (DEC-85 style).** `resting_since_bar: n` makes an order eligible from bar n as already
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

## Follow-ups

Minors from the independent review of the tests PR (#75) that this story does not take; that
review's other minors are fixed above. Neither can change a fill, and each widens the generator or
the sequencing rather than the model, which the freeze rule leaves to the backlog.

- **Generator coverage.** Generated scenarios use the `fixed` slippage model on a whole-share US
  equity, with latencies of whole minutes, and mark only the first trading day's 09:30 bar as an
  auction bar. What they therefore never produce is covered by hand tests instead — `Session::Continuous`
  bars and a fractional instrument by `hand::a_crypto_stop_triggers_on_a_continuous_bar`, the `sqrt`
  model by `hand::sqrt_impact_takes_the_root_of_the_filled_share_of_reference_volume` and
  `mandate-num`'s hand-computed roots — or not at all: a second trading day's auction bar, and a
  sub-minute latency, which against minute bars can only move eligibility on by one bar. Widening the
  generator belongs with the implementation PR, where a counter-example can be debugged against real
  logic instead of a stub.
- **Size.** The tests PR is large for one sitting. The `mandate-refcases` interpretations could have
  been sequenced as their own PR after the crate merged; the next story that touches the harness
  splits that way.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-sim
cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/trading_domain::RC-1[029]$/)'
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
- [x] Tests came first; each invariant above has a property test whose oracle is independent and
      was shown to fail on a planted bug.
- [ ] New state changes emit journal events (none: the sim is pure).
- [x] Docs updated: the feature map gains a "Backtest fill model" entry; the tracker's M3 and Claims
      rows.
- [ ] `cargo xtask check` is green (summary in each PR).
- [ ] Each PR description is complete (see the PR template).
