# Task: E4-2 Baseline backtest and metrics report

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. This story closes the M3 loop that E4-1 opened: bars in, a signal, an order through the
§6.4 fill model, fills through the accounting fold, and one report that an owner can read.

## Story

- **Story:** E4-2 ([backlog](../06-backlog-v1.md#e4-simulated-execution-and-backtest))
- **Acceptance criteria (verbatim):** "As a researcher, I want a baseline strategy and a metrics
  report (return, volatility, Sharpe, maximum drawdown, turnover, fees, buy-and-hold comparison) so
  that the loop is proven end to end." *Accepted when:* "identical inputs produce identical outputs."
- **PRD / HLD / spec anchors:** PRD FR-4.1 (a backtest with fees and slippage), FR-4.2 (the report's
  seven figures), FR-4.5 (reproducible from the recorded data snapshot); trading domain spec §2.1
  (decimals, one rounding per formula, limit prices on the tick), §2.2 (trade dates and calendars),
  §4.1 to §4.3 (bars and sessions), §5.1 and §5.3 (the v1 order policy the baseline obeys: limit
  opens in the regular session, one working order per instrument, no crossing zero), §6.1 to §6.3
  (fills and fees), §6.4 (the fill model this loop drives), §8.1 to §8.4 (positions, the backtest
  mark is the bar close, cash, settlement), §8.6 (the fold's invariants), §12 (`BacktestRunRecorded`,
  which this story does not yet append); HLD "Agent lifecycle" (Draft → Backtest → Paper) and §6.A
  step 4 (a backtest is required before an agent trades live); journal spec §12 (the
  `BacktestRunRecorded` payload the report becomes).
- **Decisions that apply:** DEC-29 and DEC-37 (v1 order policy), DEC-30 (nothing trades overnight),
  DEC-32 (no short sales in v1), DEC-72 (ADR-0001: ES-02, ES-04, ES-09, ES-13, ES-15, ES-21, ES-23),
  DEC-77 (tests PR then implementation PR), DEC-79, DEC-80 (no plain comments), DEC-83 (tests PRs
  hold stubs only), DEC-85 (an uninterpreted input fails loudly), DEC-89 and DEC-116 item 3 (exact
  decimals only: the quotient of two exact decimals is not one, so a report gives the parts and one
  stated rounding, never a hidden mean), DEC-97 (a backtest is the evidence an owner reads before
  go-live; no calibration in v1), DEC-104 and DEC-105 (account type and buying power), DEC-106 and
  DEC-108 (the fill model this loop calls), DEC-110 (every pending test must fail on the stubs),
  DEC-112 (this brief's PR is documentation only), and DEC-127 (this story's interpretations, below).

## Scope

- **Reference cases that must move from pending to passing:** none. The three backtest cases
  (RC-10, RC-12, RC-19) belong to E4-1, and `docs/specs/reference-cases/trading-domain.yaml` has no
  metrics case, so `crates/mandate-refcases/status.toml` does not change and this story ships as a
  tests PR and an implementation PR with **no status PR** (the DEC-105 precedent).
- **Fixture check before any code.** The metric block below is recomputed from the definitions in
  "Metric definitions", by hand and in an independent script, on the equity series
  E₀ = 100000, E₁ = 101000, E₂ = 99000, E₃ = 103000 (n = 3 periods, P = 252, risk-free 0):

  | Field | Value | Where it comes from |
  |---|---|---|
  | `total_return` | `0.03` | round(3000 ÷ 100000, 12, half_even) |
  | period returns | `0.01`, `-0.019801980198`, `0.040404040404` | round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12, half_even) |
  | `return_sum` | `0.030602060206` | Σ rₖ, exact |
  | `return_sum_of_squares` | `0.00212460490073004860242` | Σ rₖ², exact |
  | `mean_return` | `0.010200686735` | round(0.030602060206 ÷ 3, 12, half_even) |
  | `variance` | `0.000906221436` | round((3 × Σr² − (Σr)²) ÷ (3 × 2), 12, half_even); exact value 0.000906221435556416174137333… |
  | `volatility` | `0.030103512022` | least 12-place v with v² ≥ variance: v² = 0.000906221436058698528484, (v − 10⁻¹²)² = 0.000906221435998491504441 |
  | `variance_annualized` | `0.228367801872` | variance × 252, exact |
  | `volatility_annualized` | `0.477878438384` | ceiling root of the annualized variance, not the root times √252 |
  | `sharpe_squared` | `0.11482183684` | round(0.010200686735² ÷ 0.000906221436, 12, half_even) |
  | `sharpe_sign` | `positive` | the sign of the excess mean |
  | `sharpe` | `0.338853710087` | the 12-place root of `sharpe_squared` rounded towards −∞ |
  | `sharpe_squared_annualized` | `28.93510288368` | `sharpe_squared` × 252, exact |
  | `sharpe_annualized` | `5.379135886337` | the same rooting rule on the annualized value |
  | `max_drawdown` | `0.019801980199` | round((101000 − 99000) ÷ 101000, 12, **ceiling**), at period 2 from the period-1 peak |
  | `max_drawdown_usd` | `2000` | 101000 − 99000, exact |
  | `turnover` (with buys 50000, sells 50600) | `0.5` | round(min(50000, 50600) ÷ 100000, 12, half_even) |

  Period 2 shows the directional rule at work: the drawdown rounds **up** to `0.019801980199`
  while the period return rounds half-even to `-0.019801980198`. A drawdown never understates a
  loss, and the two are different formulas, so they need not agree in the twelfth place.

- **Invariants touched** (each gets a named test in the tests PR, with an oracle that computes the
  answer its own way):

  | Clause or invariant | Test |
  |---|---|
  | FR-4.2 the report carries every required figure, and a figure that is undefined is absent with a reason, never zero | `hand::a_report_carries_every_figure_the_prd_names`, `hand::one_period_leaves_the_dispersion_fields_absent`, `hand::a_flat_run_leaves_the_sharpe_absent_with_zero_variance` |
  | §8.2 the backtest mark is the bar close | `hand::the_period_mark_is_the_last_bars_close`, `properties::every_period_marks_at_its_last_bars_close` |
  | §8.2 equity identity: settled + Σ unsettled + receivables − payables − accrued fees + Σ market value | `properties::every_equity_observation_matches_the_independent_ledger` |
  | The per-bar order is fixed: due settlements and dividends, fills, mark, fee charges, observation, decision (DEC-127 item 2) | `hand::a_fill_in_the_last_bar_of_a_day_is_inside_that_days_equity`, `hand::a_settlement_posts_before_the_bars_fills`, `hand::a_days_fees_are_charged_once_after_the_day_ends` |
  | §6.4 rule 1, no look-ahead: nothing fills in the bar whose close produced the decision, which is timed at the next bar's start | `properties::no_order_fills_in_the_bar_that_decided_it`, `hand::a_signal_at_a_days_close_fills_no_earlier_than_the_next_bar`, `hand::a_decision_is_timed_at_the_next_bars_start_so_a_gap_delays_it` |
  | §2.2 the bar's `trade_date` agrees with the calendar's trade date for the fill instant | `hand::a_bar_whose_trade_date_disagrees_with_the_calendar_fails_the_run` |
  | §5.1, §5.3 rules 2, 3, 5, 6 the baseline opens with tick-rounded limit orders in the regular session, holds at most one working order, closes the whole position, and never crosses zero | `hand::an_entry_is_a_day_limit_buy_at_the_collar_above_the_close`, `hand::an_exit_sells_the_whole_position`, `hand::a_crypto_exit_sells_the_part_the_asset_fee_left_off_the_increment`, `properties::at_most_one_order_ever_works`, `properties::the_strategy_never_crosses_zero_and_never_goes_short` |
  | §2.1 a buy limit rounds down to the tick and a sell limit up | `num::a_limit_price_sits_on_the_reg_nms_tick_against_the_order`, `hand::a_collar_below_one_dollar_uses_the_finer_tick` |
  | DEC-127 item 12 the signal is a division-free comparison, and a tie is flat | `hand::the_crossover_compares_sums_by_cross_multiplication`, `hand::equal_averages_leave_the_strategy_flat`, `properties::the_signal_matches_the_rational_comparison_oracle` |
  | DEC-127 item 12 the signal reads only closed periods at or before its decision | `properties::the_signal_never_reads_a_later_close` |
  | FR-4.2 return: the total return and every period return are one rounding of the exact quotient | `num::a_return_is_one_rounding_of_the_exact_quotient`, `properties::returns_match_the_integer_oracle` |
  | Fees are inside the return (§8.2: accrued fees reduce equity) | `properties::a_higher_fee_never_raises_the_return`, `hand::the_total_return_is_net_of_accrued_and_charged_fees` |
  | Variance is the sample variance with divisor n − 1, one rounding, and never negative | `num::a_sample_variance_matches_the_integer_oracle_and_is_never_negative`, `hand::variance_uses_the_sample_divisor` |
  | Volatility never understates dispersion: v² ≥ variance > (v − 10⁻¹²)² | `num::a_volatility_root_is_the_least_twelve_place_value_whose_square_reaches_the_variance`, `properties::volatility_brackets_the_variance` |
  | Annualization is an exact integer multiplication of the squared figures | `num::an_annualized_variance_is_the_period_value_times_the_period_count`, `hand::annualizing_scales_the_squares_not_the_roots` |
  | Sharpe never reads better than the truth: the reported `sharpe` squared never exceeds `sharpe_squared`, and the root is rounded towards −∞ | `num::a_sharpe_root_never_exceeds_the_squared_value`, `hand::a_negative_sharpe_rounds_away_from_zero`, `properties::the_sharpe_sign_matches_the_excess_mean` |
  | Maximum drawdown is peak-to-trough on the period closes, from the running peak including E₀, and never negative | `hand::a_drawdown_after_a_new_peak_is_measured_from_that_peak`, `hand::a_run_that_only_falls_measures_from_the_starting_equity`, `properties::max_drawdown_matches_the_running_peak_oracle` |
  | Turnover counts one side, not both | `hand::a_single_round_trip_turns_over_its_notional_once`, `properties::turnover_matches_the_min_of_the_two_sides` |
  | The buy-and-hold benchmark buys at its first eligible bar, through the same fill model and fees | `hand::the_benchmark_buys_at_its_first_eligible_bar_not_the_last`, `hand::the_benchmark_that_cannot_fill_reports_its_unfilled_quantity`, `properties::the_benchmark_never_trades_after_its_first_order` |
  | The excess total return is the exact difference of the two reported returns | `hand::the_excess_return_is_the_difference_of_the_two_reported_returns` |
  | Every derived figure recomputes from the figures the report shows (DEC-127 item 15) | `properties::every_reported_statistic_recomputes_from_the_reported_inputs` |
  | FR-4.5, ES-21 identical inputs give byte-identical reports and the same digest | `properties::identical_inputs_give_byte_identical_reports`, `hand::the_report_serializes_to_the_committed_canonical_bytes`, `hand::a_changed_bar_changes_the_bars_digest` |
  | ES-21 no clock, no randomness, no floats, ordered containers only | `properties::a_run_repeated_in_the_same_process_is_equal`, and the crate's lint header |
  | §6.4 the loop drives the fill model without re-deriving it: the run's fills equal `simulate`'s | `properties::the_runs_fills_are_exactly_what_simulate_returned` |
  | ES-04 the new arithmetic is exact or an error, one rounding per formula | `num::ratios_round_once_and_reject_thirteen_places`, `num::shares_at_a_price_truncate_to_the_increment` |

  **Oracles.** `crates/mandate-backtest/tests/properties.rs` holds a second ledger and a second
  statistics implementation, neither sharing code with the crate: the ledger keeps cash, position,
  basis, and fees as `i128` integers (money at 10⁻¹², quantities at 10⁻⁹, cents for charges), as
  E3-1's oracle does, and the statistics use exact integer arithmetic on
  `i128` and 256-bit pairs with floor division, so every rounding is recomputed rather than reused. The
  signal oracle compares `F ÷ fast` with `S ÷ slow` as a pair of cross-multiplied integers. The
  drawdown oracle keeps its own running peak. Every property first compares the number of fills and
  the number of equity observations with the oracle's, so no property can pass on an empty run.

- **Planted bugs.** Each is planted, one at a time, in a throwaway implementation of the stubs (kept
  out of the PR, DEC-83); the tests PR reports which test caught each.

  | Planted bug | Test expected to fail |
  |---|---|
  | Fees dropped from the return (equity as settled + market value) | `properties::a_higher_fee_never_raises_the_return`, `hand::the_total_return_is_net_of_accrued_and_charged_fees` |
  | Drawdown measured from E₀ instead of the running peak | `hand::a_drawdown_after_a_new_peak_is_measured_from_that_peak`, `properties::max_drawdown_matches_the_running_peak_oracle` |
  | Turnover counting both sides | `hand::a_single_round_trip_turns_over_its_notional_once`, `properties::turnover_matches_the_min_of_the_two_sides` |
  | Buy-and-hold buying at the last bar's price | `hand::the_benchmark_buys_at_its_first_eligible_bar_not_the_last` |
  | A figure summed from an unordered map, so the bytes depend on iteration order | `properties::identical_inputs_give_byte_identical_reports`, `hand::the_report_serializes_to_the_committed_canonical_bytes` |
  | Variance with divisor n instead of n − 1 | `hand::variance_uses_the_sample_divisor`, `num::a_sample_variance_matches_the_integer_oracle_and_is_never_negative` |
  | The volatility root truncated instead of rounded up | `num::a_volatility_root_is_the_least_twelve_place_value_whose_square_reaches_the_variance`, `properties::volatility_brackets_the_variance` |
  | Annualization multiplying the Sharpe instead of the squared Sharpe | `hand::annualizing_scales_the_squares_not_the_roots` |
  | The mark taken from the bar open | `hand::the_period_mark_is_the_last_bars_close` |
  | The equity observation taken before the bar's fills | `hand::a_fill_in_the_last_bar_of_a_day_is_inside_that_days_equity` |
  | A day's fee charge applied twice, or before the day ends | `hand::a_days_fees_are_charged_once_after_the_day_ends` |
  | The signal decided on bar i and filled in bar i (look-ahead) | `properties::no_order_fills_in_the_bar_that_decided_it`, `hand::a_signal_at_a_days_close_fills_no_earlier_than_the_next_bar` |
  | A second order submitted while one still works | `properties::at_most_one_order_ever_works` |
  | The exit order sized by the truncated increment instead of the whole position | `hand::an_exit_sells_the_whole_position`, `hand::a_crypto_exit_sells_the_part_the_asset_fee_left_off_the_increment` |

- **Crates in scope:** a new `mandate-backtest` (the recommendation under "Decisions needed": layer 7,
  `pure = true`, `safety_critical = true`, `allowed_external = ["thiserror"]`, a CODEOWNERS line, and
  the `xtask/layers.toml` entry the founder owns). It depends on `mandate-num`, `mandate-time`,
  `mandate-canon`, `mandate-accounting`, and `mandate-sim`. `mandate-num` gains the metric arithmetic
  of DEC-127 item 14 under a shared-crate claim, because ES-04 keeps exact arithmetic inside that
  crate.
- **Crates out of scope:** `mandate-sim` (called, never changed: E4-1 owns it), `mandate-accounting`
  (called, never changed), `mandate-marketdata` (the dataset adapter is a later story, "Not done"),
  `mandate-cli` (no command yet: it sits at layer 7 too, so a `backtest` command waits for the story
  that moves it to layer 8), `mandate-refcases` (no case moves).
- **New dependencies allowed:** none. `thiserror` and `proptest` are registered; the
  `docs/dependencies.md` "Used by" cells gain `mandate-backtest`.
- **Safety-critical:** yes (DEC-127 item 1). A report that flatters a run misleads the owner who
  reads it before going live, exactly as a flattering fill model would (DEC-97, DEC-106 item 1).
  DEC-77 sequence: tests PR (crate, API stubs, pending tests), implementation PR (test files change
  only by deleting `#[ignore = "pending E4-2"]` lines). No status PR.
- **Size budget:** 400 non-generated lines per PR (ES-13). The tests PR exceeds it for test code and
  states its split; the implementation PR is split into the loop and the metrics if it does not fit.

## Data shapes

The caller's view, written before any logic; these are the tests PR's stubs. Names of types the
brief does not spell out come from `mandate-sim` (`SimBar`, `SimConfig`, `SimFill`, `Instrument`,
`FirstBarVolumes`) and `mandate-accounting` (`Config`, `AccountType`, `Execution`, `InstrumentId`).

```rust
pub struct StrategyConfig {
    pub fast_periods: u32,
    pub slow_periods: u32,
    pub collar: Bps,
    pub target_notional: Usd,
}

pub enum Strategy {
    MovingAverageCrossover(StrategyConfig),
    BuyAndHold { collar: Bps },
}

pub struct MetricsConfig {
    pub periods_per_year: u32,
    pub risk_free_per_period: Ratio,
}

pub struct RunConfig {
    pub sim: SimConfig,
    pub fees: mandate_accounting::Config,
    pub account_type: AccountType,
    pub instrument_id: InstrumentId,
    pub instrument: Instrument,
    pub tick: TickRule,
    pub starting_cash: Usd,
    pub strategy: Strategy,
    pub metrics: MetricsConfig,
}

pub struct BacktestInput<'a> {
    pub config: &'a RunConfig,
    pub bars: &'a [SimBar],
    pub coverage_start: UtcNanos,
    pub first_bar_volumes: &'a dyn FirstBarVolumes,
}

pub struct Observation {
    pub period: u32,
    pub date: Date,
    pub bar: usize,
    pub equity: Usd,
}

pub struct RunFill {
    pub fill: SimFill,
    pub bar: usize,
    pub execution: Execution,
}

pub struct SubmittedOrder {
    pub decided_at_bar: usize,
    pub order: SimOrder,
    pub limit: Price,
    pub end: OrderEnd,
}

pub struct Metrics {
    pub period_count: u32,
    pub first_date: Date,
    pub last_date: Date,
    pub starting_equity: Usd,
    pub ending_equity: Usd,
    pub net_pnl: Usd,
    pub total_return: Ratio,
    pub return_sum: Ratio,
    pub return_sum_of_squares: Ratio,
    pub mean_return: Option<Ratio>,
    pub variance: Option<Ratio>,
    pub volatility: Option<Ratio>,
    pub periods_per_year: u32,
    pub variance_annualized: Option<Ratio>,
    pub volatility_annualized: Option<Ratio>,
    pub risk_free_per_period: Ratio,
    pub sharpe_squared: Option<Ratio>,
    pub sharpe_sign: Sign,
    pub sharpe: Option<Ratio>,
    pub sharpe_squared_annualized: Option<Ratio>,
    pub sharpe_annualized: Option<Ratio>,
    pub max_drawdown: Ratio,
    pub max_drawdown_usd: Usd,
    pub max_drawdown_peak_period: u32,
    pub max_drawdown_trough_period: u32,
    pub buy_notional: Usd,
    pub sell_notional: Usd,
    pub traded_notional: Usd,
    pub fill_count: u32,
    pub turnover: Ratio,
    pub fees_total: Usd,
    pub fees_charged: Usd,
    pub fees_accrued: Usd,
    pub fees_asset: Usd,
    pub submitted_qty: Qty,
    pub filled_qty: Qty,
}

pub enum Sign {
    Negative,
    Zero,
    Positive,
}

pub struct InputDigests {
    pub bars: Digest,
    pub config: Digest,
}

pub struct Report {
    pub report_version: u32,
    pub inputs: InputDigests,
    pub strategy: Metrics,
    pub benchmark: Metrics,
    pub excess_total_return: Ratio,
}

impl Report {
    pub fn canonical(&self) -> Value;
    pub fn digest(&self) -> Digest;
}

pub struct BacktestRun {
    pub report: Report,
    pub equity: Vec<Observation>,
    pub fills: Vec<RunFill>,
    pub orders: Vec<SubmittedOrder>,
}

pub fn run(input: &BacktestInput<'_>) -> Result<BacktestRun, BacktestError>;
```

`BacktestError` names one cause each, with a stable code (ES-09): `NoBars`,
`BarTradeDateMismatch(usize)` (a bar whose `trade_date` is not the calendar's trade date for its
start), `EquityNotPositive(u32)` (a period whose opening equity is not positive, so no return is
defined), `StrategyWindowsCrossed` (`fast_periods` at or above `slow_periods`, or either zero),
`PeriodsPerYearNotPositive`, `Unimplemented` (the tests PR's stubs only; the implementation PR
removes the variant), and the wrapped `Sim`, `Accounting`, `Num`, and `Time` errors.

`Ratio` and `TickRule` are the `mandate-num` additions of DEC-127 item 14; `Digest` and `Value` are
`mandate-canon`'s.

## The loop

`run` walks `bars` once, in index order, and does exactly this at bar *i*:

1. **Due cash movements.** `account.due(bars[i].start)` (settlements at 00:00 ET, and dividends if a
   later story feeds any), applied in the order the fold returns them.
2. **Fills.** Every fill the working order produced at bar *i*, in the order `simulate` returned
   them, each folded as `Input::Fill(Execution)`.
3. **Mark.** `Input::Mark { instrument, price: bars[i].close }` (§8.2: a backtest mark is the bar
   close).
4. **Fee charges.** For each fee family, `Input::FeesCharged { family, day }` for every accrual day
   that ended before this bar's start (DEC-127 item 17), in day then family order; at the last bar,
   every remaining accrual is charged, so the report's charged and accrued split is complete.
5. **Observation.** If bar *i* is the last bar of its `trade_date`, record `Observation` with the
   account's `equity()`.
6. **Decision.** If bar *i* closes a period and no order is working, evaluate the strategy on the
   closing prices of the periods up to and including this one and, if it wants to act, submit one
   order decided at `bars[i + 1].start` and simulate it (below). A decision on the last bar is
   therefore never acted on, and never recorded.

An order is simulated by one `simulate` call over the bars from *i* + 1 to the end of the order's
life — the last bar of its last eligible session for a day order, the last bar of the input for a
GTC order — with `Eligibility::DecidedAt { at: bars[i + 1].start, approval_required: false }`, the
run's `SimConfig`, the run's `coverage_start`, and the run's `FirstBarVolumes`. The window's fill bar
indices are translated to absolute indices before use.

`bars[i + 1].start` is the decision instant because a `SimBar` carries no duration, so the earliest
instant the input *proves* is at or after bar *i*'s close is the start of the next bar (bar starts
are strictly increasing, §6.4's bar checks). With zero latency the order is then eligible from bar
*i* + 1, never from bar *i*, which is the no-look-ahead rule: the bar whose close produced the
decision cannot fill it (§6.4 rule 1, whose "a bar starting exactly at `decided_at` is eligible"
makes the choice of instant part of the contract). Latency moves eligibility further on, never back.
One `simulate` call per order is enough because the baseline keeps at most one order working (§5.3
rule 6, DEC-127 item 18).

Every instant the loop passes to the fold or to the model is a bar start, the only instant a
`SimBar` carries; nothing is keyed on an inferred bar end.

Nothing in the loop reads a clock, a file, or a random number. The only state is the account, the
working order, and the observation list.

## The baseline strategy

A moving-average crossover on period closes, chosen because it is the simplest deterministic rule
that (a) needs **no division at all** — two window sums compared by cross-multiplication — (b) takes
two integers as its whole parameter set, and (c) produces round trips, so turnover, drawdown, and the
buy-and-hold comparison are not trivially zero. A fixed-weight periodic rebalance would need a
division per rebalance (weight × equity ÷ price) and would couple the signal to the mark, which adds
roundings without adding coverage.

With period closes c₁ … c_d and windows `fast_periods` = f < `slow_periods` = s:

- The signal is defined only from period s onward; before that the strategy does nothing.
- F = Σ of the last f closes, S = Σ of the last s closes, both exact sums of 9-place prices.
- **Long** when F × s > S × f (that is, F ÷ f > S ÷ s); **flat** when F × s ≤ S × f. A tie is flat:
  ambiguity resolves to the side that does not add risk (AGENTS.md rule 3).
- **Entry** (long signal, no position, no working order): a day limit **buy** in the regular session
  at `limit = on_tick(close.slipped(collar, up), tick, down)`, quantity
  `truncate(min(target_notional, settled cash) ÷ limit, increment)`; nothing is submitted when that
  quantity is zero. `extended_hours` is false.
- **Exit** (flat signal, position held, no working order): a limit **sell** at
  `limit = on_tick(close.slipped(collar, down), tick, up)` for the **whole** position, which §5.3
  rule 2 exempts from the increment. That exemption is not decoration: a crypto buy's fee is taken in
  the asset (§8.3), so the received quantity sits off the increment, and an exit that truncated to the
  increment would leave dust behind for ever. A day order that does not fill is re-submitted at the
  next period's close while the signal stays flat.
- **Time in force** follows §5.2: `Day` on an equity, `Gtc` on a crypto instrument, whose bars are
  continuous and which `simulate` rejects a day order on (`DayOrderOnAContinuousInstrument`). A GTC
  order therefore works until it fills or the bars run out, and is never re-submitted.
- No adds while long, no second working order, no short sales (DEC-32), no protective legs, no
  notional or market orders. Every order the baseline can build is inside the v1 policy of §5.1 and
  §5.2, which `simulate` re-checks and rejects otherwise.

The **buy-and-hold benchmark** is the same loop, bars, fill model, fee configuration, and starting
cash, with a strategy that submits exactly one order: at the close of the first bar, a **GTC** limit
buy at `on_tick(close.slipped(collar, up), tick, down)` for
`truncate(starting cash ÷ limit, increment)` shares, and nothing afterwards. Its remainder keeps
working, so a benchmark the volume cap or a rising market cannot fill reports its `submitted_qty`
above its `filled_qty` instead of pretending to hold shares. Both runs pay the same slippage and the
same fees, so the comparison is between strategies and not between models.

## Metric definitions

Every figure is an exact decimal or one named rounding of one formula (§2.1). Nothing is a float;
nothing is an irrational number rendered as a decimal without a stated direction.

Let E₀ be the equity before the first bar (the starting cash, no position) and E₁ … Eₙ the period
observations. A period is a `trade_date` (crypto: the UTC date, whose last bar is the one ending
00:00 UTC, §8.2). Every quotient takes a positive denominator; an Eₖ₋₁ that is not positive is
`EquityNotPositive`, never a substituted zero.

| Figure | Definition |
|---|---|
| `net_pnl` | Eₙ − E₀, exact |
| `total_return` | round((Eₙ − E₀) ÷ E₀, 12, half_even) |
| period return rₖ | round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12, half_even), k = 1 … n |
| `return_sum`, `return_sum_of_squares` | Σ rₖ and Σ rₖ², exact (the report shows them so every statistic below recomputes) |
| `mean_return` | round(`return_sum` ÷ n, 12, half_even) |
| `variance` | round((n × `return_sum_of_squares` − `return_sum`²) ÷ (n × (n − 1)), 12, half_even), n ≥ 2; absent below that |
| `volatility` | the least 12-place v ≥ 0 with v² ≥ `variance` (a ceiling root: never understates dispersion) |
| `variance_annualized` | `variance` × `periods_per_year`, exact |
| `volatility_annualized` | the ceiling root of `variance_annualized` |
| `sharpe_squared` | round((`mean_return` − `risk_free_per_period`)² ÷ `variance`, 12, half_even), `variance` > 0; absent otherwise |
| `sharpe_sign` | the sign of `mean_return` − `risk_free_per_period`, as `negative`, `zero`, or `positive` |
| `sharpe` | the 12-place root of `sharpe_squared` rounded **towards −∞**: the floor root with a non-negative sign, the negated ceiling root with a negative one |
| `sharpe_squared_annualized`, `sharpe_annualized` | `sharpe_squared` × `periods_per_year`, exact, and the same rooting rule |
| `max_drawdown` | max over k = 0 … n of round((Pₖ − Eₖ) ÷ Pₖ, 12, **ceiling**) with Pₖ = max(E₀ … Eₖ); ≥ 0 |
| `max_drawdown_usd`, `..._peak_period`, `..._trough_period` | Pⱼ − Eⱼ exact at the **earliest** j attaining the maximum, and the earliest period attaining Pⱼ |
| `buy_notional`, `sell_notional` | Σ over fills of the fill's quantity (the gross quantity `simulate` reports) × its price, by side, exact |
| `traded_notional`, `fill_count` | their sum, exact, and the number of fills |
| `turnover` | round(min(`buy_notional`, `sell_notional`) ÷ E₀, 12, half_even) |
| `fees_total`, `fees_charged`, `fees_accrued`, `fees_asset` | the fold's, exact; the return is already net of them (§8.2 subtracts accrued fees from equity) |
| `excess_total_return` | the strategy's `total_return` − the benchmark's, exact |

