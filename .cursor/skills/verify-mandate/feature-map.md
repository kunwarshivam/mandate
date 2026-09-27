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
  equity, buying power).
- **Tests:** `crates/mandate-accounting/tests/hand.rs` (hand-calculated cases, partial fills, flips),
  `crates/mandate-accounting/tests/properties.rs` (one property per invariant against an i128
  ledger oracle), `crates/mandate-accounting/tests/corporate_actions.rs` (hand-calculated splits,
  dividends, and cash in lieu), `crates/mandate-accounting/tests/corporate_action_properties.rs`
  (I1, I3, I5, I6 and the split, mark, dividend, and receivable rules against an i128 oracle),
  `crates/mandate-accounting/tests/settlement.rs` (hand-calculated buying power in cash and margin
  accounts, pending charges, reservations), `crates/mandate-accounting/tests/settlement_properties.rs`
  (buying power and the no-debit rule I4 against an i128 oracle whose holidays come from the
  `us_2026` calendar fixture, with a gated generator and a live guard on the branches it reaches).
- **Reference cases:** `trading_domain::*` in `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-accounting`.

## Backtest fill model

- **Spec:** `docs/specs/trading-domain.md` §6.4 (the fill model, rules 1 to 9), §2.1 (backtest fill
  prices are not tick-rounded), §4.1 to §4.4 (bars, data profiles, sessions, halts), §8.2 (a
  backtest mark is the bar close); DEC-106 records the choices §6.4 leaves open.
- **Code:** `mandate-sim`: `crates/mandate-sim/src/lib.rs` (bars with their session labels, orders,
  configuration, fills, end states, errors), `crates/mandate-sim/src/fill.rs` (the pure walk over
  bars: timing, sessions, the shared volume cap, market, limit, stop, stop-limit, and OCO rules);
  the arithmetic is `mandate-num`'s (`Fraction`, `Qty::portion`, `Price::slipped`,
  `Bps::sqrt_impact`).
- **Tests:** `crates/mandate-sim/tests/hand.rs` (every order of RC-10, RC-12, and RC-19 recomputed
  by hand, plus the rules those cases do not reach: latency, day-order cancellation, the shared cap,
  the 20-session median, `sqrt` impact, adverse rounding, crypto),
  `crates/mandate-sim/tests/properties.rs` (one property per rule and per never-or-always clause,
  against an order-major `i128` simulator as the oracle), `crates/mandate-num/tests/num.rs` (the
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
  division-free moving-average crossover and the buy-and-hold benchmark),
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
DEC-130; the paths arrive with the tests PR, which updates this entry.

- **Spec:** `docs/specs/mandate.md` §6.1 to §6.4 (purposes, the evaluation order, the condition
  language, the approver count and the skip-on-timeout), §8.1 to §8.3 (the signal-model contract,
  output freshness, and `conviction_linear`), §3.1 (`accumulate` and `profit_stop`), §5.2 and §5.3
  (exact comparisons and the limits the proposal is clipped to); `docs/specs/trading-domain.md` §5.1
  and §5.3 (the v1 order policy), §8.2 (the risk mark), §9.1 and §9.6 (the gate's verdicts and the
  pacing of a discretionary exit).
- **Code:** `mandate-builder` (new; the §6 classification with its condition language, and the §8.3
  combine, decide, size, and clip walk), with the exact arithmetic (a unit-interval type, a signed
  conviction, the weighted 12-place quotients, and the wide sizing intermediates) in `mandate-num`.
  The gate's dry-run verdict reaches the crate as a value, so `mandate-risk` is not a dependency
  (DEC-130 item 2).
- **Tests:** the hand-calculated A and B case recomputations of the brief, the freshness and
  tie-breaking cases, the four clips and the accumulate clips, and property tests against an
  independent integer combine oracle, a rational sizing oracle compared by cross-multiplication, and
  a naive rule walk. Planted bugs per test: the task brief.
- **Reference cases:** the 16 `mandate::MC-A` cases and the 28 `mandate::MC-B` builder cases other
  than `MC-B17` and `MC-B30` to `MC-B32`, in `fixtures/refcases/mandate.json`. They move in a
  harness-and-status PR after stream F's tests PR adds the `mandate` harness module and, for the `B`
  family, stream G's gate supplies the `gate_dry_run` verdict each of those cases states.
