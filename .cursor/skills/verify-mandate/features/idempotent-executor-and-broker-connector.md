# Idempotent executor and broker connector

Planned by [the E7-2, E7-3 and E7-4 task brief](../../../../docs/project/tasks/M6-K-executor-and-connector.md)
and DEC-133. The tests PR has landed the two crate skeletons, their stubs, and the suite; the
implementation PR turns the pending tests green without editing them (DEC-77).

- **Spec:** `docs/specs/trading-domain.md` section 5.1 to 5.7 (the v1 order policy, the Alpaca
  capability matrix, the constraints before submission, protective exits and the tranche model, the
  kill switch, exit pricing, and the order lifecycle with the broker status mapping), 6.1 (the fill
  record), 7.1 to 7.4 (the account ledger, buying power, account restrictions, agent modes), 9.1 (the
  binding gate the executor runs), 9.6 and 9.7 (conduct controls and rate limits), 10 (paper mode and
  the shadow ledger), 11 (reconciliation), 12 (the account-stream events);
  `docs/specs/journal.md` section 2 (the account stream's single writer, copied facts, `intent_id`),
  5.1 (append, idempotency, fencing), 5.2 (write before acting, recovery by `client_order_id`), 8
  (replay and `fold_version`), 9 (the account-stream catalogue); `docs/HLD.md` section 5 ("Durability")
  and 6.D (crash recovery); ADR-0001 ES-02, ES-06, ES-09, ES-19, ES-20, ES-21, ES-23, ES-24; backlog
  E7-2, E7-3, E7-4.
- **Code:** `crates/mandate-executor/src/state.rs` (`ExecutorState` and `fold`),
  `crates/mandate-executor/src/step.rs` (`handle`, the only producer of effects),
  `crates/mandate-executor/src/ids.rs` (`ClientOrderId`, three derivations and one validating
  parser, no free constructor), `crates/mandate-executor/src/types.rs` (the vocabulary, including
  `BrokerRequest` and `AccountWideScope`), `crates/mandate-executor/src/reconcile.rs`,
  `crates/mandate-executor/src/protection.rs`, `crates/mandate-executor/src/kill.rs` (the
  agent-scoped kill switch of section 5.5: the final mode first, the agent's orders cancelled by
  id, and its sub-ledger sold through the executor's own `k-<switch>-<n>` flatten intents once the
  cancels confirm; the account and workspace scopes answer their stub; DEC-485),
  `crates/mandate-executor/src/gate.rs` (the binding
  gate's call site), `crates/mandate-executor/src/ports.rs`, `crates/mandate-executor/src/error.rs`,
  `crates/mandate-executor/src/opening.rs` (`equity_bracket_prices`, the bracket's stop and
  take-profit rounded up onto the Reg NMS grid, and `BrokerAccount::one_x_buying_power`, with
  in-module hand cases; DEC-471 item 3);
  `crates/mandate-alpaca/src/http.rs` (the paper host, the endpoint allowlist, `secrecy`-held
  credentials), `crates/mandate-alpaca/src/wire.rs`, `crates/mandate-alpaca/src/client.rs`,
  `crates/mandate-alpaca/src/record.rs` (the redaction pass), `crates/mandate-alpaca/src/error.rs`,
  and E7-8's reference-data reads (DEC-168): `crates/mandate-alpaca/src/read.rs` (the asset record
  and the latest quote as exact values, and their refusals) and `crates/mandate-alpaca/src/data.rs`
  (the data host's own request types and transport trait), plus DEC-471's one GET of the latest
  complete IEX minute bars (`BarsRequest::recent_minutes`, `DataClient::recent_minute_bars`, and
  `read::minute_bars`, which refuses another symbol, a second page, an empty page, and a bar off the
  grid or outside the window).
  In prose: `mandate-executor` (`fold` and `handle` over the account stream, the intent protocol,
  `ClientOrderId` with three derivations and no free constructor, the section 5.7 order state machine,
  reservations released by the whole terminal set, the protective sequences and the exit ladder,
  reconciliation whose adoption is scoped to the order set, and the `BrokerRequest` enum whose
  account-wide variants need an `AccountWideScope`, plus the `BrokerConnector` trait; an intent enters as
  `Input::Intent` and the adapter implementing stream I's `IntentSink` lives in the layer-8 shell
  (`mandate-shell`; DEC-138 amends DEC-133 item 1), since
  the two crates share a layer) and `mandate-alpaca` (new; the paper
  trading client behind an injected transport and clock, the endpoint allowlist, `secrecy`-held
  credentials from an injected lookup, raw-text numbers into `mandate-num`, and the broker status and
  reject mappings). It calls `mandate-risk` directly as the binding gate and reads
  `mandate-accounting` and `mandate-journal` unchanged. The shell that binds runtime, executor, and
  connector is not here.
- **Tests:** `crates/mandate-executor/tests/binding_gate.rs`,
  `crates/mandate-executor/tests/hand.rs`,
  `crates/mandate-executor/tests/properties.rs`, `crates/mandate-executor/tests/fault.rs`,
  `crates/mandate-executor/tests/refcases.rs`, `crates/mandate-executor/tests/common/mod.rs`,
  `crates/mandate-executor/tests/common/golden.rs`,
  `crates/mandate-executor/tests/golden-journal.json`;
  `crates/mandate-alpaca/tests/hand.rs`, `crates/mandate-alpaca/tests/properties.rs`,
  `crates/mandate-alpaca/tests/fixtures.rs`, `crates/mandate-alpaca/tests/common/mod.rs`,
  `crates/mandate-alpaca/tests/fixtures/record.sh`, and the recorded scenarios under
  `crates/mandate-alpaca/tests/fixtures/alpaca-trading/`; for E7-8,
  `crates/mandate-alpaca/tests/reads.rs` and the latest-quote scenarios under
  `crates/mandate-alpaca/tests/fixtures/alpaca-data/`; for DEC-471,
  `crates/mandate-alpaca/tests/bars.rs` (request construction and its refusals, exact volumes, and
  every refused answer over a scripted transport) and the bars cases in `src/http.rs`; for A1
  (E7-19, [DEC-524](../../../../docs/project/decisions/DEC-524.md)),
  `crates/mandate-alpaca/tests/margin.rs` (`last_equity` and `maintenance_margin` parsed from the
  recorded accounts and an edited body whose members all differ) and
  `crates/mandate-executor/tests/margin.rs` (maintenance excess, hand cases and a whole-cent `i128`
  property); for E1b-0 (E7-19, [DEC-853](../../../../docs/project/decisions/DEC-853.md)),
  `crates/mandate-executor/tests/cancel_openings.rs` (one agent's openings in one instrument
  cancelled by command). In prose: the hand cases of the brief
  (the submission chain, the `Unknown` lookup discipline, the
  status mapping, the protective and kill-switch sequences, the ladder, the restriction table, error
  codes), twelve `fault::crash_at_*` cases at the enumerated submission steps, and property tests
  against four independent oracles: a broker-side submission counter inside the fake connector, a
  shadow order book rebuilt from the drafts' canonical bytes, an `i128` shadow position ledger, and a
  protection accountant that finds every unprotected interval. Alpaca fixtures follow
  `mandate-marketdata`'s recorded-scenario shape adapted for a write API (method and body in
  `requests.txt`, `response-N.json`, its own `record.sh`), with `statuses.txt` beside them because a
  trading endpoint distinguishes a rejection from a duplicate id from an absence by status: eleven
  scenarios are recorded against the paper host and twelve are hand-built from the spec. Planted bugs
  per test (21): the task brief.
- **Startup hold:** the gate holds, never denies, an opening until an account has been journaled
  and a reconciliation has run since `Input::Started` (`gate::unreconciled_opening`,
  `ExecutorState::reconciled_since_start`; the coordinator's rulings on #174). Its three live cases
  are in-module in `crates/mandate-executor/src/reconcile.rs`:
  `an_opening_is_held_until_a_reconciliation_has_run_since_the_start` (a reported account, the
  release at the next tick, and every non-adding purpose allowed on a stream with neither),
  `a_run_from_before_the_restart_does_not_release_an_opening`, and
  `the_hold_lifts_on_an_account_and_a_run_in_either_order`. Run: `cargo nextest run -p
  mandate-executor --lib reconcile::tests`.
- **Rule 5's bounded wait:** an exit waits on its agent's opening in the instrument, in any state,
  for at most `unknown_absent_window_s` from the `GateDecided` that allowed it, and every waiting
  exit is re-evaluated after every step (`protection::overdue_openings`,
  `protection::release_waiting`; [DEC-160](../../../../docs/project/04-decision-log.md) (7), (13)). Its
  cases are in-module in `crates/mandate-executor/src/protection.rs` (`sequence_tests`): the
  reviewer's three paths, and `no_exit_waits_past_the_bound_but_under_a_rule_13_hold`, a property
  over random scripts against an oracle read from the drafts. Run: `cargo nextest run -p
  mandate-executor --lib protection::sequence_tests`.
- **Account-stream writers (E7-10, DEC-305 to DEC-307, DEC-389, DEC-390):** `risk_clock` as
  §9.2's whole-second timestamp (`payload::risk_clock_stamp`, stamped by `Batch::journal`; the
  fold reads it and the older integer seconds through `payload::clock_of`), the fee step's pause
  and alert whether or not its snapshot recorded (`reconcile::fee_step_pause_and_alert`), the fee
  step's §9.2 snapshot payload (`reconcile::fee_step_snapshot_fields`), and §9.1's
  `IntentReceived` and `OrderSubmitted`'s `limit_price` (`intent::intent_received_fields`,
  `intent::order_submitted_optional_fields`); the last three are not yet wired (DEC-389 items 2
  and 3). Pins are in-module and read the journal vectors: `payload::stamp_tests`,
  `intent::draft_member_tests`, and `reconcile::tests::the_fee_steps_*`. Run: `cargo nextest run
  -p mandate-executor -E 'test(fee_steps) | test(draft_member_tests) | test(stamp_tests)'`.
- **Reference cases:** none move in the tests PR. The harness steps and keys this stream owns are
  `broker_order_update` and `orders` (E7-2), `reconciliation` and `broker_position_update` (E7-3), and
  `corporate_action_prepare`, `actions`, `protective_sell_qty` and `initial.open_orders` (E7-4); they
  move in a status PR after the implementation, turning `trading_domain::RC-14` and its four variants,
  `RC-04`, `RC-06`'s `protective_orders_kept_through_dividend`, `RC-07`, `RC-11`, `RC-20`, `RC-21` and
  `RC-24` green.
- **Run:** `cargo nextest run -p mandate-executor -p mandate-alpaca`.
