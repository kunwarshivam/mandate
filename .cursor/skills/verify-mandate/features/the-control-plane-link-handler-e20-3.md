# The control-plane link's workspace-services handler (E20-3)

- **Spec:** [control-plane design](../../../../docs/design/control-plane.md) CP-3, CP-4, CP-5 (b),
  CP-9; journal spec §9.17; DEC-981, DEC-983.
- **Code:** `mandate-cp-site` (`crates/mandate-cp-site`). `src/lib.rs` (`Site`, which implements
  `mandate-cp-agent`'s `InstructionSink`; the `ControlStreams` and `Effects` ports; `Writer`; the
  per-workspace CP-9 judgement and the fan-out answer).
- **Tests:** `tests/site.rs`, through `mandate-cp-agent`'s `receive` into the journal's own
  `MemoryJournal`: a license journaled on its organization's workspaces before its effect; CP-9 once
  per kind; catalog kinds and datasets judged apart; refusals journaled once on every workspace; a
  license for an organization not hosted; an unreadable stream retried without duplicates; a
  deployment hosting no workspace keeps the message; a kill switch commits while an effect hangs;
  a property that only §9.17 records are ever appended.
- **Run:** `cargo nextest run -p mandate-cp-site`.
