# The runtime process: one agent deployment's runtime as a long-running process (E21-30 slice 1)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.1, §3.4, §3.6;
  [journal spec](../../../../docs/specs/journal.md) §5.1 (writer epochs), §9 rule 12; ADR-0001
  ES-06, ES-20; DEC-17, DEC-914, DEC-968, DEC-994, DEC-1012, DEC-1013, DEC-1014, DEC-1015, DEC-1016,
  DEC-2208, DEC-2210; trading spec §11 (the startup hold).
- **Code:** `mandate-daemon` (`crates/mandate-daemon`): `src/runtime.rs` (`RuntimeProcess`: one
  writer epoch on the agent stream, the control stream read before the account stream and the
  account stream before the clock stream, the account stream the connection's establishment names
  folded before the start and on each pass, the research driver run after them behind a
  deny-all default (`src/runtime/research.rs`, DEC-2208), a batch in
  doubt offered again as the same bytes, `mandate-process`'s lifecycle), `src/wake.rs` (`Wake`, a
  hint that only shortens a wait), `src/fixture.rs` (the binary's deny-all wiring until the post-R0
  hook), `src/launch.rs` (the deployment from `run`'s flags or the deployment manager's variables,
  DEC-1015), `src/deployment.rs` (the connection, approvers and author from the latest
  `AgentDeployed` and its stored mandate document, DEC-1016; the approvers through
  `mandate_identity::approvers`, DEC-2388 item 11, and `ApproverSource`, which each control pass
  narrows the view's approvers through, DEC-2818), and the `mandate-daemon` binary, built by `mandate-runtime-models` (`crates/mandate-runtime-models/src/main.rs`) since DEC-2383 item 7 (`run` over Postgres, or `--fixture` over an in-memory
  journal, refused `fixture_under_manager` on a pod the manager started, and `probe` for `exec`
  probes; SIGTERM drains). In the cell: the `runtime` image
  (`deploy/cell/images/runtime.Dockerfile`), its `args` and `probe` in
  `deploy/cell/charts/workspace/processes.json`, and `cargo xtask cell up` building it (DEC-1013). `mandate-store` (`crates/mandate-store`): the `Store` port and the shared envelope.
  `mandate-runtime`'s `src/tail.rs` (`tailed`, shared with the shell). `src/recorded.rs` (the cell's
  recorded research bundle: the entry, its endpoint, the caps, the one recorded reply, and a data
  plane stamped at ages before each read's cut-off, DEC-2210).
- **Tests:** `crates/mandate-daemon/tests/runtime.rs`: a deferring opening committed through
  the journal's append as `DecisionMade` version 3 and asked, never proposed (DEC-2388 item 13);
  the owner's kill switch journaled on the
  agent stream, the control stream read before the clock stream, an unreadable clock stream never
  holding the kill switch, a control stream handed again writing nothing twice, a lost answer
  written once, a hint deciding nothing, a second process fencing the first, paper only, drain, a
  draft without references stopping the process, a connection revoked while running leaving no
  approver and a grant in the same pass refused `not_an_approver` (DEC-2818), a property test over interleavings of kill
  switches, ticks, lost answers, and restarts with an oracle from the control rows; and for the
  account stream, the startup hold lifting on a clean reconciliation, the account folded before the
  start, the latest establishment followed and another connection's or environment's ignored, an
  unreadable account holding neither the kill switch nor the tick, an unfoldable record, another environment's row, or a changed
  account stopping the process, and a property test whose oracle derives the effective mode and the
  startup hold from the account rows alone. `mandate-runtime`'s
  `a_reconciliation_from_before_the_start_does_not_lift_a_hold_taken_with_an_intent_outstanding`
  (DEC-1014 item 6).
  `crates/mandate-runtime-models/tests/process.rs`: the binary's refusals (a manager launch refused until its deployment is
  resolved, and `--fixture` on a manager's pod), probes, and SIGTERM drain. `tests/launch.rs`: flags, the manager's variables, a pod for
  another process, and a flag that disagrees with the manager. `tests/deployment.rs`: the latest
  deployment, a stopped agent, a missing, corrupt or invalid document, another environment, the
  approvers DEC-953's owner or the founded workspace's roster resolves, an unreadable membership
  record resolving no one, the admitted approvers narrowing at each read and never growing
  (DEC-2818), and an unknown author. `crates/mandate-runtime-models/tests/pg.rs`: the binary
  on Postgres opening its agent stream once across two runs and draining. `tests/working.rs`: a
  research mandate's quant model pinned on its admitted instrument, a `Long` only on the one
  instrument a surviving thesis holds, an ended, refused or unreadable thesis or unread control rows
  holding the `Long` while an admitted instrument keeps its exit, and a property test whose oracle
  replays the thesis records its own way (DEC-2827). `deploy/tests/test_cell.py`:
  each service started and probed as its process list says, and no runtime rendered for no
  deployment. `mandate-runtime`'s
  `an_owner_kill_switch_without_a_bid_records_null_bid_members_and_its_owner`.
