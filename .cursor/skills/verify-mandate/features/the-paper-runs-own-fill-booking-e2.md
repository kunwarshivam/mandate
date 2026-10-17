# The paper run's own-fill booking: an owned order's unbooked fills are journaled before the flat-account check, and `--reconcile-only` (E2)

- **Spec:** `docs/project/decisions/DEC-955.md`; trading-domain spec §5.7 (a fill of an order in
  a terminal state is `LateFillApplied`) and §11 (reconciliation adopts nothing the journal does
  not own here); journal spec §9 (`FillApplied` / `LateFillApplied` version 1, DEC-938 items 2 and
  3); DEC-470 item 1 (the flat-account check, unchanged); DEC-157 item 6 (a dry run keeps
  nothing).
- **Code:** `crates/mandate-executor/src/own.rs` — `owed_orders` (the orders the stream submitted
  and has not fully booked) and `book_own_fills` (the fills that name them, matched by client
  order id or the broker's order id, summing exactly to what the broker filled; any disagreement
  drafts nothing). `crates/mandate-alpaca/src/wire.rs` — a `FILL` activity's `order_id` as
  `BrokerFill::broker_order_id`. `crates/mandate-shell/src/tracer/own_fills.rs` —
  `ProductionCycle::book_own_fills` (fold, GET the owned orders and the activities, append at the
  head read); `src/paper/context.rs` — `reconcile_context`, whose binding gate answers nothing.
  `crates/mandate-paper/src/lib.rs` — `Booking`, run before the preflight by a placing or
  `--reconcile-only` run on a paper connection, and `parse`'s `--reconcile-only`.
- **Tests:** `crates/mandate-executor/tests/own_fills.rs` (the E2 entry booked late, a working
  order's fill, nothing unowned booked, every mismatch reason, and a property over which fill
  records are missing); `crates/mandate-alpaca/tests/hand.rs`'s fill keeping the broker's order
  id; `crates/mandate-paper/tests/run.rs`'s E2 state through Postgres (booked then refused at
  Validate with reads only, a third run appending nothing, `--reconcile-only`, a disagreeing
  broker stopping at Reconcile, a dry run booking nothing); `tests/args.rs` and `tests/lines.rs`.
- **Run:** `MANDATE_PG_URL=… cargo nextest run -p mandate-executor -p mandate-alpaca -p mandate-shell -p mandate-paper`.
