- **E21-30 (Should, M6, debt; DR)** The paper shell's event ids derive from their stream, as the
  runtime's, the executor's and the scheduler's do since [DEC-2389](decisions/DEC-2389.md).
  `mandate-shell`'s `envelope::Ids` still builds an id from a space digit, the epoch, the head and
  the ordinal, so a second agent or account stream that `mandate-paper` writes into a journal
  another run already wrote would collide with that run's ids. The journal refuses the collision
  (journal spec §3), so the second run cannot start. *Accepted when:* `envelope::Ids` is built per
  stream and derives with `mandate_canon::stream_event_id`, and a test runs two accounts' paper
  runs on one journal.
