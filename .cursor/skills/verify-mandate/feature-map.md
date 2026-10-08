# Feature map

What exists, where it lives, what proves it, and how to run it. `cargo xtask ci lint` checks that
every workspace crate and reference-case suite has an entry and that every path named here exists.

## Canonical JSON and hashing

- **Spec:** `docs/specs/journal.md` §4; ADR-0001 ES-07.
- **Code:** `mandate-canon`: `crates/mandate-canon/src/parse.rs` (strict parser),
  `crates/mandate-canon/src/write.rs` (canonical writer), `crates/mandate-canon/src/lib.rs`
  (value tree, keys, SHA-256 `Digest`).
- **Tests:** `crates/mandate-canon/tests/canon.rs` (vectors, rejections, differential against
  `serde_json_canonicalizer`, scrambled spellings).
- **Reference cases:** `journal::string_escaping` in `fixtures/refcases/journal.json`.
- **Run:** `cargo nextest run -p mandate-canon`.

## Journal decimals

- **Spec:** `docs/specs/journal.md` §4.6; ADR-0001 ES-04.
- **Code:** `crates/mandate-canon/src/dec.rs` (`DecStr`: normalize, reject, never round).
- **Tests:** `crates/mandate-canon/tests/decimal.rs`.
- **Reference cases:** `journal::decimal::*`.
- **Run:** `cargo nextest run -p mandate-canon decimal`.

## Timestamps and dates

- **Spec:** `docs/specs/journal.md` §4.7; ADR-0001 ES-05; RFC 3339 §5.6 for `parse_rfc3339`.
- **Code:** `mandate-time`: `crates/mandate-time/src/lib.rs` (`UtcNanos`, `Date`;
  `UtcNanos::parse` takes only the canonical form, `UtcNanos::parse_rfc3339` zero to nine
  fractional digits and an offset).
- **Tests:** `crates/mandate-time/tests/time.rs` (naive day-count oracle);
  `crates/mandate-time/tests/rfc3339.rs` (place-value oracle, differential against a port of the
  market-data parser); `crates/mandate-journal/tests/timestamp_forms.rs` (the journal keeps the
  canonical form and a whole-second `risk_clock`).
- **Run:** `cargo nextest run -p mandate-time`.

## Typed decimals

- **Spec:** `docs/specs/trading-domain.md` §2.1; ADR-0001 ES-04.
- **Code:** `mandate-num`: `crates/mandate-num/src/lib.rs` (private-field newtypes `Qty`,
  `SignedQty`, `Price`, `Usd`, `CostBasis`, `FeeRate`, `FeePerShare`, `Bps`, `FeeCap`; 12-place
  `MarkPrice`, `SplitRatio`, `ShareIncrement`, and the `SplitQty` result of a split; canonical
  text in, exact-or-error operations), `crates/mandate-num/src/exact.rs` (256-bit intermediates,
  one rounding per formula).
- **Tests:** `crates/mandate-num/tests/num.rs` (i128 oracle for every operation and rounding mode).
- **Run:** `cargo nextest run -p mandate-num`.

## Trading calendars and trade dates

- **Spec:** `docs/specs/trading-domain.md` §6.2, §6.3; ADR-0001 ES-05.
- **Code:** `crates/mandate-time/src/calendar.rs` (`TradingCalendar`, equity trade date with the
  20:00 New York cutoff, settlement date, New York midnight through jiff's bundled tzdb).
- **Tests:** `crates/mandate-time/tests/calendar.rs` (naive DST-rule and day-count oracle).
- **Run:** `cargo nextest run -p mandate-time calendar`.

## Accounting

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

## Backtest fill model

- **Spec:** `docs/specs/trading-domain.md` §6.4 (the fill model, rules 1 to 9), §2.1 (backtest fill
  prices are not tick-rounded), §4.1 to §4.4 (bars, data profiles, sessions, halts), §8.2 (a
  backtest mark is the bar close); DEC-106 records the choices §6.4 leaves open.
- **Code:** `mandate-sim`: `crates/mandate-sim/src/lib.rs` (bars with their session labels, orders,
  configuration, fills, end states, errors), `crates/mandate-sim/src/fill.rs` (the pure walk over
  bars: timing, sessions, the shared volume cap, market, limit, stop, stop-limit, and OCO rules,
  and `check_sessions_of_asset_class`, which refuses a bar on a session its instrument's asset class
  never trades, spec §4.3, E4-3, DEC-377);
  the arithmetic is `mandate-num`'s (`Fraction`, `Qty::portion`, `Price::slipped`,
  `Bps::sqrt_impact`).
- **Tests:** `crates/mandate-sim/tests/hand.rs` (every order of RC-10, RC-12, and RC-19 recomputed
  by hand, plus the rules those cases do not reach: latency, day-order cancellation, the shared cap,
  the 20-session median, `sqrt` impact, adverse rounding, crypto, and E4-3's session-of-asset-class
  refusal by bar index, code, and message),
  `crates/mandate-sim/tests/properties.rs` (one property per rule and per never-or-always clause,
  against an order-major `i128` simulator as the oracle, and E4-3's refusal against spec §4.3's
  session table), `crates/mandate-num/tests/num.rs` (the
  fill model's arithmetic against an i128 oracle and hand-computed roots),
  `crates/mandate-refcases/tests/harness.rs` (the backtest harness reads every key the cases state).
- **Reference cases:** `trading_domain::RC-10`, `RC-12`, and `RC-19` in
  `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-sim`, and for the three cases
  `cargo test -p mandate-refcases --test refcases -- --include-ignored --exact trading_domain::RC-10
  trading_domain::RC-12 trading_domain::RC-19`.

## Baseline backtest and metrics report

- **Spec:** `docs/specs/trading-domain.md` §2.1 (decimals, one rounding per formula, limit prices on
  the tick), §2.2 (trade dates), §5.1 and §5.3 (the order policy the baseline obeys), §6.2 and §6.3
  (the instants fees are charged at), §8.2 to §8.4 (the backtest mark is the bar close, cash,
  settlement); PRD FR-4.1, FR-4.2, FR-4.5; `docs/specs/journal.md` §12 (`BacktestRunRecorded`,
  appended by a later story). The metric definitions have no spec section yet: they live in
  `docs/project/tasks/E4-2-backtest-baseline.md` and DEC-127.
- **Code:** `mandate-backtest`: `crates/mandate-backtest/src/lib.rs` (the loop's inputs, its per-bar
  order, the run's fills, orders, and observations), `crates/mandate-backtest/src/strategy.rs` (the
  strategy dispatch and the buy-and-hold benchmark),
  `crates/mandate-backtest/src/strategy/ma_crossover.rs` (the division-free moving-average
  crossover and the `Signal` it answers with, in one file so DEC-504's content hash can list it;
  E15-13 M0),
  `crates/mandate-backtest/src/metrics.rs` (every figure of a block),
  `crates/mandate-backtest/src/report.rs` (both blocks, the input digests, the canonical bytes and
  their SHA-256). It drives `mandate-sim`'s fill model and `mandate-accounting`'s fold unchanged; the
  exact arithmetic is `mandate-num`'s (`Ratio`, `Usd::ratio_to`, `Usd::shares_at`,
  `Ratio::sample_variance`, the 12-place roots, and `Price::on_tick` with the Reg NMS table).
- **Tests:** `crates/mandate-backtest/tests/hand.rs` (the brief's fixture and its three degenerate
  series recomputed, the loop's per-bar order, the fee instants, the periods, the signal, the
  benchmark, the identifiers, and the canonical bytes),
  `crates/mandate-backtest/tests/properties.rs` (an `i128` statistics oracle and a sequencer that
  rebuilds the expected input series through `Account`, one property per never-or-always clause),
  `crates/mandate-num/tests/num.rs` (the new arithmetic against the i128 oracle and the brief's
  hand-computed figures). Planted bugs per test: the task brief.
- **Reference cases:** none. The backtest cases `trading_domain::RC-10`, `RC-12`, and `RC-19` belong
  to E4-1, and `fixtures/refcases/trading-domain.json` holds no metrics case.
- **Run:** `cargo nextest run -p mandate-backtest -p mandate-num`; the pending tests with
  `cargo nextest run -p mandate-backtest --run-ignored all`.

## Autonomy and the order builder (E6-2)

Planned by [the E6-2 task brief](../../../docs/project/tasks/E6-2-autonomy-and-order-builder.md) and
DEC-130. The implementation lands in two slices. Slice 1 (autonomy, DEC-152) implements §6.2 in
`autonomy.rs` and §6.3's `Condition::matches` in `mandate-spec`, and its 40 tests are live: family A
(`MC-A01` to `MC-A16`), 16 hand tests and 8 properties. Slice 2 (the builder) implements §8.1 to
§8.3 in `builder.rs` and the sizing arithmetic in `mandate-num`, and the rest go live: family B (the
28 builder cases), 46 hand tests, 18 properties, and the two `mandate-num` tests. No E6-2 test in
the crate is pending.

- **Spec:** `docs/specs/mandate.md` §6.1 to §6.4 (purposes, the evaluation order, the condition
  language, the approver count and the skip-on-timeout), §8.1 to §8.3 (the signal-model contract,
  output freshness, and `conviction_linear`), §3.1 (`accumulate` and `profit_stop`), §5.2 and §5.3
  (exact comparisons and the limits the proposal is clipped to); `docs/specs/trading-domain.md` §5.1
  and §5.3 (the v1 order policy), §8.2 (the risk mark), §9.1 and §9.6 (the gate's verdicts and the
  pacing of a discretionary exit).
- **Code:** `mandate-builder`: `crates/mandate-builder/src/lib.rs` (the crate's contract and
  `BuilderError`'s refusals with their stable codes),
  `crates/mandate-builder/src/autonomy.rs` (§6.2's order, the built-in AUTO purposes, the first
  match, the default, the admission ceiling, the approver count, and the `Facts` a proposed action
  presents to a §6.3 rule, with in-module tests of the rule walk, the re-check and `decide`),
  `crates/mandate-spec/src/condition.rs` (`Condition::matches`, with in-module tests of every
  operator and combinator), `crates/mandate-builder/src/builder.rs` (§8.1 and §8.2's pinned triple
  and freshness, and §8.3's combine, decide, size, accumulate clips and minimum order, and
  `buy_action`, the classification facts of a buy that `propose` and a caller stating them up front
  share). The exact
  arithmetic is `mandate-num`'s (ES-04): `crates/mandate-num/src/sizing.rs` (`SizeFraction`, `Unit`,
  `Conviction`, `Signed`, the two `weighted_ratio` quotients, and `UsdExact`). The gate's dry-run
  verdict reaches the crate as a value, so `mandate-risk` is not a dependency (DEC-130 item 2), and
  the §6.3 condition tree is `mandate-spec`'s (DEC-128 item 18).
- **Tests:** `crates/mandate-builder/tests/hand.rs` (54 tests: every §6 and §8.3 figure recomputed
  by hand from the rule, the two freshness bounds at their exact instants, the tie-break among
  duplicate outputs, the four clips, the accumulate clips with fees, and the two orderings DEC-130
  item 21 fixes), `crates/mandate-builder/tests/properties.rs` (25 properties against three
  independent oracles: the combine step as `i128` integer arithmetic with its own half-even
  rounding, the sizing chain as rationals compared by cross-multiplication, and a naive rule walk
  that re-reads the list from the start), `crates/mandate-builder/tests/refcases.rs` (the 16 `MC-A`
  and 28 `MC-B` cases loaded from the fixture, one test per case id),
  `crates/mandate-builder/tests/common/mod.rs` (the two reference bases as typed inputs),
  `crates/mandate-builder/tests/buy_action.rs` (the after-values of an opening and an increase by
  hand, the crossed and overnight refusals, and `propose`'s action equal to `buy_action`'s), and the
  two `mandate-num` additions in `crates/mandate-num/tests/num.rs`. Planted bugs per test: the task
  brief and the tests PR's body.
- **Reference cases:** the 16 `mandate::MC-A` cases and the 28 `mandate::MC-B` builder cases other
  than `MC-B17` and `MC-B30` to `MC-B32`, in `fixtures/refcases/mandate.json`. Family A also runs in
  the shared harness, through `crates/mandate-refcases/src/mandate/autonomy.rs` on the parsed
  mandate's own `autonomy` block (DEC-162); its `status.toml` rows move in a status-only PR, since
  the spec guard keeps that file apart from code (ES-22). Family B (all 32 `MC-B` cases) runs in the
  shared harness through `crates/mandate-refcases/src/mandate/order_builder.rs` (DEC-250): `propose`,
  then `mandate_risk::evaluate` on the proposed order as §6.2 step 2's dry run, then `decide` on
  that verdict, with the session and close window from `mandate_risk::session_at`. All 32 pass:
  `MC-B22` after hours and `MC-B23` in the close window since #347 moved their clocks, the three
  crypto buys, `MC-B26` to `MC-B28`, since E6-10's check 2 (#422), and the four `trim_to_target`
  cases since the trim arm compares `mandate_risk::trim_proposals`' trim and `ref.py`'s guards
  (E6-4, DEC-400), and `status.toml` lists all four as passing. Its in-module tests doctor the fixture to prove
  every member is read, a cash fee rate the gate would not reserve is refused, and a `session` or
  `in_close_window` label that contradicts `now` fails the case.
