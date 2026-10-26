# X10 follow-up: an executor core error on broker input fails safe

- **X10d (Proposed; trading core)** As an owner, I want the executor to keep running its exits,
  protection and kill switch when one broker answer makes its core return an error, so that one bad
  broker field never stops it. DEC-2805 covers a batch the journal refuses; a core error on an input
  (`handle` returning `Err`) still stops the process, and a restart may meet the same input. *Accepted
  when:* the errors a broker input can cause are listed, each that is not an invariant breach is held
  as DEC-2805 item 1 holds a refused batch, and a test injects each.
<!-- section: E7 Alpaca connector and recovery -->
