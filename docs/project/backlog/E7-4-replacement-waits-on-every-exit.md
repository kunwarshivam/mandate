- **Possible safety-rule conflict, for the founder: a re-placement waits on every working exit in
  the instrument** (AGENTS.md rule 13, [DEC-485](../decisions/DEC-485.md)). After a marketable exit
  ends, `mandate-executor`'s `protection::settle` re-places protection only once every working
  exit in the instrument has ended, another holder's included. So under a shared position one
  agent's re-placement is held by another agent's exit, until that exit ends or the unprotected
  interval's `max_unprotected_s` bound ends it. Rule 13 lets exits and protective orders be held
  only by agent mode `paused` or `stopped`, by an `Unknown` order in the same instrument, or by the
  broker; another agent's working exit is none of these, though the gap stays journaled and
  bounded (trading spec §5.4). A narrowed wait was tried: wait only on the sequence's own agent's
  exits. It broke DEC-485's tests, `a_handed_on_sequence_is_bounded_by_ending_the_exit_beside_it`
  and the agent kill switch's handed-sequence case, which rely on the instrument-wide wait to bound
  a sequence handed on beside another exit, so it was reverted. Lane X's item 8 PRs keep the wait as
  it is. *Accepted when:* the founder rules whether the wait stands as a bounded, journaled gap
  within rule 13 or must narrow; if it narrows, DEC-485's handed-sequence bound is re-derived in a
  decision of its own and its tests are rewritten against it, tests first.
