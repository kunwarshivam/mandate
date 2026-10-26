# The journaled gate context (DEC-2378)

- **Spec:** DEC-2378 items 2 to 7 (the facts an opening reads, folded from journal rows alone with
  the risk clock as the only time, failing closed); journal spec §9.2 (`AgentDeployed`,
  `AgentStopped`, `AccountSnapshotRecorded`, `MandateVersionApplied`) and §9.30 (`MarkUpdated`
  version 2); mandate spec §5.2.
- **Code:** `mandate-gate-context` (`crates/mandate-gate-context`): `src/lib.rs` (`Journaled`,
  its `fold` over a `Row`, its `facts` for an opening, answering `Facts` or `Held`, and `held`, the
  agent's sub-ledger a flatten sells).
- **Tests:** `crates/mandate-gate-context/tests/facts.rs`: one test a fail-closed rule (no clock,
  no mandate, another agent's deployment, a rejected or another stream's version, a stopped agent,
  no or stale snapshot, no, stale or insane mark, a version-1 mark, a mark outside the counted
  sessions, a clock that goes back, and every unreadable row holding from then on);
  `crates/mandate-gate-context/tests/replay.rs`: random row sequences against an oracle that
  searches the rows from the newest back; `crates/mandate-gate-context/tests/held.rs`: the agent's
  own fills, each once, another agent's or another stream's not, a sold-out position dropped.
- **Run:** `cargo nextest run -p mandate-gate-context`.
