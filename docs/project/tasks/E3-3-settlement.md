# Task: E3-3 Cash-account settlement and buying power

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. This story closes milestone M2.

## Story

- **Story:** E3-3 ([backlog](../06-backlog-v1.md#e3-accounting))
- **Acceptance criteria (verbatim):** "As a trader, I want settlement tracked for cash accounts so
  that the system knows which cash is available to trade." The backlog gives no separate acceptance
  line; the reference cases below, the invariants they exercise, and the hand-calculated cases are
  the acceptance test.
- **PRD / HLD / spec anchors:** trading domain spec §7.2 (account state; model buying power by
  account type), §8.3 (cash buckets and movements; the no-debit rule), §8.4 (settlement: T+1 for
  equities, at fill for crypto), §8.6 (I4), §9.5 (buying power and reservations), §12 (journal
  events); HLD "Event-sourced state".
- **Decisions that apply:** DEC-25 (cash accounts use settled cash only), DEC-34 (margin accounts
  use settled + unsettled; settled-only for cash accounts), DEC-72 (ADR-0001), DEC-77 (tests PR,
  then implementation), DEC-79, DEC-80 (no plain comments), DEC-83 (mutants skip crates with pending
  markers; tests PRs hold stubs only), DEC-84 to DEC-87 (E3-1 fold), DEC-95 and DEC-96 (E3-2 fold
  and harness), DEC-99 and DEC-100 (recorded by this story).

## Scope

- **Reference cases that must move from pending to passing:** none. Every `buying_power`
  expectation in the suite (RC-08, RC-17, RC-18) sits in a case with `propose_order` steps and a
  `decision` expectation, which E6-3 owns (DEC-100). `trading_domain::RC-01`, `RC-02`, `RC-03`,
  `RC-05`, `RC-06` and variant, `RC-13`, `RC-23` and variant keep passing.
- **Stay pending, with their owners:** `RC-08` and `RC-18::generic_cash_account` lose their E3-3
  items and wait on `propose_order` and `decision` (E6-3) and reservations (E6-6). Their accounting
  steps are reproduced by `settlement::rc_08_cash_account_buying_power_is_settled_cash_less_pending_charges`
  and `settlement::rc_18_margin_account_reuses_unsettled_proceeds_and_its_cash_variant_does_not`
  (which also shows the arithmetic behind RC-08's deny and RC-18's allow: the QQQ order needs 600.01
  against 499.98 and 1049.98), and the harness reads them end to end without their gate step in
  `harness::rc_08_…` and `harness::rc_18_…`. `RC-17` and `RC-18` wait on E6-3, E6-6, E7-2, E7-5.
- **Invariants touched** (each a named test whose oracle is independent and was shown to fail on a
  planted bug):

  | Clause | Test |
  |---|---|
  | §7.2 cash account: settled − reservations − rounded accrued fees | `settlement::rc_08_cash_account_buying_power_is_settled_cash_less_pending_charges`, `settlement_properties::buying_power_matches_the_oracle_after_every_event` |
  | §7.2 margin account: settled + Σ unsettled − reservations − rounded accrued fees (DEC-34) | `settlement::rc_18_margin_account_reuses_unsettled_proceeds_and_its_cash_variant_does_not`, `settlement_properties::margin_and_cash_buying_power_differ_by_exactly_the_unsettled_proceeds` |
  | §7.2, §9.5 reservations come off one for one and are never negative | `settlement::reservations_reduce_buying_power_and_are_never_negative`, `settlement_properties::reservations_reduce_buying_power_one_for_one_and_are_never_negative` |
  | §7.2 round(accrued, 2, ceiling) per charge bucket (DEC-99) | `settlement::pending_charges_are_rounded_up_per_family_and_day`, `settlement_properties::charging_every_open_bucket_leaves_exactly_the_buying_power_in_cash` |
  | §8.3 no-debit rule, I4, after every gate-approved fill (DEC-99) | `settlement_properties::i4_the_no_debit_rule_holds_after_every_gate_approved_fill`, `settlement::a_cash_account_sale_can_leave_a_fee_debit_until_its_proceeds_settle` |
  | §8.4 crypto settles at fill | `settlement::crypto_proceeds_are_settled_at_fill_in_a_cash_account` |
  | The fold records debit-creating fills (DEC-99) | `settlement::a_fill_that_creates_a_debit_is_recorded_and_buying_power_reports_the_shortfall` |
  | The account type is explicit, kept, and part of the state (I6) | `settlement::the_account_type_is_set_at_opening_and_kept_through_the_fold`; `account_type` compared after every event in the oracle property |
  | Harness reads `initial.account.type` and checks `buying_power` (DEC-85, DEC-100) | `harness::rc_08_accounting_steps_pass_and_a_wrong_account_type_or_buying_power_fails`, `harness::rc_18_cash_variant_accounting_steps_pass_and_the_margin_case_reads_the_default_type` |

  **Oracles.** `settlement_properties.rs` keeps an `i128` cash ledger (money at 10⁻¹², quantities
  at 10⁻⁹, prices in cents) with its own SEC, TAF (capped), CAT, and crypto fee arithmetic, its
  own trade-day (20:00 cutoff, weekdays) and settlement-day (T+1, weekdays, the 2026-10-12 bank
  holiday) counting, its own per-bucket ceilings, and its own buying power. The generated scenario
  mixes equity and crypto fills, charges of chosen buckets, and clock advances over up to 33 days,
  in a cash or a margin account, with a random reservation total; the same inputs are folded into a
  twin account of the other type. In `gated` scenarios the generator approves each buy with the
  oracle's buying power and caps sells at the position, so the sequence is one the gate would have
  allowed. Hand-calculated tests in `settlement.rs` show the arithmetic in each doc comment. The
  harness test edits the founder's fixture in memory (RC-08 and RC-18 without their `propose_order`
  step) and shows the founder's values pass while a wrong account type, a wrong buying power, a
  missing type on a generic broker, and a cash account on the alpaca profile fail.

  **Planted bugs**, each planted in a local implementation of the stubs (kept out of this PR per
  DEC-83) and caught (tests that failed in brackets; re-run after review round 2 with the
  oracle-only I4 bound and the scenario-typed properties): cash buying power counts unsettled
  proceeds [rc_08, rc_18, fee-debit, and account-type hand tests, oracle property, difference
  property, charging property, both harness tests]; margin buying power ignores unsettled proceeds
  [rc_18 and account-type hand tests, oracle, difference, charging, I4, both harness tests];
  accrued fees rounded once as a total instead of per bucket [per-bucket hand test, oracle,
  charging property]; accrued fees rounded half-even instead of up [rc_08, debit, per-bucket, and
  account-type hand tests, oracle, charging property, harness rc_08]; reservations added instead of
  subtracted [rc_18 hand test, both reservation tests, oracle]; a negative reservation total
  accepted [both reservation tests]; a crypto sell leaves settled cash unchanged [crypto and
  per-bucket hand tests, oracle, I4]; `account_type` always reports margin [account-type and rc_08
  hand tests, oracle]; the opening type dropped and margin stored [rc_08, rc_18, fee-debit, and
  account-type hand tests, oracle, difference, charging, both harness tests]; the equities charge
  rounds up to a tenth of a dollar [rc_08, rc_18, per-bucket, debit, fee-debit, and account-type
  hand tests, oracle, charging, I4, both harness tests]; a charge debits settled cash twice
  [rc_08, rc_18, per-bucket, and fee-debit hand tests, oracle, charging, I4 (its cash bound is
  the oracle's charges, so an over-debiting fold cannot widen it), harness rc_08]; the harness
  reads `type: cash` as margin [both harness tests]; the harness never checks the `buying_power`
  key [both harness tests]. The `i4` property also caught, before it was stated exactly, that the
  spec's cash-account wording is stricter than its fee model allows (DEC-99 item 5), and
  `the_gated_generator_produces_fee_debits_while_proceeds_are_unsettled` (live) shows the gated
  generator reaches that state, so the bound is exercised.

- **Crates in scope:** `mandate-accounting` (`AccountType`, `Reservations`, `Account::opening`
  takes the type, `Account::account_type`, `Account::buying_power`), `mandate-refcases` (the
  `trading_domain` harness and its self-check). Both are safety-critical.
- **Crates out of scope:** `mandate-num`, `mandate-time` (no change), `mandate-journal` (the
  `AccountStateObserved` payload comes with E6-9), `mandate-domain`.
- **New dependencies allowed:** none.
- **Safety-critical:** yes. Delivered per DEC-77: this **tests PR** (API stubs, 15 tests marked
  `#[ignore = "pending E3-3"]`, the harness, this brief, DEC-99 and DEC-100, the feature map), then
  the implementation PR, whose test-file changes are only marker deletions. There is no status PR:
  no case moves to passing (DEC-100). Spec wording for DEC-99 item 5 goes in a separate spec-only
  change.
- **Size budget:** 400 non-generated lines per safety-critical PR (ES-13). The tests PR exceeds it
  because it holds the story's full test suite and the harness, reviewed as a unit, as for E3-1 and
  E3-2.

## Journal events that feed the fold (spec §12)

No new input. The account type is opening state today; when E6-9 journals `AccountStateObserved`,
the executor opens the fold with the observed type. Buying power is a derived value the gate reads
(`GateDecided` records it, E6-3); reservations arrive from the account ledger (E6-6).

## Interpretations (DEC-99, DEC-100)

1. **Account type on the account.** `Account::opening(account_type, settled, positions)`; no
   default; reported by `account_type()`; unchanged within a fold.
2. **Buying power.** `buying_power(reservations)` = settled (+ Σ unsettled for margin) −
   reservations − Σ over open (family, day) buckets of round(bucket, 2, ceiling). The per-bucket
   sum is what the charges will debit; it equals round(Σ, 2, ceiling) whenever one bucket is open
   and is otherwise the more conservative figure. Negative when a debit exists.
3. **Reservations.** A non-negative total (`Reservations::new` rejects a negative amount with
   `negative`); `Reservations::NONE` until E6-6 tracks orders. The broker-side minimum of §7.2 is
   the gate's.
4. **The fold records every fill**, debit-creating ones included; the no-debit rule is the gate's
   promise (I4 "after every order the gate approved").
5. **What the no-debit rule can promise.** In a cash account the charge on a sale is debited before
   the proceeds settle, so settled ≥ 0 cannot hold after every approved sell. The property asserts:
   settled ≥ 0 (cash) and buying power ≥ 0 (both) after every approved buy; settled + Σ unsettled −
   accrued ≥ 0 after every event (both); settled ≥ 0 in a cash account whenever nothing is
   unsettled, and never below minus the charges posted since then while something is. This item is
   Proposed (founder): it reads a safety rule more loosely than its text, so the property asserts
   the strictest reading the fee model's rounded charges allow until the founder decides. A
   spec-only change follows for the §8.3 and I4 wording and for §7.2's "round(accrued, 2,
   ceiling)", which item 2's per-bucket rounding contradicts.
6. **Harness.** `initial.account.type`: alpaca defaults to margin and rejects cash; generic must
   state it. `buying_power` is compared with the fold's buying power and no reservations. RC-08 and
   RC-18's cash variant stay pending on E6-3 and E6-6.

## Not done here (with the story that owns each)

| Story | What | Cases waiting |
|---|---|---|
| E6-3 | `propose_order` steps, `decision` expectations, reason codes in §9.1 order | RC-08, RC-17, RC-18 and variant |
| E6-6 | Reservations from open orders, fee reservations, the lower of model and broker buying power, `non_marginable_buying_power`, day trades | RC-08, RC-17, RC-18 and variant |
| E6-9 | `AccountStateObserved`: account type and restrictions as journaled state | RC-15 |
| E7-2, E7-5 | Broker order updates, agents | RC-17, RC-18 |

Also not here: paper-mode simulated fees in accrued fees (§10, E4-2); uncleared deposits (§7.2:
funding is no fold input yet); journaling the fold's records (the executor story).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-accounting -p mandate-refcases
cargo test -p mandate-accounting --test settlement --test settlement_properties -- --include-ignored
cargo test -p mandate-refcases --test harness -- --include-ignored
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

- [ ] The cited reference cases pass, and none that passed before now fails (none move; the
      passing cases keep passing on the stubs).
- [x] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none added; see above).
- [x] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