`Ratio` holds at most 24 fractional digits, so `return_sum_of_squares` is exact; every rounding in the
table above is at 12 places, and a sum that does not fit is an `overflow` error rather than a
silently shortened number.

Three rules hold across the table:

1. **Annualization scales the squares.** Variance and squared Sharpe multiply by an integer exactly;
   their roots are then taken once. Multiplying a root by √252 would need an irrational factor, so
   the report never does it.
2. **No annualized return, and no statistic without its parts.** A compound annual return needs a fractional
   power, which exact decimals cannot take; the report gives the total return, the period count, the
   first and last dates, and `periods_per_year`, which is what a reader needs to annualize under a
   stated convention (the DEC-116 item 3 rule: give the parts, not a hidden quotient).
3. **Every rounded root moves against the run.** Volatility up, Sharpe towards −∞, drawdown up. A
   root cannot be exact at any scale, so its direction is a choice, and the choice is the one that
   never makes a backtest look better than it was (DEC-106's doctrine).

## Determinism and the record

- The report serializes to a `mandate_canon::Value` object: decimals as canonical text (the form
  `Display` produces), counts and versions as `Int`, dates as `YYYY-MM-DD` strings, absent figures
  as `Null`. `to_canonical` gives the bytes and `Digest::of` the run digest. Keys are sorted because
  `Object` is a `BTreeMap`; every collection in the crate is ordered (ES-21).
- `inputs.bars` is the digest of the canonical form of the bar rows, and `inputs.config` the digest
  of the canonical form of `RunConfig`, so a report states which snapshot and configuration produced
  it (FR-4.5). `report_version` is 1 and rises whenever a definition above changes, as
  `fold_version` does for the fold.
- The acceptance criterion is the test `properties::identical_inputs_give_byte_identical_reports`
  plus a committed golden report in `crates/mandate-backtest/tests/` whose bytes and digest a hand
  test pins.

## Interpretations (recorded as DEC-127)

1. **Crate and criticality.** The loop, the strategy, and the metrics go in a new safety-critical
   `mandate-backtest` at layer 7 (`pure = true`, `allowed_external = ["thiserror"]`), not inside
   `mandate-sim`: the report is owner-facing evidence (DEC-97), and a separate crate keeps E4-1's
   model unedited while stream A holds it. `xtask/layers.toml` and CODEOWNERS are founder-owned, so
   the entry is under "Decisions needed" and the tests PR uses whichever placement the coordinator
   confirms; nothing else in the brief depends on the choice.
2. **The per-bar order** is due cash movements, fills, the bar-close mark, fee charges, the period
   observation, then the decision, as "The loop" states. A settlement dated before the bar therefore
   lands before its fills, and a fill in a period's last bar is inside that period's equity. Every
   instant the loop passes on is a bar start, because a `SimBar` carries no duration; a decision
   taken on bar *i*'s close is timed at `bars[i + 1].start`, the earliest instant the input proves is
   at or after that close, so with zero latency the order is eligible from bar *i* + 1 and never from
   the bar that decided it, and a decision on the last bar is never acted on.
3. **Periods.** One period per `trade_date`, observed at the last bar of that date; E₀ is the equity
   before the first bar. Crypto periods are UTC dates (§8.2). `periods_per_year` is configuration,
   252 for equity trade dates and 365 for crypto days, and the report states the value it used.
4. **Returns.** rₖ = round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12, half_even) and the total return the same way at
   12 places, half-even, matching §2.1's convention for ratio formulas. An opening equity that is
   not positive is the error `equity_not_positive`, not a return of −1 or 0.
