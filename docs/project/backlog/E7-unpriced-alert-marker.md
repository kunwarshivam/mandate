- **E7-unpriced-alert-marker (Proposed; trading core; lane X)** As an owner, I want every exit
  that waits for its configuration to be alerted, so that one waiting exit never silences another.
  - **Today:** the fold notes the last `exit_unpriced` with no kill switch as its cause, per
    instrument, and DEC-2846's wait alerts only for a breach or interval that began after it. An
    exit going at its own limit for want of a price (DEC-160 (12)) journals the same record. So a
    breach that began before such a record, and later waits for its configuration, is not alerted
    again. The exit still waits and goes once the configuration reads.
  - **Also:** an exit that waits indefinitely is alerted once, with no reminder.

  *Accepted when:* the waiting record is told apart from the fallback's (a payload member, which
  needs journal spec §9 to admit it, or a causation), a test shows both alerted in one breach,
  and a reminder cadence is decided.
