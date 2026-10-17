# The runtime process: one agent deployment's runtime as a long-running process (E21-30 slice 1)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.1, §3.4, §3.6;
  [journal spec](../../../../docs/specs/journal.md) §5.1 (writer epochs), §9 rule 12; ADR-0001
  ES-06, ES-20; DEC-17, DEC-914, DEC-968, DEC-1012, DEC-2209.
- **Code:** `mandate-daemon` (`crates/mandate-daemon`): `src/runtime.rs` (`RuntimeProcess`: one
  writer epoch on the agent stream, the control stream read before the account stream the wiring
  names and the clock stream, the account stream folded at start and each row past the core's head
  folded once a pass (DEC-2209), a batch in
  doubt offered again as the same bytes, `mandate-process`'s lifecycle), `src/wake.rs` (`Wake`, a
  hint that only shortens a wait), `src/fixture.rs` (the binary's deny-all wiring until the post-R0
  hook), and the `mandate-daemon` binary (`run --fixture`, and `probe` for `exec` probes; SIGTERM
  drains). `mandate-store` (`crates/mandate-store`): the `Store` port and the shared envelope.
  `mandate-runtime`'s `src/tail.rs` (`tailed`, shared with the shell).
- **Tests:** `crates/mandate-daemon/tests/runtime.rs`: the owner's kill switch journaled on the
  agent stream, the control stream read before the clock stream, an unreadable clock stream never
  holding the kill switch, a control stream handed again writing nothing twice, a lost answer
  written once, a hint deciding nothing, a second process fencing the first, paper only, drain, a
  draft without references stopping the process, and a property test over interleavings of kill
  switches, ticks, lost answers, and restarts with an oracle from the control rows.
  `tests/account.rs`: the account stream folded at start and tailed each pass, a clean
  reconciliation lifting the startup hold, this agent's restrictions honoured, control then account
  then clock, an unreadable or unfoldable account stream, each row folded once across restarts, and
  a property test of the mode against an oracle from the account rows.
  `tests/process.rs`: the binary's refusals, probes, and SIGTERM drain. `mandate-runtime`'s
  `an_owner_kill_switch_without_a_bid_records_null_bid_members_and_its_owner`.
- **Run:** `cargo nextest run -p mandate-daemon -p mandate-store -p mandate-runtime -p mandate-shell`.