5. **Variance** is the sample variance, divisor n − 1, computed as one rounding of
   (n × Σr² − (Σr)²) ÷ (n(n − 1)) on 256-bit intermediates, and absent when n < 2. The sample
   divisor is what a reader comparing our Sharpe with another's assumes, and the report shows n and
   both sums, so a reader recomputes the variance with the stated divisor.
6. **Volatility** is the 12-place ceiling root of the variance, and the annualized volatility the
   ceiling root of the annualized variance. Reporting the variance as well means annualization stays
   exact and a reader can check the root by squaring it.
7. **Sharpe** is reported as `sharpe_squared` with `sharpe_sign`, which annualizes by an exact
   integer multiplication, plus a `sharpe` rounded towards −∞ so it never reads better than the
   truth. A zero variance leaves both absent with the reason `zero_variance`; an excess mean of zero
   gives sign `zero` and a Sharpe of 0.
8. **No annualized (CAGR) return**, for the reason in "Metric definitions" rule 2.
9. **Maximum drawdown** is measured on the period closes only, from the running peak including E₀,
   each rung rounded up at 12 places, reported with the USD amount and the earliest peak and trough
   periods attaining it. Intra-bar lows are not used: the report says what a holder saw at each
   period close, and a low-based drawdown is a different statistic that would need bar lows for the
   whole universe.