- **Research tests:** `crates/mandate-daemon/tests/research.rs`: deny-all by default, no research
  while paused, a refused or waiting worker answer, and on a reconciled runtime: `RunStarted` under
  its run with the universe stored first, an unstored artifact or an invalid batch halting research
  while the kill switch goes on, a batch in doubt retried first and written once, a retry needing
  neither the source nor the sink, an answer never handed twice, a fence, a stale head, and the seed
  reaching the core. `tests/recorded.rs`: the bundle loading, the registry offering its entry, a
  missing file, a missing member, or text that is not strict JSON refusing the load, no filings,
  and a property test that every recorded item is stamped at its age before any cut-off and kept
  by `retrieve` (fixtures in `tests/fixtures/research/`).
  `tests/worker_dispatch.rs`: the worker's dispatch of an agentic run's reads and thinks (DEC-2260):
  every content and record stored before the answer, and nothing answered without the loop's bounds,
  for a run not started, under a moved allowlist, for an action, or when a store is refused.
  `tests/worker_turn.rs`: the worker's turn (DEC-2264): its transcript loaded through the sink,
  the record's prompt and response stored before `Called`, the gateway's refusal answered
  `Refused`, and nothing answered for a missing artifact, a run not started, or no agentic loop.
  `tests/recorded_turns.rs`: an agentic bundle's scripted turns (DEC-2266): read in order, a script
  the entry could not serve refusing the load, no order or size member, and each turn answered
  with its scripted calls, then the final reply.
  `tests/cell.rs` and `tests/research.rs`: the cell's agentic research (DEC-2268): the entry's loop
  capped by the recorded run cap, refused at the opt-in without one, and a run journaling every
  turn through to `RunEnded` `done`.
  `src/worker/model_tests.rs`: the worker over a model double, built only in this crate's tests
  (DEC-2269; E19-6): each turn sent to the double and answered with its own calls, and a hostile
  model's calls recorded `schema_invalid` by the gateway.
  `src/worker/injection_tests.rs` over `tests/fixtures/research-adversarial/`: the injected fixture
  (DEC-2270; E19-6): every in-scope news item, filing and the description injected, and no HI-11
  canary in any stored artifact or request body over a compromised model.
  `src/worker/bench_tests.rs` over `tests/research_rig/`: the adversarial bench (DEC-2271; E19-6):
  the cell's research in the runtime process against four compromised-model strategies at pass^8,
  with HI-10, HI-4, HI-8, HI-25 and HI-11 judged from the journal, each oracle failing on a seeded
  breach.
- **Run:** `cargo nextest run -p mandate-daemon -p mandate-runtime-models -p mandate-store -p mandate-runtime -p mandate-shell`
  (with `MANDATE_PG_URL` for `tests/pg.rs`); `cargo xtask cell demo --no-web` shows the runtime
  ready in the local cell.
- **Models (DEC-2383 item 7):** `mandate-runtime-models` (`crates/mandate-runtime-models/src/lib.rs`):
  `PinnedModels`, the daemon's `Models` port over `mandate_modelhost::evaluate` with the deployed
  document's `quant.` pins on its US equities, `registry` (the environment's `model_version`
  registrations folded from the control stream), the calendar, and the closes
  `mandate_bars::trusted_daily_closes` admits at the tick; `source` (only the cell's recording);
  `Held`, the read-only sink that only confirms the stored closes object; and `wiring`, which the
  binary sets for a resolved deployment. Tests: `tests/models.rs` (a property test against an
  oracle of integer-cent crossover, calendar close and hand-written closes object over ten
  faults; the registry fold; the source; the wiring's refusals; a sink that never writes) and
  `tests/runtime.rs` (the process journals the observation then the output on the in-memory
  journal, once per daily output, and neither when the closes are not held).
