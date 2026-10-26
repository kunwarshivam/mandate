- **E7-4b (In progress; L1; [DEC-2830](../decisions/DEC-2830.md), journal v0.104, before the 2026-10-27 demo)** As an owner, I want to acknowledge a reconciliation hold (a
  mismatched instrument's pause, held external activity, a batch the journal refused) through an
  acknowledgment the journal admits, so that an agent held for one trades again once I have seen
  why. Today none can be lifted. The executor's fold used to clear these holds by a pre-§9.28
  `OwnerAcknowledged` with a `subject` member, but `append` has refused that shape since journal
  spec §9.28 closed the schema. [DEC-2828](../decisions/DEC-2828.md) removed the reader, so every
  such hold stays: the reading that lifts nothing. *Accepted when:* the journal spec names an
  acknowledgment `subject` for each hold kind and the record each one names (as
  [DEC-2824](../decisions/DEC-2824.md)'s `subject` names a restriction); the executor judges it
  with §9.28's checks and lifts only that hold; and a hold the owner never acknowledged is never
  lifted.
<!-- section: E7 Alpaca connector and recovery -->
