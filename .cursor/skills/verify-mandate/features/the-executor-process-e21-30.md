# The executor process: one broker account's executor as a long-running process (E21-30, lane X)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.1, §3.4, §3.6;
  [journal spec](../../../../docs/specs/journal.md) §2 (the copy rules), §5.1 (writer epochs),
  §9.1 (`KillSwitchActivated`, `OwnerExitRequested`); trading spec §5.5 (the agent-scoped kill
  switch), §11 (reconciliation); connections spec §8.1 (the connect check); ADR-0001 ES-06, ES-20; DEC-17, DEC-1171, DEC-2003, DEC-2007.
- **Code:** `mandate-executord` (`crates/mandate-executord`): `src/process.rs` (`ExecutorProcess`:
  one writer epoch on the account stream, `StreamOpened` on an empty one, `Input::Started` on the
  first pass that reads a tick, every draft journaled before the broker sees what it names, an
  input in doubt handed again first, the reconciliation's four reads as one snapshot with its
  account reported first and read again when an answer is lost, each agent's kill switch applied
  once as the core's own agent-scoped switch naming the record it copies (`copied_from`), ready or
  not, an unreadable stream alerted once, `mandate-process`'s lifecycle) and
  `src/follow.rs` (`agents_of`: the account's agents from version-2 `ConnectionEstablished` and
  `AgentDeployed`; `switches`: an agent stream's `KillSwitchActivated` records with the bid the
  same command's `OwnerExitRequested` confirmed); `src/paper.rs` (`FixturePaper`, the fixture
  paper broker below `mandate-alpaca`'s client, DEC-2007 item 1) and `src/alpaca.rs`
  (`AlpacaBroker`, the client as the process's `Broker`, DEC-2007 item 2), `src/connect.rs`
  (`connect_check`: `StreamOpened` and `ConnectionChecked` for a paper Alpaca connection, DEC-2007
  item 4), `src/fixture.rs` (the binary's wiring, which admits no opening), and `mandate-store`'s
  `draft_bytes_with_pii`; `src/launch.rs` (the account from `run`'s flags or the deployment
  manager's variables) and the `mandate-executord` binary (`run`, `connect-check`, `probe`;
  SIGTERM drains, DEC-2007 item 3). The agents' orders are slice X1b.
- **Tests:** `crates/mandate-executord/tests/process.rs`: paper only, the stream opened once, the
  start waiting for a tick, readiness only after a clean startup reconciliation, a lost read read
  again without holding the kill switch, a process that is not ready applying the kill switch, a
  switch applied once across passes and restarts, a workspace or connection switch applied to the
  agent alone with no account-wide request, every waiting switch applied in one pass, only the
  account's agents followed, a stopped agent's switch still applied, a later version-1 deployment
  keeping the agent followed, a revoked connection
  followed again once established again, no pause of every agent, earlier pause of the agent or
  blocked account swallowing a switch, a lifted switch not applied again, an unreadable row
  stopping the process after the readable switches, an unreadable control record stopping it, an
  unreadable stream alerted once, a lost answer written once, a pass going on after a retry, the chain
  bound, a fence, drain, an unreadable switch stopping the process, and an unreadable control
  stream holding nothing back for good.
  `tests/properties.rs`: over random runs of restarts, crashes at any append, lost journal and
  broker answers and switches, every switch applied exactly once and no account-wide request; and
  a pass with nothing new changing nothing. `src/follow.rs`'s unit tests: each initiator, and the
  bid a switch carries. `tests/support/mod.rs`: the journal, broker and core doubles.
  `mandate-executor`'s `tests/copied_switch.rs`: a copied switch's `copied_from`, read back live
  and after a replay, and `null` for a switch that copies nothing.
  `tests/paper.rs`: the fixture's account, resting orders, duplicates, lookups, cancels and
  profile through `AlpacaBroker`.
  `tests/connect.rs`: a passing check opening the stream with every result and the reference in
  `pii_refs`, a second check appending only its record, an unreadable account failing and
  journaled, and invalid ids refused. `src/fixture.rs`'s unit tests: its ids and wiring.
  `tests/journal.rs`: the binary's process on the in-memory journal and the fixture broker,
  reconciled and ready, once across a restart. `tests/launch.rs` and `tests/binary.rs`: the
  launch's refusals, `connect-check`'s output, and probes and drain. `tests/pg.rs`: the connect
  check and the process on Postgres.
- **Run:** `cargo nextest run -p mandate-executord` (with `MANDATE_PG_URL` for `tests/pg.rs`).