10. **Turnover** is round(min(buy notional, sell notional) ÷ E₀, 12, half_even), with both sides, the
    total, and the fill count reported exactly. One side, because a round trip turns a position over
    once; E₀ as the denominator, because an average-equity denominator would need a second rounding
    and the owner's starting capital is the figure the envelope fixes (DEC-97).
11. **Fees** come from the fold (`fees_total`, `fees_charged`, `fees_accrued`, `fees_asset`) and are
    never added into the return separately: §8.2's equity already subtracts accrued fees, so a fee
    can neither be double counted nor dropped.
12. **The baseline strategy** is the division-free moving-average crossover of "The baseline
    strategy": long on a strict cross above, flat on a tie or below, one working order at a time
    (§5.3 rule 6), long only (DEC-32), entries as tick-rounded day limit buys in the regular session
    and exits as tick-rounded limit sells of the whole position (which the crypto asset fee leaves off
    the increment, §5.3 rule 2), no adds and no protective legs; the time in force is `Day` on an
    equity and `Gtc` on a continuous instrument (§5.2), and a GTC order is never re-submitted.
13. **The benchmark** is the same loop with a one-order GTC buy-and-hold strategy, sized
    `truncate(starting cash ÷ limit, increment)`, reporting `submitted_qty` and `filled_qty`, and the
    comparison is a full metric block plus the exact difference of the two total returns. The
    benchmark pays the same slippage and fees as the strategy; it is a tradable benchmark, not an
    index line.
