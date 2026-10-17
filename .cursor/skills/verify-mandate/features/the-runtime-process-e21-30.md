# The runtime process: one agent deployment's runtime as a long-running process (E21-30 slice 1)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.1, §3.4, §3.6;
  [journal spec](../../../../docs/specs/journal.md) §5.1 (writer epochs), §9 rule 12; ADR-0001
  ES-06, ES-20; DEC-17, DEC-914, DEC-968, DEC-994, DEC-1012, DEC-1013, DEC-1015, DEC-1016.
- **Code:** `mandate-daemon` (`crates/mandate-daemon`): `src/runtime.rs` (`RuntimeProcess`: one
  writer epoch on the agent stream, the control stream read before the clock stream, a batch in
  doubt offered again as the same bytes, `mandate-process`'s lifecycle), `src/wake.rs` (`Wake`, a
  hint that only shortens a wait), `src/fixture.rs` (the binary's deny-all wiring until the post-R0
  hook), `src/launch.rs` (the deployment from `run`'s flags or the deployment manager's variables,
  DEC-1015), `src/deployment.rs` (the connection, approvers and author from the latest
  `AgentDeployed` and its stored mandate document, DEC-1016), and the `mandate-daemon` binary (`run` over Postgres, or `--fixture` over an in-memory
  journal, and `probe` for `exec` probes; SIGTERM drains). In the cell: the `runtime` image
  (`deploy/cell/images/runtime.Dockerfile`), its `args` and `probe` in
  `deploy/cell/charts/workspace/processes.json`, and `cargo xtask cell up` building it (DEC-1013). `mandate-store` (`crates/mandate-store`): the `Store` port and the shared envelope.
  `mandate-runtime`'s `src/tail.rs` (`tailed`, shared with the shell).
- **Tests:** `crates/mandate-daemon/tests/runtime.rs`: the owner's kill switch journaled on the
  agent stream, the control stream read before the clock stream, an unreadable clock stream never
  holding the kill switch, a control stream handed again writing nothing twice, a lost answer
  written once, a hint deciding nothing, a second process fencing the first, paper only, drain, a
  draft without references stopping the process, and a property test over interleavings of kill
  switches, ticks, lost answers, and restarts with an oracle from the control rows.
  `tests/process.rs`: the binary's refusals (a manager launch refused until its deployment is
  resolved), probes, and SIGTERM drain. `tests/launch.rs`: flags, the manager's variables, a pod for
  another process, and a flag that disagrees with the manager. `tests/deployment.rs`: the latest
  deployment, a stopped agent, a missing, corrupt or invalid document, another environment, only
  `user:` approvers, and an unknown author. `tests/pg.rs`: the binary
  on Postgres opening its agent stream once across two runs and draining. `deploy/tests/test_cell.py`:
  each service started and probed as its process list says, and no runtime rendered for no
  deployment. `mandate-runtime`'s
  `an_owner_kill_switch_without_a_bid_records_null_bid_members_and_its_owner`.
- **Run:** `cargo nextest run -p mandate-daemon -p mandate-store -p mandate-runtime -p mandate-shell`
  (with `MANDATE_PG_URL` for `tests/pg.rs`); `cargo xtask cell demo --no-web` shows the runtime
  ready in the local cell.
