# The tracer bullet: one order end to end on Alpaca paper

Planned by [the E7-7 task brief](../../../../docs/project/tasks/E7-7-tracer-bullet.md) and DEC-138; the
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
