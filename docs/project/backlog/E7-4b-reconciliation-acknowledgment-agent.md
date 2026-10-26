- **E7-4b reconciliation acknowledgment agent (Proposed, M7; SC)** As an owner, I want an
  acknowledgment of one agent's reconciliation hold to name that agent, so that one agent's
  acknowledgment never lifts another agent's hold. [DEC-2830](../decisions/DEC-2830.md) item 1 lets
  `agent` be any agent of the account, as for a restriction, so the executor's §9.28 check 1 for
  `subject` `reconciliation` (`held_by` and `named_stands` in `mandate-executor`'s
  `acknowledge.rs`) finds the hold by `event` alone, while a limit's check 1 compares the agents
  (`trigger_unknown`). This story tightens that item under DEC-176, which a new decision records:
  it only adds refusals, so it adds no risk; today it reaches only the same owner's agents on the
  same account. *Accepted when:* the new decision amends DEC-2830 item 1; journal spec §9.28 and
  its `reconciliation_acknowledgments` vectors in `docs/specs/reference-cases/journal.yaml` say,
  in a spec PR before the executor change (ES-22), that an acknowledgment of a hold of a named
  agent whose `agent` is another agent is refused `trigger_unknown`, and that a hold of every
  agent (`*`, the account's) still takes any agent of the account, as DEC-2830 item 1 reads now;
  the executor's check 1 does the same; and the shared fold `mandate_journal::holds`, its
  differential test in `acknowledge.rs` and `mandate agent holds` (all three arriving with PR
  #2762, which lands first so this change has one fold to update) probe and post to the hold's
  own agent.
<!-- section: E7 Alpaca connector and recovery -->
