# Task: E3-1 Positions, cash, fees, and P&L

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story.

## Story

- **Story:** E3-1 ([backlog](../06-backlog-v1.md#e3-accounting))
- **Acceptance criteria (verbatim):** "property-based tests and hand-calculated cases pass,
  including partial fills and position flips."
- **PRD / HLD / spec anchors:** trading domain spec §2.1 (decimals and rounding), §2.2 (time and
  calendars), §6.1 to §6.3 (fills and fees), §8.1 to §8.4 (positions, marks, cash, settlement),
  §8.6 (invariants), §12 (journal events); HLD "Event-sourced state".
- **Decisions that apply:** DEC-72 (ADR-0001: ES-02, ES-04, ES-05, ES-09, ES-11, ES-13, ES-15),
  DEC-77 (two-PR mechanics), DEC-79 (agents merge after CI and an independent review), DEC-80 (no
  plain comments), DEC-82 to DEC-87 (recorded by this story).

## Scope

- **Reference cases that must move from pending to passing:** `trading_domain::schema_version`,
  `trading_domain::RC-01`, `trading_domain::RC-02`, `trading_domain::RC-03`,
  `trading_domain::RC-13`. The accounting parts of RC-07 (crypto fees in the received asset) and
  RC-11 (trade and settlement dates) are reproduced by hand-calculated tests; the cases stay pending
  on keys owned by later stories (below).
- **Invariants touched** (each a named test whose oracle is independent and was shown to fail on a
  planted bug):

  | Clause | Test |
  |---|---|
  | §2.1 decimals exact or an error, never rounded on input | `num::canonical_text_round_trips_and_nothing_else_parses`, `num::quantities_and_prices_never_round_on_the_way_in`, `num::results_that_do_not_fit_are_errors` |
  | §2.1 each formula rounds once, with its own mode | `num::rounding_matches_the_floor_based_oracle`, `num::crypto_fee_quantity_is_rounded_once`, `num::usd_fee_in_basis_points_is_rounded_once`, `num::basis_reduction_is_one_rounding_of_the_exact_proportion` |
  | §2.2 trade date (20:00 ET cutoff) and T+1 settlement on the calendar | `calendar::equity_trade_date_is_the_trading_day_of_execution_with_the_evening_cutoff`, `calendar::settlement_is_the_next_settlement_day_after_the_trade_date`, `calendar::new_york_date_and_hour_follow_the_daylight_saving_rule` |
  | Every reported value after every event | `properties::every_reported_value_matches_the_oracle_after_every_event` |
  | I1 conservation | `properties::i1_conservation_holds_for_every_event` |
  | I2 reducing fills | `properties::i2_reducing_fills_remove_one_rounding_of_the_proportional_basis` |
  | I4 cash (total = settled + Σ unsettled; no bucket ≤ D after `SettlementPosted` D) | `properties::i4_cash_totals_and_settlement_leave_no_due_bucket` |
  | I5 quantity | `properties::i5_quantity_is_the_fold_of_signed_received_quantities` |
  | I6 determinism | `properties::i6_folding_is_deterministic_across_a_text_round_trip` |
  | §6.3 asset fees are never accrued; §6.2 charges round up | `properties::asset_fees_are_never_accrued_and_charges_round_up_by_less_than_a_cent` |
  | A fill is applied once (§8.6 I7, fold side) | `properties::duplicate_fills_are_never_applied_twice` |

  I3 (splits) belongs to E3-2 and I7 (orders) to E7-2.

  **Oracles.** `num.rs` holds values as `i128` integers at a fixed scale and rounds by floor
  division. `calendar.rs` computes New York offsets from the written DST rule (second Sunday of
  March, first Sunday of November) and counts days naively. `properties.rs` keeps an `i128` ledger
  (money at 10⁻¹², quantities at 10⁻⁹, cents) with its own position, fee, cap, bucket, and
  settlement logic, over a generated scenario of fills (partial, reducing, closing, crossing), marks,
  charges, clock advances, settlements, exact-half reductions, and duplicate fills, starting from a
  held position whose basis has a 12th-place tail. Hand-calculated tests in `hand.rs` show the
  arithmetic in each doc comment.

  **Planted bugs**, each caught (test that failed in brackets): basis portion rounds the ratio before
  multiplying [basis reduction property, hand values]; half-even ties go up [crypto fee property];
  ceiling ignores sign [rounding oracle]; parse accepts trailing zeros [canonical round trip]; trade
  date cutoff at 21:00 [trade date property]; settlement ignores bank holidays [settlement
  property]; New York midnight taken as UTC [midnight property]; equity charge rounds half-even
  [charge property, oracle]; crypto asset fee accrued [asset fee property, oracle]; settlement moves
  only the bucket dated exactly D [I4, oracle]; TAF cap ignored [oracle]; per-order TAF room not
  tracked [oracle]; reduction rounds half-up [I2, oracle]; a fill price replaces a later mark
  [oracle]; a duplicate fill is applied [duplicate property, oracle]; crypto fee accrued on the New
  York date [oracle]; the charge debits the unrounded accrual [I1, oracle]; realized uses the
  unrounded basis [I1, oracle]; a crypto buy adds the gross quantity [I5, I1, I2, oracle]; `Display`
  drops digits beyond 3 places [I6 and others].

  **Mutants.** `cargo xtask ci mutants` on the implementation diff: 174 mutants, 101 caught, 73
  unviable, 0 missed.

- **Crates in scope:** `mandate-num` (new, layer 0), `mandate-time` (calendars, layer 0),
  `mandate-accounting` (new, layer 2), `mandate-refcases` (the `trading_domain` suite). All are
  safety-critical.
- **Crates out of scope:** `mandate-domain` (not needed yet, DEC-82), `mandate-journal` (payload
  schemas for these events come with the executor story that journals them), `mandate-sim`.
- **New dependencies allowed:** those ADR-0001 names: `rust_decimal` 1.43 (ES-04, no default
  features), `ruint` (ES-04, no default features), `jiff` 0.2 (ES-05, `std` and
  `tzdb-bundle-always`, only in `mandate-time`). Each has a registry row. New transitive crates:
  `jiff-core`, `jiff-tzdb`, `arrayvec`, `num-traits`, `ruint-macro`.
- **Safety-critical:** yes. Delivered as a stack (DEC-77): tests then implementation for
  `mandate-num` and `mandate-time`; tests then implementation for `mandate-accounting` and the
  `trading_domain` harness; then the status change.
- **Size budget:** 400 non-generated lines per safety-critical PR (ES-13). The tests PRs exceed it
  because each holds one crate pair's full test suite, which is reviewed as a unit; the split
  follows E5-1.

## Review round 1 (DEC-86, DEC-87)

The independent review merged the tests PRs and the `mandate-num`/`mandate-time` implementation and
failed the accounting implementation. What changed, and why:

1. **Cost basis follows the quantity's sign (DEC-86).** `Position::new` rejected only a flat position
   with a basis, so a long with a negative basis could be built and closing it at 100 reported 200
   realized. The constructor now rejects a basis whose sign opposes the quantity; initial positions
   and the harness's fixture loading go through it, and each fold transition builds its result
   through it. A reduction's removed basis is limited to the basis held, which matters only when the
   basis has digits below the 12th place.
2. **Fee configuration is non-negative by type (DEC-87).** `EquityFees::taf_cap` was a signed
   `Usd`: a negative cap made per-execution TAF negative while per-order mode clamped it to 0. It is
   now `FeeCap` (parsed at the boundary; negative text is rejected), and both modes use
   min(uncapped, cap − TAF already charged on the order).
3. **`Halve` in the property generator** is resolved from the oracle's position at that point in
   the run, not from the opening state, so it halves what is held after earlier events.

New tests (pending in the tests PR, live in the implementation PR):

| Finding | Test |
|---|---|
| Basis sign follows quantity | `hand::positions_whose_basis_opposes_the_quantity_are_rejected` |
| Closing P&L stays correct | `hand::closing_at_the_average_cost_realizes_nothing`, `hand::a_reduction_never_removes_more_basis_than_the_position_holds`, `properties::reductions_keep_the_basis_on_the_position_side_and_a_close_realizes_cash_flow` |
| Negative cap rejected | `num::fee_caps_are_non_negative_amounts_of_money` |
| Cap modes agree | `hand::both_taf_cap_modes_charge_one_execution_orders_alike` |

`crates/mandate-refcases/status.toml` is unchanged: the status PR still marks the same cases as
passing.

## Journal events that feed the fold (spec §12)

| Journal event | `mandate_accounting::Input` |
|---|---|
| `FillApplied`, `LateFillApplied` | `Fill(Execution)` |
| `MarkUpdated` | `Mark { instrument, price }` |
| `FeesCharged` | `FeesCharged { family, day }` |
| `SettlementPosted` | `SettlementPosted { date }` |

`DividendPaid`, `CorporateActionApplied`, and `CashInLieuPosted` join with E3-2. The fold returns a
`Record` per input (trade and settlement dates, signed received quantity, realized P&L, fees,
charged amounts, settled amounts) for the executor to journal; writing those events is the executor
story's work.

## Interpretations (DEC-84, DEC-85)

1. The reporting mark is the latest `MarkUpdated`, else the last fill price; a later fill never
   replaces a mark. With neither, unrealized P&L and equity are an error.
2. Net realized = gross realized − fees (accrued, charged, and asset fees at their USD value).
3. Equity trade date: New York date, next day from 20:00 ET, then the first trading day on or after;
   settlement is the first settlement day after it. `SettlementPosted` for D moves every bucket
   dated on or before D.
4. Crypto USD fees accrue on the UTC date and are charged as accrued (already whole cents); the
   equities charge is `round(daily total, 2, ceiling)`.
5. A duplicate `fill_id` is an error and changes nothing; a crossing fill is split into close and
   open at the same price.
6. Harness: `conservation` keys are changes since the case start, and the I1 identity is checked;
   `avg_cost` is compared as `round(B ÷ Q, 12, half_even)`; a step's fill ID is `step_<n>`.

## Not done here (with the story that owns each)

The harness fails these with "not interpreted until <story>":

| Story | What | Cases waiting |
|---|---|---|
| E3-2 | Corporate actions, dividends, income, receivables, broker cash postings | RC-04, RC-05, RC-06 (and variants), RC-23 (and variant) |
| E3-3 | Cash-account settlement rules | RC-08, RC-18 `generic_cash_account` |
| E4-1 | Backtests (`bars`, `orders`, `isolation`) | RC-10, RC-12, RC-19 |
| E6-3 | Gate decisions (`propose_order`, `decision`) | RC-03 `gate_rejects_zero_crossing_order`, RC-08, RC-09, RC-09B, RC-14 to RC-18, RC-20 to RC-22, RC-24, RC-25 |
| E6-5 | Kill switch | RC-14 `kill_switch` |
| E6-6 | Buying power, day trades, account regime | RC-08, RC-09, RC-09B, RC-17, RC-18 |
| E6-7 | Eligibility inputs (prior close, dollar volume, leveraged ETPs, IPOs) | RC-16, RC-24, RC-25 |
| E6-8 | Conduct breaches | RC-22 |
| E6-9 | Account status and agent mode | RC-04, RC-11, RC-14 `kill_switch`, RC-15 (and variants), RC-22 |
| E7-2 | Broker order updates and order state | RC-06 `protective_orders_kept_through_dividend`, RC-14, RC-15, RC-18, RC-20 to RC-22, RC-24 |
| E7-3 | Reconciliation | RC-04, RC-07 |
| E7-4 | Order actions, protective quantities, open orders, quotes | RC-04, RC-06, RC-14, RC-15, RC-20 to RC-22, RC-24 |
| E7-5 | Agents and external fills | RC-11, RC-16, RC-17 |

Also not here: 12-place adjusted marks (a `mandate-num` type for E3-2, DEC-82); the no-debit rule
and buying power (E6-6); journaling the fold's records (the executor story).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-num -p mandate-time -p mandate-accounting
cargo test -p mandate-refcases --test refcases -- --include-ignored trading_domain
cargo xtask ci mutants
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [x] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (the fold returns records; the executor story journals
      them).
- [x] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
