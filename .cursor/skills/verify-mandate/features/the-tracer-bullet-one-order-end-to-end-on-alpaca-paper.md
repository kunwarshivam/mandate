# The tracer bullet: one order end to end on Alpaca paper

Planned by [the E7-7 task brief](../../../../docs/project/tasks/E7-7-tracer-bullet.md) and DEC-138; the
tests PR (DEC-157) landed the fail-closed crate, and the consolidated implementation in
[PR #598](https://github.com/kunwarshivam/mandate/pull/598) wired the validated mandate, the
builder, the advisory and binding gates, the runtime, the memory/Postgres journal, the executor,
the Alpaca paper connector, and the startup/restart reconciliation through the real crates. The
E7-7 one-shot assembly — the `mandate-tracer` binary, its command line, its recorded suites,
its file artifacts, its tracer signal path and its `mandate-backtest` dependency — is deleted
under DEC-475 item 6; the shell's core below is what remains, and it now serves the paper
adapter's production door.

- **Spec:** `docs/HLD.md` section 5 (the runtime's components in order, "Durability": the write-ahead
  intent and event-sourced state), 6.B (the decision cycle), 6.D (crash recovery);
  `docs/specs/trading-domain.md` section 5.1 (limit openings in the regular session), 5.7 (the order
  lifecycle), 9.1 (the binding gate), 10 (paper mode), 11 (reconciliation), 12 (the account-stream
  events); `docs/specs/mandate.md` section 5.3, 6, 8; `docs/specs/journal.md` section 5.1 (append,
  idempotency, fencing), 5.2 (write before acting), 8 (replay), 11 (verification); ADR-0001 ES-02,
  ES-06, ES-09, ES-19, ES-20, ES-21, ES-23; backlog E7-7.
- **Code:** `crates/mandate-shell/` (layer 8, safety-critical, impure). `src/stages.rs` holds the
  `Stage` enum (the suite's contract) and one trait per stage; `src/tracer.rs` the effect runner,
  which appends every draft before it hands an intent and lets a submission out only after its
  `OrderSubmitted` committed in the same run; `src/map.rs` the total mappings with no permitting
  arm for any non-answer; `src/envelope.rs` the journal envelope (always `paper`) and the
  deterministic ids; `src/host.rs` the refusal of a configured host;
  `src/control.rs` the deployment input from the control
  stream (E19-11, DEC-505; tests in `crates/mandate-gate-inputs/tests/control.rs`) and the effective
  registrations with the DEC-523 snapshot (tests in `crates/mandate-gate-inputs/tests/registrations.rs`),
  and the policy set and model registry that govern the run, DEC-534 (tests in
  `crates/mandate-gate-inputs/tests/governance.rs`);
  `src/paper.rs` the DEC-466 one-run loader that verifies the reviewed artifacts and binds the
  bytes it checked (`Artifacts::from_registered` takes them from the confirmed version and the
  registered objects instead of files, Q1; tests in `crates/mandate-shell/tests/registered.rs`), reads the GET-only broker preflight (including the trailing window's
  IEX minute bars, DEC-471) and the liquidity facts into one `PaperFacts` snapshot, refuses any
  missing, stale, or ambiguous fact or a non-clean account, and assembles the trusted run and
  executor contexts from that snapshot alone (DEC-470); `src/adapters.rs` the production adapters:
  `AlpacaConnector`, `RiskExitPath`, the validated mandate, builder, gate, journal, executor, and
  reconciler, and the trusted daily bars and typed closes the paper assembly reads
  (`trusted_daily_bars`, `trusted_daily_closes`). The risk exit plans over the journal, stream, clock, and agent the run's bridge hands
  it (DEC-449, amended by DEC-451). It holds no trading logic—no sizing, gating, pricing, state
  machine, or arithmetic on money or quantity—and binds the owning crates directly. E7-7's
  `mandate-tracer` binary, its command line, and its recorded tests are deleted (DEC-475 item 6):
  `mandate-paper` is the one binary that places an order, and the paper-adapter feature file covers
  its door.
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
  `src/adapters.rs`'s cases for the stored-data trust rule (read oldest first, the session bound,
  the manifest-first read, and each untrusted dataset) and the purpose-flag and ownerless-exit
  mappings of the flatten adapter; `tests/flatten_probe.rs` the
  flatten adapter's nine pins over a self-proving §9.5 journal (DEC-449, DEC-451);
  `tests/overlay.rs` the policy overlay of mandate spec §6.2 step 5c over the production cycle under
  a real governance fold (DEC-534 item 2, DEC-536, `AGENTS.md` rule 13): every exit stays `auto` by
  the built-in risk-reducing step and is handed on under a nonconforming mandate, an opening and an
  increase are denied `policy_overlay`, a deny the rules decided keeps its rule label, and an
  ungoverned or conforming run decides as the rules alone. E7-7's recorded
  end-to-end suites are deleted with the binary (DEC-475 item 6): `tests/tracer.rs`'s production
  path over recorded Alpaca paper responses (whose `outlier_close` pending case went with the file
  and its `xtask/behaviour-only` row), `tests/paper.rs`'s shipping assembly, and
  `tests/binary.rs`'s binary order — the cycle, its journal order, its restart, and the observation
  and output door are pinned by `crates/mandate-shell/tests/observed.rs` and `tests/watch.rs`, and
  the shipping assembly end to end — the preflight's refusal order, the credential boundary, the
  one submission and its watch — by the paper adapter's feature file. The mandate
  fixtures are generated and checked against `reference/mandate/ref.py` by
  `tests/fixtures/tracer/generate.py`. The paper assembly is split by step under `src/paper/`
  (`artifacts`, `facts`, `judge`, `gate`, `context`) and computes no money or quantity: the
  liquidity figures are `mandate-liquidity`'s, the bracket prices and 1× buying power the
  executor's, the classification facts the builder's `buy_action`, and the fee reservation
  `mandate-accounting`'s (DEC-471 item 3). `src/paper/tests.rs` covers the artifact members and
  values, each single-fact refusal and its boundary, the session and close window, the mapping of
  the protection prices and the builder's action, and the per-request fee reservation;
  `src/paper/facts/tests.rs` the liquidity mapping and each refusal it names. The real HTTP
  runtime's I/O driver is tested with a local socket and no test holds paper credentials or can
  reach Alpaca (ES-19).
- **Reference cases:** none move, and `crates/mandate-refcases/status.toml` is untouched by every PR of
  this stream. The tracer cites `trading_domain::RC-04`, `RC-09`, `RC-09B`, `RC-11`, `RC-14`, `RC-16`,
  `RC-17`, the mandate gate and autonomy families, and the journal append vectors read-only.
- **Run:** `cargo nextest run -p mandate-gate-inputs -p mandate-shell`; `cargo xtask ci pending` for the pending cases.
  The manual paper run is `mandate-paper`'s, not the shell's own command (DEC-475 items 1 and 6);
  see the paper adapter's feature file and the first paper trade brief's E2.