14. **`mandate-num` additions** (ES-04 keeps exact arithmetic in that crate, so the metrics crate
    divides nothing itself), under a shared-crate claim: `Ratio` (signed, at most 24 places so an
    exact sum of squares fits, with every reported rounding at 12; ES-04 already names the type),
    `Usd::ratio_to`, `Usd::shares_at` (truncate(cash ÷ price, increment)),
    `Ratio::{checked_add, checked_sub, negated, times_int, squared_quotient, mean, sample_variance,
    root_floor, root_ceiling}`, and `Price::on_tick(TickRule, Adverse)` with §2.1's Reg NMS table
    (0.01 at or above 1.00 USD, 0.0001 below) and a broker `Increment` for crypto. Each has an
    integer oracle in `crates/mandate-num/tests/num.rs`.
15. **The report is self-checking.** Every derived figure is computed from figures the report itself
    shows — the sums, n, the mean, the variance — with one stated rounding, so a reader recomputes
    the whole block from the report and no hidden higher-precision intermediate can change an
    answer. A property test recomputes every figure this way.
16. **The metrics have no spec section today.** The definitions live in this brief and in DEC-127,
    and are *proposed* for a section of the trading domain spec in the pre-M5 spec window, because
    they are owner-facing performance evidence, and how performance is presented to an owner is the
    founder's call (DEC-79, and the compliance document's performance rules). This PR edits no spec.
