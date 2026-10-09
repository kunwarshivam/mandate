# Accounting

- **Spec:** `docs/specs/trading-domain.md` §6, §7.2 (account type and buying power), §8
  (invariants I1 to I7 in §8.6), §12 (the journal events that feed the fold); task briefs
  `docs/project/tasks/E3-1-accounting.md`, `docs/project/tasks/E3-2-corporate-actions.md`, and
  `docs/project/tasks/E3-3-settlement.md`.
- **Code:** `mandate-accounting`: `crates/mandate-accounting/src/lib.rs` (inputs, corporate
  actions, fee configuration, account type, reservations, errors),
  `crates/mandate-accounting/src/account.rs` (the pure fold: positions, cash buckets, fee accrual
  and charges, splits, dividends, cash in lieu, receivables, income, realized and unrealized P&L,
  equity, buying power), `crates/mandate-accounting/src/reservation.rs` (`fee_reservation`: §2.1's
  `round(estimated fees, 2, ceiling)` for a `ProspectiveOrder`, by folding an equity buy; a crypto
  buy and every sell reserve zero, DEC-471 item 3).
- **Tests:** `crates/mandate-accounting/tests/hand.rs` (hand-calculated cases, partial fills, flips),
  `crates/mandate-accounting/tests/properties.rs` (one property per invariant against an i128
  ledger oracle), `crates/mandate-accounting/tests/corporate_actions.rs` (hand-calculated splits,
  dividends, and cash in lieu), `crates/mandate-accounting/tests/corporate_action_properties.rs`
  (I1, I3, I5, I6 and the split, mark, dividend, and receivable rules against an i128 oracle),
  `crates/mandate-accounting/tests/settlement.rs` (hand-calculated buying power in cash and margin
  accounts, pending charges, reservations), `crates/mandate-accounting/tests/settlement_properties.rs`
  (buying power and the no-debit rule I4 against an i128 oracle whose holidays come from the
  `us_2026` calendar fixture, with a gated generator and a live guard on the branches it reaches),
  `crates/mandate-accounting/tests/reservation.rs` (hand-calculated CAT reservations at the cent
  ceiling, the no-fee schedule, sells and crypto buys at zero, and the zero-quantity refusal).
- **Reference cases:** `trading_domain::*` in `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-accounting`.