- **Run:** `cargo nextest run -p mandate-builder`.

## Agent runtime and kill switches

Planned by [the E6-1 and E6-5 task brief](../../../docs/project/tasks/E6-1-agent-runtime-and-kill-switches.md)
and DEC-131. The crate holds stubs until the implementation PR; the 88 tests below are pending and
every one fails on those stubs (`cargo xtask ci pending`, DEC-110).

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
  `crates/mandate-runtime/src/error.rs` (`RuntimeError` with a stable `code()` per variant). Over
  `mandate-journal`'s drafts and append protocol unchanged. The shell (tokio, the
  Postgres `LISTEN`/`NOTIFY` tail) is an M6 crate and is not here.
- **Tests:** `crates/mandate-runtime/tests/hand.rs` (62 hand cases: the fold's sequencing and loud
  refusals, the risk clock and deadlines, derived ids and fencing, modes and restrictions, decisions,
  approvals, version application, recovery, and the kill switches),
  `crates/mandate-runtime/tests/properties.rs` (26 properties against three oracles that share no
  code with the crate: a shadow fold rebuilt from the emitted drafts' payloads, a separately written
  restriction lattice, and an interval accumulator for durations),
  `crates/mandate-runtime/tests/common/mod.rs` (the in-memory shell, which can put an append in doubt,
  fence a writer, and crash and restart), and `crates/mandate-runtime/tests/golden-journal.json` (the
  committed fold output that pins `FOLD_VERSION`). Planted bugs per test: the task brief.
- **Reference cases:** none move. `trading_domain::RC-14`'s `kill_switch` variant also needs E7-2's
  `actions` and E6-9's `agent_mode`; the mandate suite's flatten family MC-F01 to MC-F04 belongs to
  `mandate-risk`.
- **Run:** `cargo nextest run -p mandate-runtime` (and, while the tests are pending,
  `cargo nextest run -p mandate-runtime --run-ignored ignored-only --no-fail-fast`).

## Idempotent executor and broker connector

Planned by [the E7-2, E7-3 and E7-4 task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md)
and DEC-133; the paths arrive with the tests PR, which updates this entry.

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
- **Code:** `mandate-executor` (new; `fold` and `handle` over the account stream, the intent protocol,
  `ClientOrderId` with three derivations and no free constructor, the section 5.7 order state machine,
  reservations released by the whole terminal set, the protective sequences and the exit ladder,
  reconciliation whose adoption is scoped to the order set, and the `BrokerRequest` enum whose
  account-wide variants need an `AccountWideScope`, plus the `BrokerConnector` trait; an intent enters as
  `Input::Intent` and the adapter implementing stream I's `IntentSink` lives in the layer-7 shell, since
  the two crates share a layer) and `mandate-alpaca` (new; the paper
  trading client behind an injected transport and clock, the endpoint allowlist, `secrecy`-held
  credentials from an injected lookup, raw-text numbers into `mandate-num`, and the broker status and
  reject mappings). It calls `mandate-risk` directly as the binding gate and reads
  `mandate-accounting` and `mandate-journal` unchanged. The shell that binds runtime, executor, and
  connector is not here.
- **Tests:** the hand cases of the brief (the submission chain, the `Unknown` lookup discipline, the
  status mapping, the protective and kill-switch sequences, the ladder, the restriction table, error
  codes), twelve `fault::crash_at_*` cases at the enumerated submission steps, and property tests
  against four independent oracles: a broker-side submission counter inside the fake connector, a
  shadow order book rebuilt from the drafts' canonical bytes, an `i128` shadow position ledger, and a
  protection accountant that finds every unprotected interval. Alpaca fixtures follow
  `mandate-marketdata`'s recorded-scenario shape adapted for a write API (method and body in
  `requests.txt`, `response-N.json`, its own `record.sh`) and are hand-built; a recording pass against
  the paper host is an addition on top. Planted bugs per test (21): the task brief.