17. **Fee charge timing.** Each family's accrual for a day is charged once, at the first bar whose
    start is at or after New York midnight following that day, and any remainder at the last bar of
    the run. Charging changes the total return only by §6.2's rounding up to the cent, which moves
    against the run.
18. **One `simulate` call per order** over the bars of that order's life, because the baseline keeps
    at most one order working. Multi-order cap sharing stays E4-1's tested behaviour; a runner with
    several instruments and several working orders is a later story.
19. **The account** is the broker's type (DEC-105): a margin account, because every Alpaca account
    is one (OD-08), with no reservations and no buying-power check — the risk gate is not in this
    loop (see "Not done"), so a fee can leave settled cash slightly negative, which §8.3 allows for
    margin accounts.
20. **A bar's `trade_date` is checked, not trusted.** For an equity the loop recomputes the trade
    date of each bar's start with `TradingCalendar::equity_trade_date`; for crypto, which has no trade
    date (§2.2), it compares the bar's `trade_date` with the UTC date of its start. Either
    disagreement fails the run, so a mislabelled bar cannot silently move a period boundary (the
    DEC-108 item 6 doctrine).

## Not done

- **No mandate, no risk gate, no autonomy policy** in the loop: the baseline submits straight to the
  fill model. A backtest of a *mandate* (FR-4.1's full sense) waits for the M5 gate stories, which
  will run the same loop with the gate between the strategy and the fill model.
- **No journal append.** `BacktestRunRecorded` (journal spec §12) needs a journal client, an artifact
  store, and an ID generator; the report's canonical bytes and digest are exactly what that event's
  payload and artifact will carry, and the story that wires the runner appends it.
- **No dataset adapter.** Bars arrive as `SimBar`s. A stored Parquet bar carries up to 18 fractional
  digits (DEC-89) while `Price` holds 9, and session labels, auction flags, and the 20-session
  medians come from E2-4's calendar work, so the dataset-to-bar conversion needs its own rule and its
  own story.
- **No corporate actions or dividends in the loop.** The fold accepts them and the loop applies
  whatever `account.due` returns, but nothing feeds `Input::CorporateAction` yet; FR-4.1's corporate
  actions land with the adapter story. FR-4.1's fees and slippage are proven here.
- **No perpetual funding**, no multi-instrument portfolio, no multi-order volume-cap sharing, no
  protective exits, no paper-versus-backtest comparison, no CLI command, and no parameter search or
  calibration (DEC-97: no calibration in v1).
- **No annualized return, no beta, no alpha, no per-trade statistics** beyond the fill counts and
  notionals FR-4.2 names.
- **Performance.** The tracker's note that the fold copies the account on every input is measured on
  the longest fixture and reported in the implementation PR; optimizing it is a separate change.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-backtest
cargo nextest run -p mandate-num
cargo xtask ci pending
cargo mutants -p mandate-backtest
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The metrics report is produced for the baseline and the benchmark, and identical inputs give
      byte-identical reports and digests.
- [ ] Tests came first; each invariant above has a test whose oracle computes the answer its own way
      and was shown to fail on a planted bug.
- [ ] New state changes emit journal events (none here: the run is a pure function, and the story
      that appends `BacktestRunRecorded` owns the event).
- [ ] Docs updated: this brief, DEC-127, the feature map's "Baseline backtest and metrics" entry, and
      the tracker's M3, Stories, Claims, and work-graph rows.
- [ ] `cargo xtask check` is green (summary in each PR).
- [ ] Each PR description is complete (see the PR template).
