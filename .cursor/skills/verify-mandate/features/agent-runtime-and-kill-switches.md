# Agent runtime and kill switches

Planned by [the E6-1 and E6-5 task brief](../../../../docs/project/tasks/E6-1-agent-runtime-and-kill-switches.md)
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
  [first paper trade brief](../../../../docs/project/tasks/first-paper-trade.md)),
  `crates/mandate-runtime/tests/common/mod.rs` (the in-memory shell, which can put an append in doubt,
  fence a writer, and crash and restart), and `crates/mandate-runtime/tests/golden-journal.json` (the
  committed fold output that pins `FOLD_VERSION`). Planted bugs per test: the task brief.
- **Reference cases:** none move. `trading_domain::RC-14`'s `kill_switch` variant also needs E7-2's
  `actions` and E6-9's `agent_mode`; the mandate suite's flatten family MC-F01 to MC-F04 belongs to
  `mandate-risk`.
- **Run:** `cargo nextest run -p mandate-runtime`, and the mutation gate the implementation PR must
  pass, `MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants`.
