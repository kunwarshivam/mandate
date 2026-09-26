# Task: E3-2 Splits, dividends, and cash in lieu

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story.

## Story

- **Story:** E3-2 ([backlog](../06-backlog-v1.md#e3-accounting))
- **Acceptance criteria (verbatim):** "As a trader, I want splits and dividends applied to
  positions and cash so that stock P&L is correct." The backlog gives no separate acceptance line;
  the reference cases below and the invariants they exercise are the acceptance test.
- **PRD / HLD / spec anchors:** trading domain spec §2.1 (adjusted marks, dividends), §8.2 (equity
  and total P&L with receivables and income), §8.3 (dividend and cash-in-lieu cash movements),
  §8.5 (corporate actions), §8.6 (I1, I3, I5, I6), §12 (`CorporateActionApplied`, `DividendPaid`,
  `CashInLieuPosted`); HLD "Event-sourced state".
- **Decisions that apply:** DEC-72 (ADR-0001), DEC-77 (tests PR, then implementation, then
  status), DEC-79, DEC-80 (no plain comments), DEC-82 (the 12-place mark type belongs here),
  DEC-83 (mutants skip crates with pending markers), DEC-84 to DEC-87 (E3-1 fold), DEC-91 to
  DEC-96 (recorded by this story; DEC-94 is proposed and awaits the founder).

## Scope

- **Reference cases that must move from pending to passing:** `trading_domain::RC-05`,
  `trading_domain::RC-06`, `trading_domain::RC-06::short_position_generic_broker`,
  `trading_domain::RC-23`, `trading_domain::RC-23::forward_3_for_1_non_terminating_mark`.
  `trading_domain::RC-01`, `RC-02`, `RC-03`, and `RC-13` keep passing.
- **Stay pending, with their owners:** `RC-04` (its split, mark, and equity steps are reproduced by
  `corporate_actions::rc_04_forward_split_multiplies_quantity_and_divides_the_mark`; it waits on
  `broker_position_update` and `reconciliation` steps and the `reconciliation` expectation, E7-3;
  `corporate_action_prepare`, `actions`, and initial `open_orders`, E7-4; `orders`, E7-2;
  `agent_mode`, E6-9). `RC-06::protective_orders_kept_through_dividend` (its accounting is RC-06's;
  it waits on `corporate_action_prepare`, `actions`, and `open_orders`, E7-4, and `orders`, E7-2).
- **Invariants touched** (each a named test whose oracle is independent and was shown to fail on a
  planted bug):

  | Clause | Test |
  |---|---|
  | §2.1 adjusted marks: 12 places, one rounding | `num::mark_prices_are_positive_with_at_most_twelve_places`, `num::adjusted_marks_are_one_rounding_of_mark_times_old_over_new`, `num::a_price_is_the_same_mark_and_values_at_a_mark_are_exact` |
  | §8.5 Q' is Q × new ÷ old truncated to the increment | `num::a_split_truncates_q_times_new_over_old_toward_zero_to_the_increment`, `num::split_ratios_are_positive_integers`, `num::split_results_that_do_not_fit_are_errors` |
  | §8.5 residual basis and cash in lieu, one rounding each | `num::the_residual_basis_is_one_rounding_of_the_exact_fraction`, `num::cash_in_lieu_is_one_rounding_of_the_residual_times_the_price` |
  | Every reported value after every event | `corporate_action_properties::every_reported_value_matches_the_oracle_after_every_event` |
  | I1 conservation, with income | `corporate_action_properties::i1_conservation_with_income_holds_for_every_event` |
  | I3 splits (bound as corrected by DEC-94, proposed) | `corporate_action_properties::i3_a_split_moves_market_value_only_by_the_mark_rounding`, `corporate_actions::i3_is_bounded_by_the_raw_quantity_not_the_split_quantity` |
  | §8.5 residual removal never exceeds the basis held (DEC-86, DEC-92) | `corporate_action_properties::a_split_removes_the_residuals_basis_and_never_more_than_the_basis_held` |
  | §8.5 every stored mark is adjusted | `corporate_action_properties::every_stored_mark_is_replaced_by_the_adjusted_mark` |
  | I5 quantity, with split truncations | `corporate_action_properties::i5_quantity_is_the_fold_of_fills_and_split_truncations` |
  | §8.5 dividend entitlement: fills traded before the ex-date | `corporate_action_properties::a_dividend_is_entitled_on_the_position_after_fills_traded_before_the_ex_date` |
  | §8.3 receivables become settled cash exactly once | `corporate_action_properties::receivables_become_settled_cash_exactly_once` |
  | I6 determinism | `corporate_action_properties::i6_folding_round_tripped_inputs_is_identical` |

  **Oracles.** `num.rs` holds values as `i128` integers at a fixed scale and rounds by floor
  division, as for E3-1. `corporate_action_properties.rs` keeps an `i128` ledger: quantities at
  10⁻⁹, basis at 10⁻¹⁸, marks at 10⁻¹², money at 10⁻²¹. It computes trade dates with a fixed −4 h
  offset and a naive weekday and holiday count, and has its own split, dividend, receivable,
  settlement, and ordering logic. The generated scenario mixes fills (whole, fractional, and
  one-nanoshare dust), marks, up to three splits (forward, reverse, 1:2 on whole shares, with and
  without a cash-in-lieu price), dividends, clock advances, cash-in-lieu postings, redelivered
  actions, late fills, and stale actions. Values are biased toward rounding ties (odd-cent prices,
  per-share amounts in multiples of 0.005). After every input the property compares the result
  (applied or the expected error), checks that a rejection changes nothing, and compares every
  reported value; a drain phase then pays every dividend and posts every cash in lieu.
  Hand-calculated tests in `corporate_actions.rs` show the arithmetic in each doc comment.

  **Hand-calculated tests** (`corporate_actions.rs`): one per reference case turned green
  (`rc_05_…`, `rc_06_long_…`, `rc_06_short_…`, `rc_23_fractionable_…`, `rc_23_forward_…`), RC-04's
  accounting (`rc_04_…`), and one per rule the cases do not reach: a short reverse split owing
  cash in lieu, a split leaving no share, dust residuals, half-even cash in lieu and dividends,
  flat holders, adjusted last fill prices and flat marks, a mark that rounds to zero, ordering by
  ex-date, postings with nothing outstanding, earliest-match cash in lieu, `due` order on one date,
  and no dividend paid before its ex-date.

  **Planted bugs**, each caught (tests that failed in brackets): the §8.5 formula used when no
  share remains [residual property, oracle]; cash in lieu rounded half-up [residual property,
  oracle]; last fill price not adjusted [mark property, oracle]; dividend rounded half-up
  [entitlement property, oracle]; dividends due before settlements on one date [oracle]; equity
  without receivables [I1, oracle]; adjusted mark rounded by ceiling [I3, mark property]; split
  quantity rounded instead of truncated [I5]; a round trip that drops the cash-in-lieu price [I6];
  a payment that keeps its receivable [receivables property]; entitlement on the absolute quantity
  [entitlement property]; an action admitted on a fill's trade date [oracle]; a posting that
  settles the latest match [earliest-match hand test]. In `mandate-num`: residual basis, cash in
  lieu, and adjusted mark each rounded twice [their one-rounding properties]; marks accepting 13
  places [mark parse property]; value at a mark rounded to 12 places [value property]; split
  rounding instead of truncating [split property].

  **Mutants.** `MANDATE_BASE_REF=<tests PR head> cargo xtask ci mutants` on the implementation
  diff: 67 mutants, 37 caught, 30 unviable, 0 missed.

- **Crates in scope:** `mandate-num` (`MarkPrice`, `SplitRatio`, `ShareIncrement`, `SplitQty`),
  `mandate-accounting` (corporate actions, receivables, income), `mandate-refcases` (the
  `trading_domain` harness). All are safety-critical.
- **Crates out of scope:** `mandate-journal` (payload schemas come with the executor story that
  journals these events), `mandate-time` (no change), `mandate-domain`.
- **New dependencies allowed:** none.
- **Safety-critical:** yes. Delivered as a stack (DEC-77): this tests PR (API stubs, pending tests,
  harness, brief, decisions), then the implementation, whose test-file changes are only marker
  deletions, then the status change. Spec wording for DEC-92 to DEC-94 goes in a separate
  spec-only change.
- **Size budget:** 400 non-generated lines per safety-critical PR (ES-13). The tests PR exceeds it
  because it holds the story's full test suite and the harness, reviewed as a unit, as for E3-1.

## Journal events that feed the fold (spec §12)

| Journal event | `mandate_accounting::Input` |
|---|---|
| `CorporateActionApplied` | `CorporateAction(Split \| CashDividend)` |
| `DividendPaid` (00:00 New York on the pay date, emitted by `advance_clock` in `Account::due` order) | `DividendPaid { instrument, ex_date }` |
| `CashInLieuPosted` | `CashInLieuPosted { instrument, amount }` |
| `CorporateActionPrepared` | none: it cancels orders (E7-4) and has no accounting effect |

The fold's records (`Split`, `CashDividend`, `DividendPaid`, `CashInLieuPosted`) carry the amounts
for the executor to journal.

## Interpretations (DEC-91 to DEC-96)

1. **Types (DEC-91).** Marks are `MarkPrice` (positive, at most 12 places); a split's quantities
   exist only as a `SplitQty` built by `SplitRatio::split`; a `CashDividend` cannot pay before its
   ex-date.
2. **No share left (DEC-92).** When Q' = 0 the whole basis is removed. Otherwise the §8.5 formula
   applies and provably never removes more than the basis held.
3. **Cash in lieu (DEC-93).** round(f × p, 2, half_even), signed like Q; a posting settles the
   earliest outstanding cash in lieu of exactly that amount, else `cash_in_lieu_mismatch`.
4. **I3 (DEC-94, proposed; founder to confirm).** The bound is \|Q_raw\| × 5 × 10⁻¹³, not
   \|Q'\| × 5 × 10⁻¹³. It loosens the approved I3 tolerance, which DEC-79 leaves to the founder, so
   it is not accepted until the founder confirms it. The fold's arithmetic is unchanged; only the
   bound the I3 tests check moves.
5. **Order and dividends (DEC-95).** Duplicate, then out of order (ex-date on or before a fill's
   trade date, or before an applied ex-date), then fills before an applied ex-date are rejected;
   dividends round half-even on the position at application; `due` lists settlements before
   dividends on one date; every stored mark is adjusted; I1 includes income and is exact for
   splits; tax lots are out of scope.
6. **Harness (DEC-96).** The conservation baseline is the first state at which every compared
   value is defined; `corporate_action_prepare` belongs to E7-4.

## Not done here (with the story that owns each)

| Story | What | Cases waiting |
|---|---|---|
| E7-2 | Order state | RC-04, RC-06 `protective_orders_kept_through_dividend` |
| E7-3 | Reconciliation, `pending_corporate_action`, broker position updates; broker amounts that differ from the fold's dividends or cash in lieu | RC-04 |
| E7-4 | `corporate_action_prepare` (order cancels, confirmation), protection re-derivation, open orders | RC-04, RC-06 `protective_orders_kept_through_dividend` |
| E6-9 | Agent mode | RC-04 |

Also not here: tax lots (§8.5); other corporate actions (stock dividends, spin-offs, mergers,
symbol changes, delistings: out of scope for v1); journaling the fold's records (the executor
story); the paper-mode shadow ledger for dividends (§10).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-num -p mandate-accounting
cargo test -p mandate-accounting --test corporate_action_properties -- --include-ignored
cargo test -p mandate-refcases --test refcases -- --include-ignored trading_domain
MANDATE_BASE_REF=<tests PR head> cargo xtask ci mutants
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