- **Reference cases:** none move in the tests PR. The harness steps and keys this stream owns are
  `broker_order_update` and `orders` (E7-2), `reconciliation` and `broker_position_update` (E7-3), and
  `corporate_action_prepare`, `actions`, `protective_sell_qty` and `initial.open_orders` (E7-4); they
  move in a status PR after the implementation, turning `trading_domain::RC-14` and its four variants,
  `RC-04`, `RC-06`'s `protective_orders_kept_through_dividend`, `RC-07`, `RC-11`, `RC-20`, `RC-21` and
  `RC-24` green.
- **Run:** `cargo nextest run -p mandate-executor -p mandate-alpaca`.

## Risk gate

Planned by [the E6-3 task brief](../../../docs/project/tasks/E6-3-risk-gate.md) and DEC-129; the
paths arrive with the tests PR, which updates this entry.

- **Spec:** `docs/specs/trading-domain.md` §9 (§9.1 the evaluation order and reason codes,
  §9.2 the day-trading regime, §9.3 leverage and short sales, §9.4 sessions, §9.5
  buying power, §9.6 market-conduct controls), §3.1 to §3.3 (instrument fields, the
  eligibility floor, concentration), §4.3 and §4.4 (sessions, auction windows, halts),
  §5.1 to §5.6 (the v1 order policy, the constraints before submission, the kill switch,
  exit pricing), §7.2 to §7.4 (buying power, account restrictions, agent modes), §8.2
  (risk marks); `docs/specs/mandate.md` §1.1 (MI-1 to MI-20), §2.3 (the working universe),
  §5.3, §5.5, §5.9; backlog E6-3, E6-4, E6-6 to E6-9.
- **Code:** `mandate-risk` (new; the pure gate over a mandate, a risk state, an account snapshot, a
  working universe, a market context and one proposed order, returning allow, deny, defer or hold
  with the first failing check's stable code and the whole check list for the journal; the
  §9.2 day-trade ledger; the drawdown ladder's size factor and trim proposals; the
  agent-scoped flatten plan), with the exact arithmetic added to `mandate-num`. It reads
  `mandate-accounting`'s account figures and `mandate-time`'s calendar and sessions, and changes
  neither.
- **Tests:** the hand-calculated cases of the brief's reference-case table, one property per
  invariant and per "never" or "always" in trading-domain spec §9 against an independent
  `i128` oracle, and the E6-3 fuzz over random mandates, market paths and proposal sequences whose
  shadow ledger is accumulated separately from the gate's own figures. Planted bugs per test: the
  task brief.
- **Reference cases:** `mandate::MC-G01` to `MC-G16` and `MC-F01` to `MC-F04` in
  `fixtures/refcases/mandate.json`; `trading_domain::RC-09`, `RC-09B`, `RC-15`, `RC-16`, `RC-22`
  and `RC-25` with their variants, and the `propose_order` steps of `RC-03`, `RC-08` and `RC-18`,
  in `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-risk`.

## Journal drafts and the event catalogue

- **Spec:** `docs/specs/journal.md` §2, §3, §9.
- **Code:** `mandate-journal`: `crates/mandate-journal/src/draft.rs` (envelope validation),
  `crates/mandate-journal/src/catalogue.rs` (stream types, required `config_refs`),
  `crates/mandate-journal/src/schema.rs` (field types and payload schemas; a schema is registered
  by the story that first emits the event).
- **Tests:** `crates/mandate-journal/tests/append.rs`, `crates/mandate-journal/tests/catalogue.rs`
  (the spec table written out again).
- **Run:** `cargo nextest run -p mandate-journal`.

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
  `crates/mandate-spec/src/risk.rs` (the risk-state fold, breach confirmation, risk days),
  `crates/mandate-spec/src/goal.rs`, `crates/mandate-spec/src/change.rs` (the version and §9.2
  classification), `crates/mandate-spec/src/condition.rs` (the §6.3 language, owned here and nowhere
  else); `mandate-domain`: `crates/mandate-domain/src/lib.rs` (the vocabulary `mandate-risk`,
  `mandate-builder`, and the research agent share). The exact arithmetic stays in `mandate-num`
  (ES-04).
