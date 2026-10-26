- **E7-4b reconciliation acknowledgment agent (Proposed, M7; SC)** As an owner, I want an
  acknowledgment of a reconciliation hold to lift only a hold of the agent it names, so that one
  agent's acknowledgment never lifts another agent's hold. The executor's §9.28 check 1 for
  `subject` `reconciliation` (`held_by` and `named_stands` in `mandate-executor`'s
  `acknowledge.rs`, [DEC-2830](../decisions/DEC-2830.md)) finds the hold by the acknowledgment's
  `event` alone and never compares the acknowledgment's `agent` with the hold's, so any agent of the
  account can acknowledge, and lift, another agent's reconciliation hold; a limit's check 1 already
  compares them (`trigger_unknown`). It reaches only the same owner on the same account, and no
  demo account holds two agents. *Accepted when:* check 1 refuses `trigger_unknown` an
  acknowledgment of a hold of a named agent whose `agent` is another agent, and a hold of every
  agent (`*`) takes only an acknowledgment scoped to the account (any agent of it, as the executor
  process hands only its followed agents' acknowledgments, or an account-scoped form if §9.28 gains
  one), with reference vectors in a spec PR before the executor change (ES-22), the shared fold
  `mandate_journal::holds` and the differential test in `acknowledge.rs` probing the agent too,
  and `mandate agent holds` posting each hold to its own agent. First after #2762 lands.
<!-- section: E7 Alpaca connector and recovery -->
