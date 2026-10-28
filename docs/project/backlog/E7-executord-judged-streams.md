- **E7-executord-judged-streams (Proposed; debt; lane X)** As the founder, I want every
  mandate-executord test's account stream judged by the journal's rules, so that a batch the real
  journal refuses fails a test, as the lost exits of 2026-10-27 did not. DEC-2846 item 5 judges
  only a stream the executor opened itself. Many tests still write the account stream's
  `StreamOpened` and first rows by hand (`journaled()` in `tests/deployment.rs`, the seeded
  fixtures), and those streams are not judged. *Accepted when:* those tests let the executor open
  the stream, or seed it through the journal's own append, and the test journal judges every
  account stream.