- **Tests:** `crates/mandate-spec/tests/dec.rs` (live: every grammar's own values, the four things
  `DecStr` normalises pinned as rejections, an integer oracle for the ordering, and the normal-form
  identity the version hash rests on), `crates/mandate-spec/tests/document.rs` (the code and pointer
  each rejection carries), `crates/mandate-spec/tests/validate.rs` (the closed §7 list, the provenance
  rules, the confirmation screen's four figures), `crates/mandate-spec/tests/policy.rs` (the nearest
  broken ancestor, each key kind, the absence asymmetry),
  `crates/mandate-spec/tests/risk.rs` (§5.2's evaluation order, the two-quote hard trigger, the
  rollover, the floor's three refusals, the loss carry under a withdrawal),
  `crates/mandate-spec/tests/risk_day.rs` (every day of a year against `new_york_midnight`),
  `crates/mandate-spec/tests/goal.rs` (one test per row of §3.1's table and per `on_complete`),
  `crates/mandate-spec/tests/properties.rs` (the risk-state, risk-day, and goal invariants, each with
  an oracle that computes its answer a second way: the journal alone, an interval accumulator, the
  calendar, or a second run of the same market),
  `crates/mandate-spec/tests/common/mod.rs` (a mandate as a canonical value, built by hand, plus the
  two session clocks §5.2 counts in); `crates/mandate-refcases/tests/mandate_harness.rs` (live: the
  DEC-85 sweeps over case keys, expectations, inputs, and journalled members);
  `crates/mandate-domain/tests/domain.rs` (live). The classification tests and their oracles arrive
  with tests PR 3. Planted bugs per test: the task brief.
- **Reference cases:** `fixtures/refcases/mandate.json` families S, V, P, C, R, T, and L (202 cases),
  through `crates/mandate-refcases/src/mandate.rs`; families G, A, B, and N stay with streams G, H,
  and J and fail as "not interpreted until" their owning story. A rejection that carries no reason
  fails its case, so the thirty cases expecting `schema_valid: false` cannot pass on a parse that
  refuses everything.
- **Run:** `cargo nextest run -p mandate-spec`, `cargo nextest run -p mandate-domain`, and
  `cargo test -p mandate-refcases -- --include-ignored mandate::`.

## Research-agent contract, admission, and lineages

Planned by [the stream-J task brief](../../../docs/project/tasks/M5-J-research-thin-slice.md) and
DEC-132; the paths arrive with the tests PR, which updates this entry.

- **Spec:** `docs/specs/mandate.md` §1.1 (MI-15 to MI-20), §2.3 (the working universe at runtime),
  §4.3 (the internal research profile and the keys the overlay supplies), §8.1 (the signal model
  contract and the research agent as its one exception), §8.2 (the output shape and freshness), §8.4
  (the research agent: the thesis fields, the cost cap, correlated flow, the stagger, the thin
  slice), §8.5 (the seventeen ordered admission checks), §8.6 (thesis lifetime and revision
  lineages), §11; `docs/specs/trading-domain.md` §3.2 and §7.1 (the eligibility floor and
  instrument-group claims, whose verdicts arrive as facts), §9.6 (the conduct controls the stagger
  sits inside); `docs/specs/journal.md` §9 (`ThesisProposed`, `ThesisRevised`, `UniverseChanged`);
  `docs/adr/0002-autonomous-ideation-and-retail.md`; ADR-0001 ES-02, ES-09, ES-21.
- **Code:** `mandate-research` (new; the thesis as typed data, the §8.5 checks as one pure function,
  the §8.6 lineage fold with the revision cap and retirement, thesis expiry, the deterministic §8.4
  stagger offset hashed through `mandate-canon`, and the three journal entries the crate produces).
  It **never calls a model:** a model output arrives as a typed value, and the platform boundary that
  produces it is E17-2's shell story. The spike that found the shape is
  `python/research_spike/` (E17-0, a spike, not product code).
- **Tests:** per the brief: one named test per §8.5 check and per removal reason, the family-N
  figures recomputed by hand, and property tests whose oracles rebuild the working universe from the
  emitted `UniverseChanged` entries, compute the failing-check set unordered and take its minimum,
  count lineage revisions in their own accumulator, and reduce the stagger digest
  least-significant-first. Planted bugs per test: the task brief.
- **Reference cases:** `fixtures/refcases/mandate.json` family N (28 cases: admission, lineage,
  thesis expiry, stagger), through the `mandate` suite in `mandate-refcases`; the other families stay
  with streams F, G, and H.
- **Run:** `cargo nextest run -p mandate-research` and
  `cargo test -p mandate-refcases -- --include-ignored mandate::MC-N`.

## Reference-case harness

- **Spec:** ADR-0001 ES-11; DEC-77 (pending and passing cases).
- **Code:** `mandate-refcases`: `crates/mandate-refcases/src/journal.rs` (one interpretation per
  prose case), `crates/mandate-refcases/src/trading_domain.rs` (fills, marks, fee charges,
  settlement, corporate actions, dividends, cash-in-lieu postings, the account type, and buying
  power; every other step type and expectation key fails as "not interpreted until" its owning
  story), `crates/mandate-refcases/tests/refcases.rs`, `crates/mandate-refcases/tests/harness.rs`
  (the harness reads the account type and checks `buying_power`: RC-08 and RC-18's cash variant
  without their gate step), `crates/mandate-refcases/status.toml` (founder-owned).
- **Suites:** `fixtures/refcases/journal.json` (46 cases, all passing),
  `fixtures/refcases/trading-domain.json` (accounting cases from E3-1 and E3-2; the rest
  pending their stories), `fixtures/refcases/mandate.json` (families S, V, P, C, R, T, and L harnessed by stream F; the rest pending their streams).
- **Run:** `cargo nextest run -p mandate-refcases`; pending cases with
  `cargo test -p mandate-refcases -- --include-ignored`.

## Reference-case fixtures and the mandate reference implementation

- **Spec:** `docs/specs/mandate.md`; ADR-0001 ES-10, ES-11.
- **Code:** `python/mandate_tools/src/mandate_tools/export_refcases.py` (YAML to JSON, strict
  loader); `reference/mandate/` (founder-owned reference implementation).
- **Tests:** `python/mandate_tools/tests/test_export_refcases.py`; `reference/mandate/fuzz.py`,
  `reference/mandate/check_cases.py`, `reference/mandate/mutants.py`.
- **Run:** `cargo xtask refcases`, `cargo xtask ci reference`.

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
  compare-before-write), `crates/mandate-marketdata/src/download.rs` (one dataset over a day range).
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
  (the NYSE calendar 2018 to 2028, four sessions per trading day); `mandate-marketdata`:
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
  - `crates/mandate-time/tests/session.rs`: typed NYSE closure lists and its own DST rule;
  - `crates/mandate-marketdata/tests/venue.rs`: its own 2026 schedule oracle;
  - `crates/mandate-marketdata/tests/inspect.rs`: hand-built datasets, and a per-slot oracle
    for stretches;
  - `crates/mandate-marketdata/tests/corporate_actions.rs`, `actions.rs`, and `download.rs`;
  - `crates/mandate-cli/tests/inspect.rs` and `download.rs`: the exact report lines.
- **Run:** `cargo nextest run -p mandate-time -p mandate-marketdata -p mandate-cli`.

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

- **Spec:** DEC-77 (tests PR, implementation PR, status PR), DEC-110; the story playbook step 8.
- **Code:** `pending_problems` and `pending_tests` in `xtask/src/main.rs` (markers found on tokens
  in every tracked or untracked `.rs` file, run in one nextest `--run-ignored ignored-only`).
- **Tests:** the `xtask` unit tests (markers in comments, doc comments, strings, raw strings, split
  across lines; result matching) and a fixture workspace in a temporary git repository in which
  committed, uncommitted, and untracked pending tests pass on the stubs.
- **Run:** `cargo nextest run -p xtask`; `cargo xtask ci pending`.

## Repository automation

- **Code:** `xtask`: `xtask/src/main.rs` (every CI job), `xtask/layers.toml` (crate layers and
  safety-critical policy), `.cargo/mutants.toml` (approved equivalent mutants).
- **CI:** `.github/workflows/ci.yml` (`fast`, `full`), `.github/workflows/nightly.yml`.
- **Run:** `cargo xtask check`.