- **Run:** `cargo nextest run -p mandate-builder -p mandate-num`; families A and B in the shared
  harness with `cargo nextest run -p mandate-refcases --run-ignored all mandate::MC-A
  mandate::autonomy mandate::MC-B mandate::order_builder` (the flag runs cases `status.toml` does
  not yet list as passing).

## The client ceiling (E6-12)

Planned by [the E6-12 task brief](../../../docs/project/tasks/E6-12-client-ceiling.md) and DEC-262.
The DEC-77 tests PR ([#369](https://github.com/kunwarshivam/mandate/pull/369)) stubbed
`client_ceiling` with 12 tests pending on it; the implementation PR makes them live, and no E6-12
test is pending.

- **Spec:** `docs/specs/mandate.md` §6.2 step 5a and MI-30 (DEC-185), §6.4's `decided_by`.
- **Code:** `crates/mandate-builder/src/autonomy.rs` (`RequestedBy`, `ActionContext::requested_by`,
  `DecidedBy::ClientCeiling`, and the ceiling as the last step of `classify`), and the one line of
  `crates/mandate-builder/src/builder.rs` that stamps `propose`'s buys as the agent's own.
- **Tests:** `crates/mandate-builder/tests/hand.rs` (twelve tests, from
  `a_proposed_buy_is_the_order_builders_own_request` on),
  `crates/mandate-builder/tests/properties.rs` (two generated properties against the naive rule walk
  extended by step 5a, and an exhaustive sweep of 4,212 decisions with its own deny-or-ask oracle).
  Planted bugs: the task brief.
- **Reference cases:** none; no mandate case states `requested_by` (DEC-262 item 7).
- **Run:** `cargo nextest run -p mandate-builder --run-ignored all`.

## Tripwires (E6-13)

Planned by [the E6-13 task brief](../../../docs/project/tasks/E6-13-tripwires.md) and DEC-350 to
DEC-352. The DEC-77 tests PR ([#613](https://github.com/kunwarshivam/mandate/pull/613)) fixed the
public boundary and pending tests. The implementation removes those pending markers; MC-W01 to
MC-W57 remain pending in `status.toml` until the separate status-only PR.

- **Spec:** `docs/specs/mandate.md` MI-3, MI-7, MI-28, MI-31, V-044, §5.2, §5.8 to §5.10,
  §6.1, §6.5, §6.7, §9.2, and §11.
- **Code:** `crates/mandate-spec/src/document.rs` and `document/parse.rs` (the document
  vocabulary and strict parser), `crates/mandate-spec/src/validate.rs` (V-044),
  `crates/mandate-spec/src/change.rs` (§9.2 classification),
  `crates/mandate-executor/src/tripwire.rs` (the pure account-stream fold, metric accumulation,
  firing, latching, ordered journal effects, and step-up acknowledgment), and
  `crates/mandate-spec/src/risk/fold.rs` (the executor projection after the lifetime floor and
  before effective-mode application). The executor receives net realized P&L from
  `mandate-accounting`; it does not recompute cost basis.
- **Tests:** `crates/mandate-spec/tests/tripwires.rs` (parsing, V-044, V-020/V-042 interaction,
  classification, and its independent property oracle),
  `crates/mandate-executor/tests/tripwires.rs` (all three metrics, exact boundaries, version and
  risk-day behavior, ordered opaque effects, latching, acknowledgment refusals, replay, late
  fills, and restart, with independent sequence oracles), and the in-module integration tests in
  `crates/mandate-spec/src/risk/fold.rs` (the projection's restriction, lift, and event order).
- **Reference cases:** `mandate::MC-W01` to `MC-W57` in `fixtures/refcases/mandate.json`, through
  `crates/mandate-refcases/src/mandate/tripwire.rs`; MC-W52 and MC-W57 also run through the
  risk-state adapter in `crates/mandate-refcases/src/mandate.rs`. The harness derives realized P&L
  through `mandate-accounting` and compares every stated step, probe, state, and journal member.
- **Run:** `cargo nextest run -p mandate-spec -p mandate-executor`; all pending family-W cases with
  `cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/MC-W/)'`.

## Agent runtime and kill switches

Planned by [the E6-1 and E6-5 task brief](../../../docs/project/tasks/E6-1-agent-runtime-and-kill-switches.md)
and DEC-131, and implemented in the DEC-77 stage-3 PR: every stub carries its real logic, the 88
pending markers are gone, and all 101 tests run live: the round-3 sanctioned case plus the twelve the
implementation reviews' rulings added, one per finding (DEC-131 item 25(k)).

- **Spec:** `docs/specs/mandate.md` section 2 (lifecycle and applying a version), 2.3 (the working
  universe as runtime state), 5.2 (inputs, the risk clock, MI-13), 5.5 (the agent-scoped kill
  switch), 5.9 (restrictions and the effective mode, MI-6), 6.4 (approvals and the `skip` timeout);
  `docs/specs/trading-domain.md` section 5.5 (kill switch), 5.6 (exit pricing), 7.4 (agent modes);
  `docs/specs/journal.md` section 2 (streams, single writers, copied facts, a kill switch as a
  command), 5.1 (append and fencing), 5.2 (write before acting, crash recovery), 8 (replay and
  `fold_version`), 9 (the agent-stream catalogue); `docs/HLD.md` section 5; ADR-0001 ES-06, ES-20,
  ES-21, ES-24.
- **Code:** `mandate-runtime` (new): `crates/mandate-runtime/src/state.rs` (`RuntimeState`, `fold`,
  `FOLD_VERSION`), `crates/mandate-runtime/src/step.rs` (`handle`, the only producer of effects),
  `crates/mandate-runtime/src/types.rs` (`Input`, `Effect`, `Mode`, `Initiator`, `KillScope`,
  `FlattenPlan` — which has no account-wide variant, so `cancel-all` and `close-position` are
  unrepresentable), `crates/mandate-runtime/src/ports.rs` (the pure `IdGen`, `GateDryRun`, and
  `OrderPlan` in `Ports`, and the shell-driven `IntentSink` and `TimerSource`),
  `crates/mandate-runtime/src/error.rs` (`RuntimeError` with a stable `code()` per variant), and
  `crates/mandate-runtime/src/payload.rs` (the one place a journaled payload becomes a core value and
  a core value becomes a canonical payload again, so every draft the runtime writes is one its own
  fold can rebuild state from). Over
  `mandate-journal`'s drafts and append protocol unchanged. The shell (tokio, the
  Postgres `LISTEN`/`NOTIFY` tail) is an M6 crate and is not here.
- **Tests:** `crates/mandate-runtime/tests/hand.rs` (75 hand cases: the fold's sequencing and loud
  refusals, the risk clock and deadlines, derived ids and fencing, modes and restrictions, decisions,
  approvals, version application, recovery, and the kill switches),
  `crates/mandate-runtime/tests/properties.rs` (26 properties against three oracles that share no
  code with the crate: a shadow fold rebuilt from the emitted drafts' payloads, a separately written
  restriction lattice, and an interval accumulator for durations),
  `crates/mandate-runtime/tests/observation.rs` (`ObservationRecorded` as journal spec §9.1 closes
  it, checked against `mandate-journal`'s registered schema: E15-13, slice R0 of the
  [first paper trade brief](../../../docs/project/tasks/first-paper-trade.md)),
  `crates/mandate-runtime/tests/common/mod.rs` (the in-memory shell, which can put an append in doubt,
  fence a writer, and crash and restart), and `crates/mandate-runtime/tests/golden-journal.json` (the
  committed fold output that pins `FOLD_VERSION`). Planted bugs per test: the task brief.
- **Reference cases:** none move. `trading_domain::RC-14`'s `kill_switch` variant also needs E7-2's
  `actions` and E6-9's `agent_mode`; the mandate suite's flatten family MC-F01 to MC-F04 belongs to
  `mandate-risk`.
- **Run:** `cargo nextest run -p mandate-runtime`, and the mutation gate the implementation PR must
  pass, `MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants`.

## The tracer bullet: one order end to end on Alpaca paper

Planned by [the E7-7 task brief](../../../docs/project/tasks/E7-7-tracer-bullet.md) and DEC-138; the
tests PR (DEC-157) landed the fail-closed crate, and the consolidated implementation in
[PR #598](https://github.com/kunwarshivam/mandate/pull/598) wires the validated mandate,
freshness-bounded stored bars, signal, builder, advisory and binding gates, runtime, memory/Postgres
journal, executor, Alpaca paper connector, and startup/restart reconciliation through the real
crates.

- **Spec:** `docs/HLD.md` section 5 (the runtime's components in order, "Durability": the write-ahead
  intent and event-sourced state), 6.B (the decision cycle), 6.D (crash recovery);
  `docs/specs/trading-domain.md` section 5.1 (limit openings in the regular session), 5.7 (the order
  lifecycle), 9.1 (the binding gate), 10 (paper mode), 11 (reconciliation), 12 (the account-stream
  events); `docs/specs/mandate.md` section 5.3, 6, 8; `docs/specs/journal.md` section 5.1 (append,
  idempotency, fencing), 5.2 (write before acting), 8 (replay), 11 (verification); ADR-0001 ES-02,
  ES-06, ES-09, ES-19, ES-20, ES-21, ES-23; backlog E7-7.
- **Code:** `crates/mandate-shell/` (layer 8, safety-critical, impure). `src/stages.rs` holds the
  `Stage` enum (the suite's contract) and one trait per stage; `src/tracer.rs` the one pass and the
  effect runner, which appends every draft before it hands an intent and lets a submission out only
  after its `OrderSubmitted` committed in the same run; `src/map.rs` the total mappings with no
  permitting arm for any non-answer; `src/envelope.rs` the journal envelope (always `paper`) and the
  deterministic ids; `src/host.rs` the refusal of a configured host; `src/cli.rs` and
  `src/bin/mandate-tracer.rs` the binary; `src/control.rs` the deployment input from the control
  stream (E19-11, DEC-505; tests in `crates/mandate-shell/tests/control.rs`) and the effective
  registrations with the DEC-523 snapshot (tests in `crates/mandate-shell/tests/registrations.rs`),
  and the policy set and model registry that govern the run, DEC-534 (tests in
  `crates/mandate-shell/tests/governance.rs`);
  `src/paper.rs` the DEC-466 one-run loader that verifies the reviewed artifacts and binds the
  bytes it checked (`Artifacts::from_registered` takes them from the confirmed version and the
  registered objects instead of files, Q1; tests in `crates/mandate-shell/tests/registered.rs`), reads the GET-only broker preflight (including the trailing window's
  IEX minute bars, DEC-471) and the liquidity facts into one `PaperFacts` snapshot, refuses any
  missing, stale, or ambiguous fact or a non-clean account, and assembles the trusted run and
  executor contexts from that snapshot alone (DEC-470); `src/adapters.rs` the production adapters:
  `StoredBars`, `MovingAverage`, `AlpacaConnector`, `RiskExitPath`, the validated mandate, builder, gate, journal, executor, and
  reconciler. The risk exit plans over the journal, stream, clock, and agent the run's bridge hands
  it (DEC-449, amended by DEC-451). It holds no trading logic—no sizing, gating, pricing, state
  machine, or arithmetic on money or quantity—and binds the owning crates directly.
- **Tests:** `src/stages/fail_closed.rs` over the permissive doubles of `src/stages/doubles.rs`: one
  case per `Stage`, each asserting at the furthest boundary its stage could reach (zero submissions;
  zero hands up to `Sink`; zero `IntentProposed` up to `Journal`; zero `OrderSubmitted` up to
  `Executor`; the stub reached, nothing downstream of it), the all-stubs case, the all-doubles
  keystone that places exactly one order, and the named scenarios (`flatten_poison_halts`,
  `refuses_to_start_while_protection_is_unimplemented`, `reconcile_mismatch_pauses`,
  `a_restart_sends_nothing_and_a_repeat_run_is_refused`, `ask_journals_the_request_and_sends_nothing`,
  `gate_allow_with_not_reached_refused`, `proposal_sanity`, and more), the environment scanner and the
  shadow order book over the committed bytes, and the host, transport, and defaulting-combinator
  source scans; `src/map.rs`'s properties that no source error or gate answer maps to a permitting
  verdict (TI-3) and that an opening `Allow` with a `NotReached` check is refused (TI-11);
  `src/adapters.rs`'s cases for the stored-data trust rule, the crossover's envelope windows, and the
  purpose-flag and ownerless-exit mappings of the flatten adapter; `tests/flatten_probe.rs` the
  flatten adapter's nine pins over a self-proving §9.5 journal (DEC-449, DEC-451).
  `tests/tracer.rs` runs the production path over recorded Alpaca paper responses and a bar dataset
  written by `mandate-marketdata`'s own writer (`happy`, `happy_is_deterministic`, `autonomy_ask`,
  `signal_flat`, `signal_undecided`, `stale_stored_bars_are_refused`, `oversized_proposal`,
  `duplicate_after_restart`, `fresh_journal_with_broker_position`, `broker_unknown_then_absent`,
  `reconcile_mismatch_pauses`, and the fixture's validation and one-share sizing). `outlier_close`
  remains pending and failing on E2-14 until its founder-gated price-trust rule lands; the shell
  cannot invent price arithmetic (DEC-138 item 3). The mandate
  fixtures are generated and checked against `reference/mandate/ref.py` by
  `tests/fixtures/tracer/generate.py`. The paper assembly is split by step under `src/paper/`
  (`artifacts`, `facts`, `judge`, `gate`, `context`) and computes no money or quantity: the
  liquidity figures are `mandate-liquidity`'s, the bracket prices and 1× buying power the
  executor's, the classification facts the builder's `buy_action`, and the fee reservation
  `mandate-accounting`'s (DEC-471 item 3). `src/paper/tests.rs` covers the artifact members and
  values, each single-fact refusal and its boundary, the session and close window, the mapping of
  the protection prices and the builder's action, and the per-request fee reservation;
  `src/paper/facts/tests.rs` the liquidity mapping and each refusal it names. `tests/paper.rs`
  drives the shipping assembly (`Artifacts::load`, `preflight`, `liquidity_facts`, `load_contexts`,
  `production`) over one scripted transport that also answers the minute-bars read: GETs only
  before the run, exactly one `POST` after the intent and submission commit, none on a restart, and
  none when the bars read is empty. `tests/binary.rs` holds the binary's order and its pre-credential
  refusals. The real HTTP runtime's I/O driver is tested with a local socket and no test holds paper
  credentials or can reach Alpaca (ES-19).
- **Reference cases:** none move, and `crates/mandate-refcases/status.toml` is untouched by every PR of
  this stream. The tracer cites `trading_domain::RC-04`, `RC-09`, `RC-09B`, `RC-11`, `RC-14`, `RC-16`,
  `RC-17`, the mandate gate and autonomy families, and the journal append vectors read-only.
- **Run:** `cargo nextest run -p mandate-shell`; `cargo xtask ci pending` for the pending cases; the
  manual paper run is `cargo run -p mandate-shell --bin mandate-tracer -- --mandate <path> --dataset
  <dir> --config-dir <dir> --journal <dsn> --confirm-paper --place-one-order`, which needs both
  flags, accepts only the reviewed E7-7 paper artifacts, constructs the fixed Alpaca paper
  transport, refuses any attempt to configure a host, and reads today's minute bars from the data
  host (DEC-471, Proposed; see the task brief).

## Idempotent executor and broker connector

Planned by [the E7-2, E7-3 and E7-4 task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md)
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
  (E7-19, [DEC-524](../../../docs/project/decisions/DEC-524.md)),
  `crates/mandate-alpaca/tests/margin.rs` (`last_equity` and `maintenance_margin` parsed from the
  recorded accounts and an edited body whose members all differ) and
  `crates/mandate-executor/tests/margin.rs` (maintenance excess, hand cases and a whole-cent `i128`
  property). In prose: the hand cases of the brief
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
  `protection::release_waiting`; [DEC-160](../../../docs/project/04-decision-log.md) (7), (13)). Its
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

## MCP transport for broker connectors (E7-16)

- **Spec:** `docs/specs/connections.md` §6.2 rules 1 and 4, §6.5 (the reserved exit budget), CN-9;
  DEC-441 item 8; slice M1 of the first-live-trade brief (#706).
- **Code:** `mandate-mcp`: `crates/mandate-mcp/src/endpoint.rs` (`PinnedEndpoint`: `https` on the
  pinned host only, plain `http` only to a loopback literal in the crate's own test build),
  `crates/mandate-mcp/src/budget.rs` (`RateBudget`: the ordinary bucket and the reserved one only
  risk-reducing calls draw on), `crates/mandate-mcp/src/transport.rs` (`McpTransport`: one `POST`
  per message, the `Mcp-Session-Id` carried, no redirect followed, bounded timeouts and answer size,
  and the injected `Monotonic` clock), `crates/mandate-mcp/src/error.rs` (`McpError`, and
  `ServerText`, which has no `Display` and whose `Debug` withholds what the server sent).
- **Tests:** in-crate where a loopback server is needed, since loopback is accepted only in the
  crate's own test build: `crates/mandate-mcp/src/tests/endpoint.rs`,
  `crates/mandate-mcp/tests/production.rs` (the production build, which refuses plain `http` even to
  loopback), `crates/mandate-mcp/src/tests/budget.rs` (an oracle that steps one refill period at a
  time), `crates/mandate-mcp/src/tests/server.rs` (the scripted loopback server),
  `crates/mandate-mcp/src/tests/answers.rs`, `crates/mandate-mcp/src/tests/errors.rs`.
- **Run:** `cargo nextest run -p mandate-mcp --run-ignored all`.

## Risk gate

Planned by [the E6-3 task brief](../../../docs/project/tasks/E6-3-risk-gate.md) and DEC-129. The
implementation PRs fill the crate in story by story: E6-3 has landed `evaluate` and `agent_flatten`,
E6-9 check 3's halt and no market orders under a presumed halt (a market exit is re-priced),
E6-7 check 2's eligibility floor, E6-6 `session_at`, check 3's sessions, the rest of check 4,
check 7's buying power and check 8's `legacy_pdt` budget with its account-wide ledger fold, and E6-8 check 5's mark and collar,
check 6's conduct controls, the pacing of an allowed exit, `evaluate_cancel` and `surveillance`
(DEC-163), and E6-10 check 2's USD pairs for crypto, read from
`InstrumentSnapshot::quote_currency` (DEC-254, DEC-255), so every check is whole for both asset
classes. Were a check still owed, the gate would fail closed for adding risk (DEC-129 item 29),
while a reducing purpose passes it.

- **Spec:** `docs/specs/trading-domain.md` §9 (§9.1 the evaluation order and reason codes,
  §9.2 the day-trading regime, §9.3 leverage and short sales, §9.4 sessions, §9.5
  buying power, §9.6 market-conduct controls), §3.1 to §3.3 (instrument fields, the
  eligibility floor, concentration), §4.3 and §4.4 (sessions, auction windows, halts),
  §5.1 to §5.6 (the v1 order policy, the constraints before submission, the kill switch,
  exit pricing), §7.2 to §7.4 (buying power, account restrictions, agent modes), §8.2
  (risk marks); `docs/specs/mandate.md` §1.1 (MI-1 to MI-20), §2.3 (the working universe),
  §5.3, §5.5, §5.9; backlog E6-3, E6-4, E6-6 to E6-10.
- **Code:** `mandate-risk`: `crates/mandate-risk/src/lib.rs` (the gate's inputs, the eight §9.1
  checks as `Check`, the four verdicts, `ReasonCode` with the registered spelling of each, `Origin`
  and the `Purpose` it maps to, `GateError`, and the signatures of `evaluate`, `evaluate_cancel`,
  `assign_purpose`, `session_at`, `size_factor`, `trim_proposals`, `agent_flatten`, `fold_day_trades` and
  `surveillance`), `crates/mandate-risk/src/gate.rs` (`evaluate`: the eight checks in order,
  purpose assignment, check 1 whole, the working universe, §5.3 rules 3 and 9, §5.1's limit-only
  openings, the re-pricing of a market exit, and the fail-closed
  refusal of an opening while a check is owed), `crates/mandate-risk/src/limits.rs` (the §5.3
  mandate limits: concentration, order size, the re-entry cooldown, orders per day, and gross
  exposure with the account's own 1×),
  `crates/mandate-risk/src/flatten.rs` (`agent_flatten`, the agent-scoped kill switch's plan),
  `crates/mandate-risk/src/floor.rs` (check 2's eligibility floor, trading spec §3.2),
  `crates/mandate-risk/src/session.rs` (`session_at` from the committed calendar and check 3's
  session and auction-window rules), `crates/mandate-risk/src/account_rules.rs` (§5.3 rules 2 and
  4 to 8, buying power with the fee reservation, and the `legacy_pdt` day-trade budget),
  `crates/mandate-risk/src/daytrades.rs` (`fold_day_trades`, §9.2's `legacy_pdt` ledger folded
  account-wide from every agent's fills, DEC-259, with its in-module hand tests and a running-total
  oracle property),
  `crates/mandate-risk/src/conduct.rs` (check 5's fresh quote and collar, check 6's conduct
  controls, the collar, participation and close-window pacing of an allowed exit, a
  discretionary exit whose collar cannot be computed routed whole by the other controls alone
  (E6-6, DEC-327, DEC-383), and `evaluate_cancel`'s minimum resting time, trading spec §8.2 and
  §9.6),
  `crates/mandate-risk/src/trim.rs` (`trim_proposals`, mandate §5.5's `trim_to_target` under
  DEC-65's guards, DEC-399, with in-module boundary tests),
  `crates/mandate-risk/src/surveillance.rs` (§9.6's daily surveillance report: figures and flagged
  thresholds, concentration a figure only, no judgement), `crates/mandate-risk/src/spec_types.rs` (the stream-F shapes this crate needs
  before `mandate-spec` and `mandate-domain` exist, in the names DEC-128 item 21 fixes; the first
  implementation PR after stream F's tests PR deletes it). It reads `mandate-accounting`'s
  `AccountType`, `AssetClass` and `Side` and changes neither them nor `mandate-time`.
- **Tests:** `crates/mandate-risk/tests/hand.rs` (every MC-G and MC-F figure recomputed from the
  spec, the mode rule, the `Unknown`-order rule, the account states, the eligibility floor, and a
  check that every reason code the gate can emit is registered in the founder-owned case file),
  `crates/mandate-risk/tests/properties.rs` (one property per invariant and per "never" or "always"
  in §9, including MI-1 scoped to its own words, the mode rule, MI-8, a shadow-ledger sequence
  property, and `an_exit_over_extreme_figures_is_still_routed`), `crates/mandate-risk/tests/usd_pairs.rs` (§3.2 item 7's USD pairs for crypto, E6-10:
  a non-USD or unstated pair denied at check 2, a USD pair passing, check 2 whole for crypto, an
  exit in any pair and a US equity never judged by the rule, and a property whose oracle is
  `opening ∧ crypto ∧ quote ≠ USD`; live since E6-10's implementation, #394),
  `crates/mandate-risk/tests/common/mod.rs` (the fixtures and the independent `i128`
  oracle, which never calls the crate's arithmetic), and the in-module tests in `gate.rs`,
  `conduct.rs` and `surveillance.rs` for the boundaries the files above cannot pin (among them
  E6-6's exit routing, DEC-383: an exit over an uncomputable collar routed at its own limit and
  sliced as an `i128` oracle computes, an opening over one keeping the collar's error, a
  proposal of zero refused before the collar is reached (DEC-401), and only `overflow` and
  `not_positive` skipped). Planted bugs per test: the task
  brief.
- **Reference cases:** `mandate::MC-G01` to `MC-G16` and `MC-F01` to `MC-F04` in
  `fixtures/refcases/mandate.json`, through `crates/mandate-refcases/src/mandate/risk_gate.rs`
  (DEC-178; MC-G13 stays pending on E6-8's checks 5 and 6), with
  `crates/mandate-refcases/tests/mandate_gate_harness.rs` proving the two arms read and compare
  every member; `crates/mandate-risk/tests/refcases.rs` is the crate-local copy it replaces, kept
  until a follow-up deletes it; `trading_domain::RC-09`, `RC-09B`, `RC-15`, `RC-16`, `RC-22`
  and `RC-25` with their variants, and the `propose_order` steps of `RC-03`, `RC-08` and `RC-18`,
  in `fixtures/refcases/trading-domain.json`. The trading-domain harness decides them through
  `evaluate` (E6-9, DEC-199): RC-03's `gate_rejects_zero_crossing_order`, RC-08 and RC-18's
  `generic_cash_account` run; the rest fail naming the stories they still wait for (RC-15 on E7-2,
  E7-3, E7-4 and E7-5).
- **Run:** `cargo nextest run -p mandate-risk`, and
  `cargo test -p mandate-refcases --test refcases -- --include-ignored mandate::MC-G` (and
  `mandate::MC-F`).

## Journal drafts and the event catalogue

- **Spec:** `docs/specs/journal.md` §2, §3, §9.
- **Code:** `mandate-journal`: `crates/mandate-journal/src/draft.rs` (envelope validation),
  `crates/mandate-journal/src/catalogue.rs` (stream types, required `config_refs`),
  `crates/mandate-journal/src/schema.rs` (field types and payload schemas; a schema is registered
  by the story that first emits the event).
- **Tests:** `crates/mandate-journal/tests/append.rs`, `crates/mandate-journal/tests/catalogue.rs`
  (the spec table written out again).
- **Run:** `cargo nextest run -p mandate-journal`.

## Production configuration references (E7-19)

- **Spec:** `docs/specs/journal.md` §9's `config_refs` table and its append-time reference check
  for `policy_set` and `model_registry` — the object is present, canonical, and carries the
  expected top-level `kind`, and nothing beyond that; DEC-484.
- **Code:** `crates/mandate-journal/src/lib.rs` (`validate_config_artifacts` and
  `referenced_config_object`, reached through `config_artifact_refusal` by
  `MemoryJournal::append_with_config_artifacts` and `PgJournal::append_with_config_artifacts`),
  `crates/mandate-journal/src/draft.rs` (`Draft::config_artifact_path`, the version-2 records a
  plain append cannot check, named so both stores fail closed the same way),
  `crates/mandate-journal-pg/src/lib.rs` (`missing_config_artifact`, applied after §5.1 step 1's
  idempotency so an identical retry still answers `AlreadyCommitted`).
- **Tests:** `crates/mandate-refcases/tests/production_config_refs.rs` (the four reference tests:
  version-2 agent records and registrations bind their objects, every invalid reference is
  refused as specified, and every new reference requires its stored object),
  `crates/mandate-journal/src/lib.rs`'s `production_config_tests` and
  `crates/mandate-journal-pg/src/lib.rs`'s
  `a_version_two_configuration_draft_passes_validation_and_is_refused_after_idempotency` (the
  plain-append refusal, which no external oracle covers).
  J0 (DEC-510): `crates/mandate-journal-pg/tests/pg_config_artifacts.rs` (the Postgres
  artifact-aware append, `PgJournal::append_with_config_artifacts`, gives `MemoryJournal`'s
  outcomes, heads, and rows; refusals write nothing; §5.1's order; a property against an outcome
  built from each step). Both journals call `mandate_journal::config_artifact_refusal`, one
  predicate (DEC-510 item 4). Each test starts with a database-free property, so without Postgres
  that property still runs.
- **Reference cases:** the `production_config_refs` section of `fixtures/refcases/journal.json`.
- **Not covered here:** DEC-484 item 4's object shape (`policy_set_version`, level order and
  uniqueness, sorted unique models and params) is not checked at append, because the spec's
  append-time check is only absence and a differing top-level `kind`; the validated production
  input owns it, as the E7-19 backlog row says.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-journal-pg -p mandate-refcases -E
  'binary(production_config_refs) | package(mandate-journal) | package(mandate-journal-pg)'`. J0's
  Postgres tests: `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-journal-pg --test
  pg_config_artifacts`.

## Agent-stream payload schemas (E7-9)

- **Spec:** `docs/specs/journal.md` §9.1 (the eight closed schemas, consistency rules 1 to 13,
  subject rules 14 and 15, copy rule 16, rule 10's batch clause) and §11 (`intent_action_mismatch`,
  `mode_event_mismatch`); DEC-168, DEC-177, DEC-252.
- **Code:** `crates/mandate-journal/src/agent.rs` (`Draft::parse` takes a §9.1 schema by the stream
  type, then the rules; `verify_agent_stream` runs the two range checks after `verify_events`).
- **Tests:** `crates/mandate-refcases/tests/agent_stream.rs` (one test per §9.1 family, the batches,
  the range checks, and that every expectation key is read), `crates/mandate-journal/tests/append.rs`
  (`an_agent_stream_opens_with_an_agent_subject_only`), `crates/mandate-journal/tests/catalogue.rs`
  (`a_closed_agent_stream_schema_refuses_an_unlisted_member`).
- **Reference cases:** `journal::agent_stream::*` (the vectors' `agent_stream` section of
  `fixtures/refcases/journal.json`, generated by `reference/journal/generate.py`).
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases --run-ignored all -E
  'binary(agent_stream) | test(/agent_stream/)'`.

## Control-stream payload schemas and the `JournaledFact` mapping (E7-10)

- **Spec:** `docs/specs/journal.md` §9.2 (the control stream's closed schemas, `OwnerCommandRefused`
  on the agent and account streams, the `pointer` and `date` types, consistency rules 17 to 24,
  subject rules 25, 26 and 28, copy rule 27, and the mapping table); DEC-168, DEC-261, DEC-302, DEC-303,
  DEC-304, DEC-402. `AccountSnapshotRecorded` and rule 24 are registered with the executor's fee-step
  writer (DEC-261 item 7, DEC-402).
- **Code:** `crates/mandate-journal/src/control.rs` (`Draft::parse` routes each §9.2 type on its
  stream; `payload`, then `subject_and_copy`), `crates/mandate-journal/src/schema.rs` (`Ty::Pointer`),
  `crates/mandate-spec/src/context.rs` (`JournaledFact::from_record`, the mapping, with its in-module
  `record_tests`; DEC-303 items 10 and 15).
- **Tests:** `crates/mandate-refcases/tests/control_stream.rs` (one test per §9.2 family, the chain,
  and the mapping over the vectors' `journaled_facts`), `crates/mandate-spec/tests/journal_record.rs`
  (the mapping from hand-written records), `crates/mandate-journal/tests/catalogue.rs` (unlisted
  members at every depth; `account_snapshot_recorded_is_routed_to_section_9_2`),
  `control::tests` in `control.rs` (the vectors inside the crate), and
  `mandate-executor`'s `reconcile::tests::the_fee_steps_snapshot_is_never_refused_for_its_members`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-spec -p mandate-refcases -p
  mandate-executor -E 'binary(control_stream) | binary(journal_record) | binary(catalogue) |
  test(/control::tests/) | test(the_fee_steps_snapshot_is_never_refused_for_its_members)'`.

## Account-stream risk-state records (journal spec §9.3, under E7-10)

- **Spec:** `docs/specs/journal.md` §9.3 (`MandateVersionApplied` and `UniverseChanged`, the `asset_id`
  type, rules 29 to 33, and the mapping table); DEC-403, DEC-404. Both are registered, and the mapping re-derives
  §9.2's classification from the two stored documents and refuses a mismatch (DEC-404 items 5 and 8).
- **Code:** `crates/mandate-journal/src/control.rs` (`governs`, `MANDATE_VERSION_APPLIED`,
  `UNIVERSE_CHANGED`, and rules 29 to 33 in `payload`), `crates/mandate-journal/src/catalogue.rs`
  (`UniverseChanged`'s entry), and `crates/mandate-spec/src/context.rs` (`JournaledFact::from_record`
  with `change::classify`).
- **Tests:** `crates/mandate-refcases/tests/risk_state.rs` (the vectors through `append`, the
  mapping, and the classification re-derivation), `crates/mandate-refcases/tests/asset_id.rs` (the
  journal's `asset_id` check and `AssetId::parse` agree over a generated corpus), `control::tests` in `control.rs`
  (`every_risk_state_draft_is_judged_as_its_vectors_say`, `a_required_risk_state_member_is_never_null`,
  `a_risk_state_record_at_another_schema_version_is_an_unknown_schema`,
  `rule_33_refuses_nothing_that_shows_no_raise`),
  `crates/mandate-journal/tests/catalogue.rs` (`risk_state_records_are_routed_to_section_9_3`,
  `risk_state_records_refuse_an_unlisted_member`), and `record_tests` in `context.rs`
  (`a_version_maps_only_under_the_classification_its_documents_give`, over 14 §9.2 rows, and
  `a_version_document_that_is_absent_or_an_impostor_is_refused`).
- **Run:** `cargo nextest run -p mandate-journal -p mandate-spec -p mandate-refcases
  -E 'binary(risk_state) | test(/risk_state/) | test(/universe_change/) |
  test(/version_maps_only/) | test(/impostor/) | test(/schema_version/) | test(/rule_33/)'`.

## Research-agent thesis records (journal spec §9.4, under E17-2)

- **Spec:** `docs/specs/journal.md` §9.4 (`ThesisProposed` and `ThesisRevised`, one shared schema,
  rules 34 to 38, `instrument_id` an `asset_id`); DEC-413, DEC-414. `append` checks the schema and
  rules; the mandate re-derivation of §8.5 checks 4, 5, 6, 10 and 16's cap (DEC-413 item 5) is
  `check_thesis_record`, which every reader of a thesis record runs before acting on it (DEC-414
  item 3).
- **Code:** `crates/mandate-journal/src/control.rs` (`governs`, `THESIS`, `THESIS_RECORD`,
  `thesis_rules`, `horizon_agrees`), `crates/mandate-journal/src/catalogue.rs` (both entries, `man`
  and `mod`), and `crates/mandate-spec/src/context.rs` (`check_thesis_record`,
  `THESIS_MANDATE_CHECKS`, `MANDATE_ALONE_CHECKS`).
- **Tests:** `control::tests` in `control.rs` (`every_thesis_draft_is_judged_as_its_vectors_say`,
  `a_required_thesis_member_is_never_null`,
  `a_thesis_record_at_another_schema_version_is_an_unknown_schema`),
  `crates/mandate-journal/tests/catalogue.rs` (`thesis_records_are_routed_to_section_9_4` and
  `thesis_records_refuse_an_unlisted_member`), and `thesis_tests` in `context.rs` (the six
  re-derivation tests: pinned universe, admission `deny`, asset class, revision cap, the named
  mandate, an absent or impostor document).
- **Run:** `cargo nextest run -p mandate-journal -p mandate-spec -E 'test(/thesis/)'`; the vectors
  themselves with `uv run --directory python pytest -q
  mandate_tools/tests/test_journal_research_vectors.py`.

## Append protocol

- **Spec:** `docs/specs/journal.md` §5.1.
- **Code:** `MemoryJournal::append` and `seal` in `crates/mandate-journal/src/lib.rs`.
- **Tests:** `crates/mandate-journal/tests/properties.rs` (append-only, gapless, fenced,
  idempotent against a model), `crates/mandate-journal/tests/append.rs`.
- **Reference cases:** `journal::chain::*`, `journal::append::*`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.

## Postgres journal

- **Spec:** `docs/specs/journal.md` §5.1, §6.1, §11; ADR-0001 ES-08; DEC-109;
  `docs/project/tasks/E5-3-postgres-journal.md`.
- **Code:** `mandate-journal-pg`: `crates/mandate-journal-pg/src/lib.rs` (`PgJournal`: the append
  protocol in one transaction holding the head row, reads that re-verify stored bytes, `migrator`);
  the schema, roles, and triggers in `migrations/`.
- **Tests:** `crates/mandate-journal/tests/conformance/mod.rs` (the append suite every journal
  backend runs: stream rules, fencing, `risk_clock`, retries, rejections, the chain and append
  vectors, and a differential property against `MemoryJournal`), run by
  `crates/mandate-journal/tests/memory_conformance.rs` and
  `crates/mandate-journal-pg/tests/pg_conformance.rs`; `crates/mandate-journal-pg/tests/pg.rs`
  (migrations, privileges and triggers, database-enforced chain rules, concurrent appenders and
  owners, failed and lost commits, re-verification on read, the tamper vectors replayed on stored
  rows). Setup: `crates/mandate-journal-pg/README.md`.
- **Run:** `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-journal-pg`, or
  `cargo xtask ci postgres`; without `MANDATE_PG_URL` the Postgres tests skip.

## Verification, anchoring, and export

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11.
- **Code:** `crates/mandate-journal/src/verify.rs`, `crates/mandate-journal/src/merkle.rs`,
  `export_line` in `crates/mandate-journal/src/lib.rs`.
- **Tests:** `crates/mandate-journal/tests/verify.rs`, `crates/mandate-journal/tests/properties.rs`
  (any tampering detected; rewrites caught only by the anchor; independent Merkle construction).
- **Reference cases:** `journal::tamper::*`, `journal::merkle`, `journal::export_line_seq_1`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.

## Journal cold store

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11, §12; DEC-263 to DEC-265;
  `docs/project/tasks/E5-6-journal-cold-store.md`.
- **Code:** `mandate-journal-cold`: `crates/mandate-journal-cold/src/lib.rs` (the segment manifest
  and its canonical bytes — refused above the canonical integer bound, `SeqUnrepresentable` —
  `import_line`, `verify_range`'s per-range checks with the both-direction segment chain,
  `tsa_imprint_matches`'s structural scope and the fail-closed `verify_tsa` entry point
  (`TsaVerificationIncomplete` until DEC-265 item 1's crypto half lands), the inclusion proof
  with its `MalformedAnchor` refusal, and the fallible export bundle's digest
  (`PartUnrepresentable`, never an invented value) — implemented, over `mandate-journal`'s own
  `verify_events`, `export_segment`, `Anchor`, `merkle_root` and `tsa_imprint`).
- **Tests:** `crates/mandate-journal-cold/tests/cold.rs` (the manifest's six fields and their
  canonical bytes with an independent oracle, the importer's split, each cold check at its bar,
  the range walk's reachability and its overlapping-segment refusal, the token's imprint
  containment and the entry point's refusal, the proof's sibling roots, the lying anchor, the
  export digest), live since the implementation PR, and
  `crates/mandate-journal-cold/tests/properties.rs` (E5-6a, DEC-287: the range walk's
  invariants over random segment sequences — entered at the genesis, inside the first supplied
  segment, or exactly at any supplied segment's first seq with the earlier segments omitted, a
  stale copy prepended in front of the trusted start included — against an oracle that computes
  the expected outcome from the scenario it built — the verified state, the failing check and
  its seq by DEC-264's order including the per-event arm and the canonical-form rule, the
  reachability rule, and the token's two answers — plus the manifest round trip, the inclusion
  proof's root rebuilt by the oracle's own §10 fold, and the export digest, 256 cases each).
- **Run:** `cargo nextest run -p mandate-journal-cold`.

## Content-addressed artifacts

- **Spec:** `docs/specs/journal.md` §6.3, §11 check 6; DEC-107;
  `docs/project/tasks/E5-2-artifact-store.md`.
- **Code:** `crates/mandate-journal/src/artifact.rs` (`ArtifactRef`, the `ArtifactSource` and
  `ArtifactStore` traits, `get_artifact`, the map store); `mandate-artifacts-fs`:
  `crates/mandate-artifacts-fs/src/lib.rs` (`FsArtifactStore`: write-once objects under
  `sha256/<2 hex>/<hex>`, flushed and hard-linked into place).
- **Tests:** `crates/mandate-journal/tests/artifacts.rs` (SHA-256 vectors, round trip, one address
  per content, flipped bits, no overwrite, verification through the store),
  `crates/mandate-artifacts-fs/tests/fs.rs` (the same on disk, plus layout, permissions, missing
  versus unavailable, reopening, concurrent puts, readers during a write, and crashed writes).
  Planted bugs per test: the task brief.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-artifacts-fs`.

## Journal verification and artifact commands

- **Spec:** `docs/specs/journal.md` §6.2 (the segment format the command reads), §6.3, §10, §11,
  §12; backlog E5-4; DEC-115; `docs/project/tasks/E5-4-verification-cli.md`.
- **Code:** `crates/mandate-cli/src/journal.rs` (`mandate journal verify`: reads a segment file back
  into stored rows, runs `verify_events` then `verify_anchor`, reports the first failure with its
  spec check code and a non-zero exit), `crates/mandate-cli/src/artifact.rs` (`mandate artifact put`
  and `get` over `FsArtifactStore`, the get re-hashed through `get_artifact`). The checks themselves
  are `mandate-journal`'s and `mandate-artifacts-fs`', unchanged by this story.
- **Tests:** `crates/mandate-cli/tests/journal_verify.rs` (every tamper vector an export can
  express, replayed through the command against `fixtures/refcases/journal.json`, with the two it
  cannot asserted as inexpressible; the exact report; malformed lines, trusted starts, anchors,
  mixed streams, artifacts present, absent, and altered), `crates/mandate-cli/tests/artifact.rs`
  (the FIPS 180-2 addresses a put prints, the round trip, the re-hash on read, reference forms).
  Planted bugs per test: the task brief.
- **Reference cases:** `journal::tamper::*` and `journal::export_line_seq_1` in
  `fixtures/refcases/journal.json`, read directly rather than through `mandate-refcases`.
- **Run:** `cargo nextest run -p mandate-cli --test journal_verify --test artifact`.

## Cold-store verification command

- **Spec:** `docs/specs/journal.md` §6.2 (the segment files and manifests the command reads),
  §6.3, §10, §11 (the per-range checks), §12; backlog E5-8; DEC-490 (the command's shape, the
  directory layout and walk order, the refusals, the check order and the report), DEC-263 to
  DEC-265; `docs/project/tasks/E5-8-cold-verify-cli.md`.
- **Code:** `crates/mandate-cli/src/journal/cold.rs` (`mandate journal verify-cold <dir>`: reads
  the directory's `<name>.jsonl` and `<name>.manifest.json` pairs, refuses an incomplete export or
  an unreadable path with a stable code, walks the segments in manifest order through
  `mandate_journal_cold::verify_range`, checks the anchor over the walked rows and the token
  through `verify_tsa`, which never verifies until DEC-265 item 1's crypto half lands, and
  reports the first failure with its code and a non-zero exit); the shared refusals and helpers in
  `crates/mandate-cli/src/journal.rs`.
- **Tests:** `crates/mandate-cli/tests/journal_verify_cold.rs` (every tamper vector a cold export
  can express, replayed against `fixtures/refcases/journal.json`, with the three it re-expresses
  asserted; the segment checks at their `seq`, manifest order, the mid-segment entry, the
  trusted start, the anchor over earlier segments and before the start, the token's two answers
  and its order, the incomplete and unreadable refusals with their codes, artifacts, and the
  exact report); `crates/mandate-cli/tests/binary.rs` (the `mandate`
  binary run as a process: `journal verify` and `journal verify-cold`, each
  printing its report and exiting on the outcome). Planted bugs per test: the task brief.
- **Reference cases:** `journal::tamper::*` in `fixtures/refcases/journal.json`, read directly.
- **Run:** `cargo nextest run -p mandate-cli --test journal_verify_cold --test binary`;
  `cargo xtask ci pending`.

## Mandate document, validation, risk state, and change classification

Planned by [the stream-F task brief](../../../docs/project/tasks/M5-F-mandate-spec.md) and DEC-128.
The crates exist; the rules above `SchemaDec` are stubs until their implementation PRs (DEC-77).

- **Spec:** `docs/specs/mandate.md` §1.1 (MI-1 to MI-20), §2 (lifecycle, provenance, the working
  universe), §3 (structure and goals), §4 (validation, warnings, the policy hierarchy), §5 (the risk
  state, the risk day, breach confirmation, the lifetime floor, restrictions and the effective mode),
  §6.3 (the condition language), §7 (platform defaults and proposals), §9 (the version hash and
  change classification), §11 (the reference cases); `docs/specs/trading-domain.md` §8.1 and §8.2
  (cost-basis reduction, risk marks); `docs/specs/journal.md` §4 (the canonical form the version
  hashes) and §9; ADR-0001 ES-02, ES-04, ES-09, ES-21, ES-22.
- **Code:** `mandate-spec`: `crates/mandate-spec/src/dec.rs` (`SchemaDec`, a decimal checked against
  its field's whole schema `$def` — the pattern and, for `decimal`, the `-0` exclusion — so the text is
  already the journal form and a version hash reproduces the confirmed document),
  `crates/mandate-spec/src/document.rs` (the hashed envelope document and the strict parse),
  `crates/mandate-spec/src/validate.rs` (the V-rules, the warnings, the closed platform-default list,
  `ValidatedMandate`), `crates/mandate-spec/src/policy.rs` (the hierarchy and its runtime overlay),
  `crates/mandate-spec/src/risk.rs` (the risk-state types, breach confirmation, risk days),
  `crates/mandate-spec/src/risk/limits.rs` (the §5.2 and §5.6 comparisons),
  `crates/mandate-spec/src/risk/fold.rs` (the fold over marks, fills, clock ticks, universe changes,
  staleness, a `profit_stop` goal, and the stepwise lift after a reset, slices R2 to R4, DEC-167
  items 5 to 8), `crates/mandate-spec/src/risk/fold/daily.rs` (the daily loss over risk days: the
  rollover, a breach carried over it, the renewal, and the lift),
  `crates/mandate-spec/src/risk/fold/owner.rs` (acknowledgments, allocation changes, floor
  loosening, goal completion, and retirement, slice R4),
  `crates/mandate-spec/src/goal.rs`, `crates/mandate-spec/src/change.rs` (the version and §9.2
  classification), `crates/mandate-spec/src/condition.rs` (the §6.3 language, owned here and nowhere
  else), `crates/mandate-spec/src/context.rs` (`ValidationContext::from_journal`, the fold over
  journaled facts with refusing defaults, DEC-169); `mandate-domain`: `crates/mandate-domain/src/lib.rs` (the vocabulary `mandate-risk`,
  `mandate-builder`, and the research agent share). The exact arithmetic stays in `mandate-num`
  (ES-04).
- **Tests:** `crates/mandate-spec/tests/dec.rs` (live: every grammar's own values, the four things
  `DecStr` normalises pinned as rejections, an integer oracle for the ordering, and the normal-form
  identity the version hash rests on), `crates/mandate-spec/tests/document.rs` (the code and pointer
  each rejection carries), `crates/mandate-spec/tests/validate.rs` (the closed §7 list, the provenance
  rules, the confirmation screen's four figures), `crates/mandate-spec/tests/policy.rs` (the nearest
  broken ancestor, each key kind, the absence asymmetry), `crates/mandate-spec/tests/policy_document.rs`
  (a `policy.schema.json` document and a `policy_set` object read strictly, DEC-484 item 4),
  `crates/mandate-spec/tests/risk_day.rs` (the year tiled without gap or overlap),
  `crates/mandate-spec/tests/goal.rs` (each §3.1 "done when" row, and a `profit_stop` left to the risk
  state), `crates/mandate-spec/tests/risk.rs` (the §5 fold: the ladder and its hysteresis boundary,
  breach confirmation either side of the window, the two-quote hard trigger, the rollover, the daily
  lift and its renewal, acknowledgment and the stepwise lift, the floor and its loosening, the loss
  carry, allocation scaling, session marks and staleness, and eleven properties whose oracles are an
  `i128` accumulator, an interval scan for breach time, and a second reader of the journal),
  `crates/mandate-spec/tests/context.rs` (the context from journaled facts: each field's source and
  refusing default, the base refused without any one required fact, and a property against an oracle
  that reads the journal backwards),
  `crates/mandate-spec/tests/common/mod.rs` (a mandate as a canonical value, built by hand);
  `crates/mandate-spec/tests/change.rs` (§9.2 row by row in both directions, the DEC-121 pinning
  switch as a whole and each way it fails to be one, the autonomy shapes, the literal version-vector
  digest, and four properties: step-up exactly when some path increases risk, the join over changed
  paths against hand-written per-edit classes, an allocation-only change classified by its direction,
  and MI-11 against a first-match evaluator of the test's own; DEC-172),
  `crates/mandate-spec/src/change/tests.rs` (the `not_in` shapes no other test reaches, DEC-172
  item 13); `crates/mandate-spec/src/risk/limits.rs`'s tests (the comparisons against an integer
  oracle, the exact set of limits they read, and the profit stop's level);
  `crates/mandate-spec/src/risk/fold/tests.rs` (the fold's edges: the lift delay's boundary,
  restart, and a clock that over-reports, severe rungs that a receding drawdown does not lift, the
  whole-second monotone risk clock, staleness, the crypto and equity clocks, the floor's carry,
  fills, the daily action, a breach carried over the rollover, the renewal, the daily hard wait
  across midnight, the profit stop, and a lift property whose oracle is a run rule);
  `crates/mandate-spec/src/risk/fold/tests/owner.rs` (slice R4's edges, and two properties: an
  allocation change never triggers or lifts a limit, against the same walk with a tick in its place
  and §5.1 recomputed on `i128` counts; and the ladder is monotone, against the latched rungs
  rebuilt from the journal); `crates/mandate-num/src/sizing.rs`'s tests (`UsdExact::quotient`);
  `crates/mandate-domain/tests/domain.rs` (live). Planted bugs per test: the task brief and the E10-3
  tests and implementation PRs.
- **Reference cases:** `fixtures/refcases/mandate.json` families S, V, P, C, R, T, and L (202 cases),
  through `crates/mandate-refcases/src/mandate.rs`; families G and F are stream G's, through
  `crates/mandate-refcases/src/mandate/risk_gate.rs` (Risk gate, above); families A and B stay
  with stream H and fail as "not interpreted until" their owning story, and family N is stream J's
  (below). A rejection that carries no reason
  fails its case, so the thirty cases expecting `schema_valid: false` cannot pass on a parse that
  refuses everything.
- **Run:** `cargo nextest run -p mandate-spec`, `cargo nextest run -p mandate-domain`, and
  `cargo test -p mandate-refcases -- --include-ignored mandate::`.

## Research-agent contract, admission, and lineages

Planned by [the stream-J task brief](../../../docs/project/tasks/M5-J-research-thin-slice.md) and
DEC-132. The crate is **stubs and pending tests** until the E17-3, E17-7, and E17-9 implementation
PRs land: every entry point returns `ResearchError::Unimplemented`, and `cargo xtask ci pending`
proves each pending test fails on them (DEC-110).

- **Spec:** `docs/specs/mandate.md` §1.1 (MI-15 to MI-20), §2.3 (the working universe at runtime),
  §4.3 (the internal research profile and the keys the overlay supplies), §8.1 (the signal model
  contract and the research agent as its one exception), §8.2 (the output shape and freshness), §8.4
  (the research agent: the thesis fields, the cost cap, correlated flow, the stagger, the thin
  slice), §8.5 (the seventeen ordered admission checks), §8.6 (thesis lifetime and revision
  lineages), §11; `docs/specs/trading-domain.md` §3.2 and §7.1 (the eligibility floor and
  instrument-group claims, whose verdicts arrive as facts), §9.6 (the conduct controls the stagger
  sits inside); `docs/specs/journal.md` §9 (`ThesisProposed`, `ThesisRevised`, `UniverseChanged`);
  `docs/adr/0002-autonomous-ideation-and-retail.md`; ADR-0001 ES-02, ES-09, ES-21.
- **Code:** `mandate-research`: `crates/mandate-research/src/lib.rs` (the thesis and its platform facts as typed data,
  `checks` and `admit` for the §8.5 decision, `fold_theses` for the §8.6 lineages, `expire_theses`
  for the three removals, `stagger_offset`, `stagger_release_at` and `next_proposal_at` for §8.4's
  timing, and the `ResearchEvent` entries the crate produces),
  `crates/mandate-research/src/score.rs` (E17-8's `evaluate`, the pure forward-paper scorer) and
  `crates/mandate-research/src/spec_types.rs` (the narrow stream-F views the first implementation PR
  after F's tests PR deletes). It **never calls a model:** a model output arrives as a typed value,
  and the platform boundary that produces it is E17-2's shell story. The spike that found the shape
  is `python/research_spike/` (E17-0, a spike, not product code).
- **Tests:** `crates/mandate-research/tests/admission.rs` (one case per §8.5 check, plus MC-N01,
  MC-N02, MC-N08, MC-N14, MC-N16, MC-N25 and MC-N26 by hand),
  `crates/mandate-research/tests/lineage.rs` (MC-N17 to MC-N19, MC-N24, MC-N27, MC-N28),
  `crates/mandate-research/tests/expiry.rs` (MC-N20 to MC-N22 and the horizon boundary),
  `crates/mandate-research/tests/stagger.rs` (MC-N23's three offsets and the anchor rules, with the
  least-significant-first reduction oracle),
  `crates/mandate-research/tests/properties.rs` (the four oracles: the universe replayed from the
  emitted events, the failing-check set computed unordered, an independent lineage counter, and the
  oracle self-checks that fail on a seeded bug),
  `crates/mandate-research/tests/score.rs` (E17-8's forward-paper scoring: hand-calculated cases,
  the boundary rules, and the empty scoreable set's named refusal `empty_scoreable_set` at a
  registered minimum of zero, DEC-335 and DEC-336, with its branch-counting property),
  `crates/mandate-research/tests/refcases.rs` (25 of the 28 family-N cases loaded from
  `fixtures/refcases/mandate.json` rather than typed out; the three that state `first_order_autonomy`
  wait for stream H's `classify`), and `crates/mandate-research/tests/rules.rs` (the rule logic this
  crate carries live, pinned unignored so `cargo mutants` reaches it). Planted bugs per test: the task
  brief.
- **Reference cases:** `fixtures/refcases/mandate.json` family N (28 cases: admission, lineage,
  thesis expiry, stagger), through `crates/mandate-refcases/src/mandate/research.rs` in the
  `mandate` suite (DEC-154). All four kinds are interpreted, and MC-N01, MC-N14 and MC-N26 decide
  their first order through `mandate-builder`'s `classify` (DEC-179), so all 28 run; the status rows
  of those three move in a status-only PR. The module's in-module tests doctor every expected member
  of every interpreted case and require it to fail, and read every first-order fact back from its
  own §6.3 field.
- **Run:** `cargo nextest run -p mandate-research` and
  `cargo test -p mandate-refcases -- --include-ignored mandate::MC-N`.

## Approval core: content, admission, re-validation, and step-up (E8-1 to E8-3)

- **Spec:** the [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md); mandate spec §6.1
  (owner controls and step-up), §6.4, and MI-21 to MI-25; journal spec v0.5 §9; DEC-155, DEC-156,
  DEC-158 (option (c)), DEC-165, DEC-173. The MC-E cases follow the harness count correction
  (DEC-173 item 1).
- **Reference model:** the escalation section of `reference/mandate/ref.py`, fuzzed by
  `reference/mandate/fuzz.py` (`fuzz_escalation`, `fuzz_policy_quorum`, `fuzz_drift`,
  `fuzz_ask_budget`, `fuzz_quiet_hours`, `fuzz_owner_controls`, `fuzz_content`) and
  mutation-checked by `reference/mandate/mutants.py`. Check 7's quorum is the stricter of the bound
  requirement and the policy overlay (DEC-173 item 13).
- **Code:** `mandate-approval` (layer 1; E8-1, E8-2 and E8-3 implemented, check 7's policy overlay
  included, DEC-173 item 13):
  `crates/mandate-approval/src/content.rs` (`BoundAction`, `content_object`, `content_hash`,
  `confirmation_code`), `crates/mandate-approval/src/admit.rs` (`admit`: checks 1 to 7;
  `PolicyOverlay`, and `quorum`, the stricter of the bound requirement and the overlay),
  `crates/mandate-approval/src/revalidate.rs` (`revalidate`: checks 8 to 12, `GrantedOrder`),
  `crates/mandate-approval/src/drift.rs`, `crates/mandate-approval/src/budget.rs`,
  `crates/mandate-approval/src/notify.rs` (the closed `Notification`),
  `crates/mandate-approval/src/quiet.rs`, and `crates/mandate-approval/src/stepup.rs`
  (`owner_command`, `kill_switch`, whose answer has no refusing variant, and `kill_switch_code`).
- **Tests:** `crates/mandate-approval/tests/content.rs` (E8-1: the content object, its hash and
  code, the notification payload, quiet hours), `crates/mandate-approval/tests/budget.rs` (E8-2:
  the ask budget and suppressions), and `crates/mandate-approval/tests/properties.rs` (the sentinel
  scanner, the budget counter with its own DST table, and content separation), all live, with
  fixtures in `crates/mandate-approval/tests/common/mod.rs`; for E8-3,
  `crates/mandate-approval/tests/admission.rs` (checks 1 to 7, lateness, step-up, owner commands,
  the kill switch), `crates/mandate-approval/tests/revalidation.rs` (checks 8 to 12 and drift),
  and `crates/mandate-approval/tests/grant_properties.rs` (the check-table, clock-accumulator,
  principal, assertion-ledger, scaled-integer drift, field-comparer and kill-switch oracles), all
  live; and `crates/mandate-approval/tests/quorum.rs` (check 7 against the policy overlay, with
  its own scaled-integer oracle), all live. In-module tests in `src/stepup.rs` probe step-up
  evidence at the clock's extremes against an `i128` oracle, and `src/drift.rs` a drift too large
  to compute. The runtime's approval path (M7 tests PR 3 of 4, DEC-257 items 5 to 12):
  `crates/mandate-runtime/tests/approvals.rs` (the grant, skip, timeout, lateness, re-tail,
  principal, step-up, same-step cancellation, re-validation and drift cases, the owner exit whose
  step-up is refused yet still routed, pause, resume and the owner's kill switch) and
  `crates/mandate-runtime/tests/approval_properties.rs` (the transition-table and causation-walker
  oracles over random scripts), live since #395, with fixtures in
  `crates/mandate-runtime/tests/common/escalation.rs`. The `quorum` record (DEC-488): four hand
  cases in `tests/approvals.rs`, of which the first tests PR carries the two on the live
  one-approver binding and the second carries the two stricter ones with the `rebound_shell` and
  `two_approvers` fixtures, and one property in `tests/approval_properties.rs`, whose oracle
  reads the expected `{required, independent}` off the journaled request and decides which
  responses check 7 judged from the generated answer its `causation` names, and one harness test in
  `crates/mandate-refcases/tests/mandate_lifecycle_harness.rs` (the ten quorum cases whole, their
  `quorum` compared), live since E8-3's implementation: `escalation::answered` writes the member
  through `quorum_applied` (DEC-488), and all twenty-six lifecycle cases pass whole
  (`every_lifecycle_case_passes_whole`). The CLI's owner control (M7 tests PR 4 of 4,
  DEC-257 items 13 to 17): `crates/mandate-cli/src/control.rs` (`ControlJournal`, `Owner`, `Ids`,
  `ControlError`), `crates/mandate-cli/src/approvals.rs` (`list`, `show`, `approve`, `skip`,
  `outcome`, `message`) and `crates/mandate-cli/src/agent.rs` (`code`, `kill_code`, `command`,
  `kill`, `status`), stubbed; `crates/mandate-cli/tests/approvals.rs` and
  `crates/mandate-cli/tests/agent.rs`, pending E8-3 but for one live fixture check, with an
  in-memory journal in `crates/mandate-cli/tests/common/mod.rs`.
- **The `approvals` commands (K1a, DEC-533):** `crates/mandate-cli/src/inbox.rs` (`list`, `show`,
  `approve` and `skip` over P0's journal as D1b's paper owner; the renderers `list_lines`,
  `show_lines`, `granted_lines`, `skipped_line`; `assertion_id`), stubbed, and `main`;
  `crates/mandate-cli/tests/inbox.rs`, pending E8-3 but for the live flags test.
- **Run:** `cargo nextest run -p mandate-approval -p mandate-runtime -p mandate-cli`;
  `cargo xtask ci pending`.

## The CLI's Postgres control journal (E10-16, P0)

- **Spec:** `docs/specs/journal.md` §5.1, §6.1, §11 check 6; the first paper trade brief
  (`docs/project/tasks/first-paper-trade.md`, P0 and X-12); DEC-510, DEC-520.
- **Code:** `crates/mandate-cli/src/postgres.rs` (`JournalArgs`, the `--journal` and `--store`
  options D1, D2 and V0 flatten; `PgControlJournal`, `mandate-journal-pg` behind `ControlJournal`,
  appending through J0's artifact-aware append with the `mandate-artifacts-fs` store; the DSN a
  `SecretString` exposed only to `PgJournal::from_dsn`).
- **Tests:** `crates/mandate-cli/tests/postgres.rs` (the control-stream vectors byte for byte, a
  registration refused until its object is in the store, as `MemoryJournal` answers, and
  ownership, fencing and retries as the CLI tests' journal answers; each starts with a
  database-free DSN property; the options parse and hide the DSN), and the in-module
  `waiting_sleeps_for_the_whole_delay`.
- **`journal export` (V0, DEC-522):** `crates/mandate-cli/src/journal/export.rs` (one stream to
  the §6.2 segment `journal verify` reads, written whole to `<out>.tmp` and published by a hard
  link that never replaces a file), and `main`, synchronous so the journal's runtime is the only
  one; its tests are the four export tests in `crates/mandate-cli/tests/postgres.rs` (the vectors'
  segment, line for line, verified with its store; an existing file and an empty stream refused;
  the binary exporting and verifying; no refusal naming the DSN or creating a file), each running
  without a database until it reaches the journal.
- **Run:** `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-cli --test postgres`, or
  `cargo xtask ci postgres`, which runs `mandate-cli` beside `mandate-journal-pg`.

## The CLI's configuration and model registrations (E10-16, D1)

- **Spec:** `docs/specs/journal.md` §9.2 (`ConfigSnapshotRegistered` versions 1 and 2); the first
  paper trade brief (D1); DEC-504 item 3, DEC-523, DEC-526.
- **Code:** `crates/mandate-cli/src/config.rs` (`register`, which stores any kind's object and
  registers it, holding the instrument snapshot to exactly DEC-523's object; `register_model`,
  whose content and hash come only from `mandate_modelhost::content`; both paper only), stubbed.
- **Tests:** `crates/mandate-cli/tests/config_register.rs`, pending E10-16, each committed draft
  read back through `Draft::parse`.
- **The commands (D1b, DEC-527):** `crates/mandate-cli/src/register.rs` (`OwnerArgs`, the
  required `--workspace` and `--user`, always in paper; `run` and `run_model` over P0's
  `--journal` and `--store`), stubbed, and `main`; `crates/mandate-cli/tests/register_commands.rs`,
  pending E10-16: the flags, the refusals without a database, and the binary against Postgres.
- **`workspace open` (D1c, DEC-527 items 7 and 8):** `crates/mandate-cli/src/workspace.rs` (`open`,
  the control stream's one `StreamOpened` as the `control_services` opener, in paper), stubbed;
  `crates/mandate-cli/tests/workspace_open.rs`, pending E10-16 but for the live flags test.
- **Run:** `cargo nextest run -p mandate-cli --test config_register --test register_commands`;
  `cargo xtask ci pending`; `cargo xtask ci postgres` for the binary test.

## The CLI's mandate version and confirmation (E10-16, D2a)

- **Spec:** `docs/specs/journal.md` §9.2 (`MandateVersionCreated`, `MandateConfirmed`);
  `docs/specs/mandate.md` §2.1, §4.1, §6.1 (`cli_confirm`), §9.1, §10; the first paper trade brief
  (D2a); DEC-505, DEC-523, DEC-530.
- **Code:** `crates/mandate-cli/src/version.rs` (`create`, which stores the canonical document and
  its record and commits `MandateVersionCreated`, every envelope path `user_entered`; `confirm`,
  which takes the code bound to the version, checks every V-rule but V-002 and the registered
  instrument snapshots, and commits `MandateConfirmed`; both paper only).
- **Tests:** `crates/mandate-cli/tests/version.rs`: payloads and records written
  out from the vectors' shapes and read back through `Draft::parse`; the stream folded with
  `JournaledFact::from_record` and `ValidationContext::from_journal`, leaving only V-001 and V-002;
  every refusal code, each writing nothing; a store failing at each write committing nothing. The SPY mandate is
  `crates/mandate-cli/tests/fixtures/spy_mandate.json`.
- **Run:** `cargo nextest run -p mandate-cli --test version`.
- **`agent deploy` (D2b, DEC-530 item 9):** `crates/mandate-cli/src/deploy.rs` (`deploy`, which
  takes the stream's latest confirmed version and a code bound to the agent and the version, and
  commits `AgentDeployed` with `config_refs.mandate_version`; one active deployment per agent),
  stubbed; `crates/mandate-cli/tests/deploy.rs`, pending E10-16, over a control stream seeded in
  §9.2's shapes: the exact payload, record and envelope; the fold reading the agent's version in
  force; every refusal code, each writing nothing; a failing store committing nothing.
  `cargo nextest run -p mandate-cli --test deploy`.

## Reference-case harness

- **Spec:** ADR-0001 ES-11; DEC-77 (pending and passing cases).
- **Code:** `mandate-refcases`: `crates/mandate-refcases/src/journal.rs` (one interpretation per
  prose case), `crates/mandate-refcases/src/trading_domain.rs` (fills, marks, fee charges,
  settlement, corporate actions, dividends, cash-in-lieu postings, the account type, and buying
  power; every other step type and expectation key fails as "not interpreted until" its owning
  story), `crates/mandate-refcases/src/trading_domain/gate.rs` (E6-9's gate driver, DEC-199:
  `propose_order` steps and the `decision` expectation through `mandate_risk::evaluate`, and the
  account's §7.3 status from `initial.account` and `broker_account_update`, `crypto_status` as
  check 1's `crypto_active` with only an exact `ACTIVE` active and a detected inactivity never
  lifted, and a short refused by the day-trade fold (E6-10, DEC-314, DEC-315, DEC-395); its
  in-module tests run RC-15's steps 3 and 4 in a `closing_only` account),
  `crates/mandate-refcases/tests/refcases.rs`, `crates/mandate-refcases/tests/harness.rs`
  (the harness reads the account type and checks `buying_power`: RC-08 and RC-18's cash variant
  without their gate step), `crates/mandate-refcases/tests/trading_domain_gate_harness.rs` (the gate
  driver reads every key it claims, an edited decision fails, and what it cannot read is refused;
  RC-15's `status_not_active` without its later stories' expectations passes; RC-09's crypto
  opening denied `crypto_account_inactive` by an initial or updated `crypto_status`, and never
  lifted by a later `ACTIVE`),
  `crates/mandate-refcases/src/mandate/research.rs` (family N of the
  mandate suite, through `mandate-research`), `crates/mandate-refcases/src/mandate/autonomy.rs`
  (family A, through `mandate-builder`'s `classify`), `crates/mandate-refcases/src/mandate/order_builder.rs`
  (family B, through `mandate-builder`'s `propose` and `decide` and `mandate-risk`'s `evaluate`),
  `crates/mandate-refcases/src/mandate/risk_gate.rs` (families G and F, through `mandate-risk`,
  with `crates/mandate-refcases/tests/mandate_gate_harness.rs`; DEC-178),
  `crates/mandate-refcases/status.toml` (founder-owned).
- **Suites:** `fixtures/refcases/journal.json` (46 cases, all passing),
  `fixtures/refcases/trading-domain.json` (accounting cases from E3-1 and E3-2; the rest
  pending their stories), `fixtures/refcases/mandate.json` (families S, V, P, C, R, T, and L
  harnessed by stream F, N by stream J, A and B by stream H, and G and F by stream G; a case whose
  own story is pending fails naming it).
- **Run:** `cargo nextest run -p mandate-refcases`; pending cases with
  `cargo test -p mandate-refcases -- --include-ignored`; the mutation gate on a harness change,
  `MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants` (DEC-253: the
  crate is `safety_critical = true`, so the gate covers it although it is a `tool` crate).

## Reference-case fixtures and the mandate reference implementation

- **Spec:** `docs/specs/mandate.md`; ADR-0001 ES-10, ES-11.
- **Code:** `python/mandate_tools/src/mandate_tools/export_refcases.py` (YAML to JSON, strict
  loader); `reference/mandate/` (founder-owned reference implementation);
  `python/mandate_tools/src/mandate_tools/mutation_anchors.py` (the anchor-only check: every `old`
  text of every mutant table in `reference/mandate/mutants.py` still occurs in `ref.py`, per PR and
  in under a second, where the nightly's sweep would fail before its first run; DEC-493).
- **Tests:** `python/mandate_tools/tests/test_export_refcases.py`,
  `python/mandate_tools/tests/test_mutation_anchors.py` (passes on the committed reference, names a
  stale anchor, refuses a `mutants.py` with no table); `reference/mandate/fuzz.py`,
  `reference/mandate/check_cases.py`, `reference/mandate/mutants.py`.
- **Run:** `cargo xtask refcases`; `cargo xtask ci reference` (the mandate generator, case check,
  fuzz seeds 1 to 3 and `reference/journal/generate.py --check` in the environment
  `reference/mandate/requirements.txt` pins, then the anchor check from the `python/` workspace);
  `cargo xtask ci nightly` for seeds 1 to 10 and the `mutants.py` sweep.

## Liquidity figures

- **Spec:** trading-domain spec §3.2 item 5 and §9.6; DEC-470 item 4; DEC-471 item 3.
- **Code:** `crates/mandate-liquidity/src/lib.rs` (pure typed inputs and the prior close,
  lower-median 20-session dollar volume, truncated 20-session average volume, and trailing
  five-minute volume of complete bars). `crates/mandate-shell/src/paper/facts.rs` validates the
  stored dataset, parses its close and volume fields, maps broker minute bars, and orchestrates.
- **Tests:** the in-module hand cases in `crates/mandate-liquidity/src/lib.rs`, plus the mapping and
  refusal cases in `crates/mandate-shell/src/paper/facts/tests.rs`.
- **Run:** `cargo nextest run -p mandate-liquidity -p mandate-shell`.

## Historical market data download

- **Spec:** backlog E2-1; ADR-0001 ES-19, ES-23; DEC-88, DEC-89, DEC-90;
  `docs/project/tasks/E2-1-download.md`.
- **Code:** `mandate-marketdata`: `crates/mandate-marketdata/src/number.rs` (raw JSON number text to
  `DecStr` and `Decimal128` units, exact or rejected), `crates/mandate-marketdata/src/timestamp.rs`
  (vendor timestamps, UTC days), `crates/mandate-marketdata/src/model.rs` (datasets, symbols, feeds,
  timeframes, day ranges, bars and trades), `crates/mandate-marketdata/src/alpaca.rs` (request
  paths, page parsing), `crates/mandate-marketdata/src/client.rs` (pagination, retries, backoff),
  `crates/mandate-marketdata/src/http.rs` (the market-data host only, credentials),
  `crates/mandate-marketdata/src/dataset.rs` and
  `crates/mandate-marketdata/src/dataset/partition.rs` (Parquet partitions, manifest,
  compare-before-write), and `crates/mandate-marketdata/src/download.rs` (one dataset over a day
  range).
  `mandate-cli`: `crates/mandate-cli/src/download.rs` (`mandate download`),
  `config/research-basket.toml`.
- **Tests:** `crates/mandate-marketdata/tests/` against recorded responses in
  `crates/mandate-marketdata/tests/fixtures/alpaca/`; `crates/mandate-cli/tests/download.rs`.
- **Run:** `cargo nextest run -p mandate-marketdata -p mandate-cli`; against the data host with the
  paper credentials, `cargo nextest run -p mandate-marketdata --features live-alpaca --test live`
  (without the feature the live test is not built).

## Concurrent dataset writes

- **Spec:** DEC-89 (compare-before-write, never replace a stored partition);
  `docs/project/tasks/marketdata-write-safety.md`.
- **Code:** `Store::put_day` and its write helpers in `crates/mandate-marketdata/src/dataset.rs`
  (advisory lock on the dataset directory, unique temporary names, hard-link publish).
- **Tests:** `crates/mandate-marketdata/tests/concurrent_writes.rs` (threads and processes, leftover
  temporary files).
- **Run:** `cargo nextest run -p mandate-marketdata --test concurrent_writes`.

## Market-data rate limiting

- **Spec:** `docs/project/tasks/marketdata-rate-limit.md` (claim #115); ADR-0001 ES-05, ES-19.
- **Code:** `crates/mandate-marketdata/src/rate.rs` (header budget, in-flight count, token-bucket
  floor, 429 window), `Client::get_with_retry` in `crates/mandate-marketdata/src/client.rs`, and
  `rate_headers` in `crates/mandate-marketdata/src/http.rs`.
- **Tests:** `crates/mandate-marketdata/tests/rate_limit.rs` (scripted responses and fake
  fixed-window and token-bucket hosts on an injected clock).
- **Run:** `cargo nextest run -p mandate-marketdata --test rate_limit --test client`.

## Dataset inspection

- **Spec:** backlog E2-2; trading domain spec §4.1, §4.2; DEC-89;
  `docs/project/tasks/E2-2-inspect.md`; the data-quality warnings of
  `docs/project/tasks/inspect-quality.md`.
- **Code:** `crates/mandate-marketdata/src/inspect.rs` (coverage with closed-day counts, exact
  statistics, gaps between bars, duplicates, the `Quality` warnings, untrusted partitions),
  `read_manifest` in `crates/mandate-marketdata/src/dataset.rs`; `crates/mandate-cli/src/inspect.rs`
  (`mandate inspect`, the text report).
- **Tests:** `crates/mandate-marketdata/tests/inspect.rs` (hand-built datasets through the real
  store; a slot-grid oracle for gaps), `crates/mandate-marketdata/tests/inspect_quality.rs`
  (warnings and inconsistent bars over Alpaca records from the M1 rehearsal),
  `crates/mandate-cli/tests/inspect.rs` (the exact report).
- **Run:** `cargo nextest run -p mandate-marketdata -p mandate-cli inspect`.

## Top-of-book quotes

- **Spec:** backlog E2-3; trading domain spec §2.1, §4.1, §4.2, §8.2; DEC-89, DEC-116;
  `docs/project/tasks/E2-3-quotes.md`.
- **Code:** `Quote`, `Kind::Quotes`, and `Records::Quotes` in
  `crates/mandate-marketdata/src/model.rs`; the quotes request and page parsing in
  `crates/mandate-marketdata/src/alpaca.rs`; the quote columns in
  `crates/mandate-marketdata/src/dataset/partition.rs` and their scales in
  `crates/mandate-marketdata/src/dataset.rs`; the statistics (`Values::Quotes`, `Extent`, `Spread`,
  and `QuoteTotals`) in `crates/mandate-marketdata/src/inspect.rs`; `KindArg::Quotes` in
  `crates/mandate-cli/src/download.rs` and the report lines in
  `crates/mandate-cli/src/inspect.rs`. Paging, retries, and storage are the shared paths in
  `crates/mandate-marketdata/src/client.rs` and `Store::put_day`.
- **Tests:** `crates/mandate-marketdata/tests/quotes.rs` against the recorded
  `stock-quotes-*` and `crypto-quotes-*` scenarios in
  `crates/mandate-marketdata/tests/fixtures/alpaca/`, and the quotes tests of
  `crates/mandate-cli/tests/download.rs` and `crates/mandate-cli/tests/inspect.rs`.
- **Run:** `cargo nextest run -p mandate-marketdata --test quotes`,
  `cargo nextest run -p mandate-cli`.
## Market sessions and corporate actions in market data

- **Spec:** backlog E2-4; trading domain spec §1 principle 2, §2.2, §4.2, §4.3, §4.5, §8.5; DEC-82,
  DEC-89, DEC-91; `docs/project/tasks/E2-4-sessions-and-corporate-actions.md`.
- **Code:** `crates/mandate-time/src/session.rs` and `crates/mandate-time/data/us-equities.calendar`
  (the NYSE calendar 2018 to 2028, four sessions per trading day, and
  `last_completed_regular_session`, the one answer for the shell's dataset check and the E15-13
  model host to share, refusing a clock past the range, DEC-517); `mandate-marketdata`:
  - `crates/mandate-marketdata/src/model/corporate_action.rs`: splits, dividends, other actions,
    and point-in-time adjustment through `mandate_num::SplitRatio::mark`;
  - `crates/mandate-marketdata/src/alpaca.rs` and `client.rs`: `/v1/corporate-actions`;
  - `crates/mandate-marketdata/src/venue.rs` with `data/sip.venue` and `data/iex.venue`: each
    feed's venue hours;
  - `crates/mandate-marketdata/src/actions.rs`: `corporate-actions.json` next to a dataset;
  - `download.rs`: records the actions of the stored span;
  - `inspect.rs`: `classify` and the actions report.

  `mandate-cli`: `crates/mandate-cli/src/inspect.rs` (classes, adjusted prices, and the action
  list in the report) and `crates/mandate-cli/src/download.rs` (the actions line).
- **Tests:**
  - `crates/mandate-time/tests/session.rs`: typed NYSE closure lists and its own DST rule, and a
    hand-written table of last completed sessions;
  - `crates/mandate-marketdata/tests/venue.rs`: its own 2026 schedule oracle;
  - `crates/mandate-marketdata/tests/inspect.rs`: hand-built datasets, and a per-slot oracle
    for stretches;
  - `crates/mandate-marketdata/tests/corporate_actions.rs`, `actions.rs`, and `download.rs`;
  - `crates/mandate-cli/tests/inspect.rs` and `download.rs`: the exact report lines.
- **Run:** `cargo nextest run -p mandate-time -p mandate-marketdata -p mandate-cli`.

## Quant model host (E15-13)

- **Spec:** mandate spec §8.1 and §8.2 (the pin, the content hash, `as_of` as the data cut-off);
  `docs/project/tasks/first-paper-trade.md` ("The model host", slices M0 to M2, FT-4, FT-5);
  DEC-503, DEC-504, DEC-517 (the last completed session), DEC-518 (the content object and when
  its hash is pinned).
- **Code:** `mandate-modelhost`, `crates/mandate-modelhost/` (layer 8, safety-critical, pure):
  `src/lib.rs` (`content`,
  `evaluate`, `Refusal`), `src/ma_crossover.rs` (the model's own host code and the listed sources),
  with the crossover itself in `crates/mandate-backtest/src/strategy/ma_crossover.rs`.
- **Tests:** `crates/mandate-modelhost/tests/host.rs` (the content object against canonical JSON
  written by hand from the files on disk, 1.0.0's content hash pinned as a literal (DEC-518 item 1),
  the output mapping, `as_of` on early closes, weekends and
  a given calendar) and `crates/mandate-modelhost/tests/refusals.rs` (one refusal per failed check
  in the brief's order, each refusal's stable code, FT-4 over all 63 sets of identity changes, the
  check order over every pair of stages, and the signal against an `i128` oracle with ties and a
  clock-independence check), with fixtures in `tests/common/mod.rs`.
- **Run:** `cargo nextest run -p mandate-modelhost`.

## Authentication: OIDC token verification (E9-1)

- **Spec:** [identity spec](../../../docs/specs/identity.md) §6.1, ID-9; the
  [E9-1 brief](../../../docs/project/tasks/E9-1-authn.md); DEC-211, DEC-650, DEC-820.
- **Code:** `mandate-authn`, `crates/mandate-authn/` (layer 1, safety-critical, pure): `src/lib.rs`
  (`Refusal` with its stable codes, `SetupError`, `TokenPart`), `src/oidc.rs` (`IssuerConfig`,
  `TokenKind`, `VerifiedSubject`, `verify`, `CLOCK_SKEW_S`, `MAX_TOKEN_BYTES`), and `src/jwks.rs`
  (`Jwks`, `Algorithm`), implemented (slice A1).
- **Tests:** `crates/mandate-authn/tests/oidc.rs` (each allowed algorithm verifies; `none`, HMAC
  and every other `alg` refused before a key is used, whatever the `kid`, the HMAC key-confusion
  attack with five encodings of the public key included; an ES256-only issuer; a header naming
  its own key or a critical extension; the `kid` and key-type checks, with a second ES256 key so
  a verifier ignoring `kid` is caught; another key's signature and the ES256 signature form; no
  claim read before the signature; the issuer byte for byte against another tenant signing with
  the same keys; each refusal's stable code and `TokenKind`'s nonce-free `Debug`), with the
  in-memory issuer in `tests/common/mod.rs` (keys made in the test, one fixed RS256 test key);
  `crates/mandate-authn/tests/claims.rs` (audience and `azp`, `exp` and `nbf` with the skew, the
  nonce, `email_verified` for invitations, and the validity window against its own oracle) and
  `crates/mandate-authn/tests/shape.rs` (malformed and oversized tokens, strict base64url from
  hand-written text, the key set's rules and weak or foreign keys, the issuer configuration, any
  single-byte change, and the ID-9 canary over every printed type), and
  `crates/mandate-authn/tests/boundaries.rs` (the 60 s skew, `exp` and `nbf` at the edges of the
  clock's range, a numeric `sub`, an empty `kid`, and the key set's exact `Debug`),
  all live.
  Sessions (slice A2, DEC-652): `src/session.rs` (`SessionRecord`, `SessionLimits`,
  `ProviderAnswer`, `EndReason`, `SubjectStanding`, and `admit`, which builds
  `mandate_identity::Session` through the seal), stubbed; `crates/mandate-authn/tests/session_api.rs`
  (a refresh secret's `Debug` and the `SessionRevoked` reason codes, live; the digest-only
  record, pending).
- **Run:** `cargo nextest run -p mandate-authn`.
## Identity: roles, the permission matrix, and the authorization step (E9-2)

- **Spec:** identity spec §3 to §5, §4.2's matrix and its grammar (DEC-641), §4.5's authorization
  step, the context contract (DEC-642), and the refusal codes (DEC-643); ID-2, ID-8, ID-13.
- **Code:** `mandate-identity`, `crates/mandate-identity/` (layer 1, safety-critical, pure):
  `src/lib.rs` (`authorize`, `change_roles`, `Authorized`, `TenantContext`, the sealed `Tenant`
  and `MembershipLookup`, `MembershipQuery`, `Session`, `Membership`, `Refusal`, `ClientScope`,
  `StepUpActionKind`, `StepUpEvidence`) and `src/permission.rs` (one `Permission` per §4.2 row).
  Tests PR: `authorize` and `change_roles` are stubs.
- **Tests:** in the crate, because its session, membership, and lookup types are sealed to it:
  `crates/mandate-identity/src/tests/matrix.rs` (ID-2 and the failed membership read, pending
  E9-2: every role set, membership state, cool-off, session kind, principal kind, permission, and
  scope against §4.2 parsed from `docs/specs/identity.md` at test time by the grammar its doc
  states; the grammar check is live), with the hand-written row map in
  `crates/mandate-identity/src/tests/rows.rs` and the test doubles in
  `crates/mandate-identity/src/tests/mod.rs`; `crates/mandate-identity/src/tests/wire.rs` (each
  refusal's code, and the step-up kinds and client scopes read from workspace API §3.6 and §3.8,
  live); and the `compile_fail` doctests in `src/lib.rs` (a `TenantContext` cannot be built,
  defaulted, or cloned, nor `Tenant` implemented, outside the crate), with their in-crate control.
- **Run:** `cargo nextest run -p mandate-identity --run-ignored all` and
  `cargo test -p mandate-identity --doc`.
- **Seal and test support:** `mandate-identity-seal` (layer 0, safety-critical; the `Seal` token
  and `LookupSeal`, closed by its `allowed_dependents` list in `xtask/layers.toml`, which
  `cargo xtask layers` checks, DEC-642 item 7) and `mandate-identity-testkit` (layer 11, so only
  dev-dependencies reach it, and `dev_only` in `xtask/layers.toml`; `StaticLookup`, `FailingLookup`, `session`, `membership`; DEC-645).
  The ULID text codecs are pending in `crates/mandate-identity/src/tests/ulid.rs`.

## Simulated Robinhood broker (E7-25)

- **Spec:** the Robinhood tool contract (E7-15: "The equity order tools", "Order states") and
  slice S1 of the first live trade brief, both from PR #706; connections spec §6; DEC-124;
  DEC-441 items 11 and 16.
- **Code:** `mandate-rh-sim`, `crates/mandate-rh-sim/` (layer 10, above every product crate, so
  a dev-dependency only; safety-critical, pure):
  `src/lib.rs` (`Sim`, its scripted `Event`s and `Fault`s, the order and request types,
  `SimError`).
- **Tests:** `crates/mandate-rh-sim/tests/rules.rs` (only an agentic account reviews or places;
  account numbers are unique), with fixtures in `tests/common/mod.rs`. Pending E7-25.
- **Run:** `cargo nextest run -p mandate-rh-sim`.

## Research-agent spike (E17-0)

- **Spec:** ADR-0002; mandate spec §8.1 to §8.3 (the thesis shape and the sizing idea);
  `docs/project/tasks/RS-1-research-spike.md`. A spike, not product code.
- **Code:** `python/research_spike/src/research_spike/config.py` (basket, caps, model, data dir),
  `python/research_spike/src/research_spike/alpaca.py` (data host and paper host only),
  `python/research_spike/src/research_spike/openrouter.py` (completions, tokens, cost),
  `python/research_spike/src/research_spike/propose.py` (prompt and strict thesis validation),
  `python/research_spike/src/research_spike/journal.py` (hash-chained JSON Lines, artifacts),
  `python/research_spike/src/research_spike/execute.py` (deterministic sizing, journal before
  submit), `python/research_spike/src/research_spike/score.py` (returns versus SPY, cost per thesis),
  `python/research_spike/src/research_spike/__main__.py` (`run`, `score`, `report`, `status`).
- **Tests:** `python/research_spike/tests/` (validation, sizing against hand-computed values, the
  hash chain, scoring on a synthetic series, recorded market-data fixtures).
- **Run:** `cd python && uv run pytest research_spike`; live against the paper account,
  `uv run python -m research_spike run --dry-run` (see `python/research_spike/README.md`).

## Pending tests fail on the stubs

- **Spec:** DEC-77 (tests PR, implementation PR, status PR), DEC-110, DEC-137; the story playbook
  step 8. Gap 1 is closed only in part: the gate reads a failure's text, not its cause (E1-3).
- **Code:** `pending_problems`, `pending_tests`, `STUB_MARKERS`, `names_a_stub` and
  `generated_pending_markers` in `xtask/src/main.rs` (markers found on tokens in every tracked or
  untracked `.rs` file, run in one nextest `--run-ignored ignored-only`; each failure must carry
  its stub's own report, and a marker the scan cannot attach to a named function is a `markers`
  failure).
- **Tests:** the `xtask` unit tests (markers in comments, doc comments, strings, raw strings, split
  across lines; a marker inside a `macro_rules!` body, on a `$`-named function, or on no named
  function at all; which bodies are stubs; result matching) and a fixture workspace in a temporary
  git repository in
  which committed, uncommitted, and untracked pending tests pass on the stubs or fail away from
  them.
- **Run:** `cargo nextest run -p xtask`; `cargo xtask ci pending`; `cargo xtask ci mutants`.

## Repository automation

- **Code:** `xtask`: `xtask/src/main.rs` (every CI job, including `mutants_outcome`,
  `mutant_verdicts` and `is_stub_function`, which exempt an `Unimplemented` stub body of a crate
  with pending tests, on a missed-mutant exit status, and nothing else, and
  `live_tests_judge_every_mutant` with `listed_mutant_counts`, `live_test_counts` and
  `unjudged_mutants`, which fail a mutated package with no live test to judge its mutants before the
  run, since `cargo mutants` would report every one of them caught, DEC-139; the job takes its
  repository as a parameter, so `Fixture::gated` drives the whole of it and neither it nor the
  pre-flight can be deleted without a test failing; `mutated_crates`, which gates every
  `safety_critical = true` crate whatever its layer, DEC-253),
  `xtask/layers.toml` (crate layers and safety-critical policy), `.cargo/mutants.toml` (approved
  equivalent mutants).
- **CI:** `.github/workflows/ci.yml` (`fast`, `full`), `.github/workflows/nightly.yml`.
- **Run:** `cargo xtask check`.
