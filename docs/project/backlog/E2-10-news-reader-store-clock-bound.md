# E2-10 follow-up: hold the news reader's receipt bound against the hot store's clock

- **E2-10f (Proposed, M5; lane SC)** As the founder, I want the news reader's receipt bound to catch
  a host clock that is already ahead when the reader starts, so that a skew present at boot cannot
  pin the news stream's last `knowledge_at` in the future. Today the trusted time is the host clock
  read at start plus a monotonic clock's elapsed time (DEC-2289), so a skew at boot passes the
  bound. *Accepted when:* the trusted time is also bounded by a second host's clock, the hot store's
  `now()`, read through a `Store` method that the Postgres and in-memory stores implement. A reader
  whose store cannot answer it holds its rounds, as a held round does. A test injects a host clock
  ahead at start, with the store's clock behind it, and shows the round is held, counted and alerted.
<!-- section: E2 Market data -->
