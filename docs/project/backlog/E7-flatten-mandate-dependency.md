- **E7-flatten-mandate-dependency (Proposed; trading core; lane X)** As an owner, I want my kill
  switch's flatten never to wait on configuration, so that rule 13's "does not depend on model
  state" holds for its sell as well as for the stop.
  - **Today:** a flatten's `IntentReceived` names the agent's `mandate_version`, which journal spec
    §9 requires, and its `GateDecided` names all five references. Where they do not read, DEC-2846
    applies the switch at once and the flatten waits, journaled `exit_unpriced` and alerted.
  - **The question:** a flatten is pure risk reduction, so it arguably needs only the instrument
    and fee references, never the mandate or the rule set.

  *Accepted when:* the founder or the journal spec decides whether a kill switch's flatten
  journals without the mandate reference and the rule set. If it does, the executor sells without
  waiting for them, and a test shows a flatten sold while they do not read.
