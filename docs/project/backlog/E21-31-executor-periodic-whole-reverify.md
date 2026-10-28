- **E21-31-executor-periodic-whole-reverify (Proposed; hardening; lane D1)**
  As the owner, I want a rewrite of journal history that an executor has already read to be found
  within a bounded time, not only at the executor's next start. DEC-2847 item 4 accepts that the
  executor's kept folds (the scheduler's clock, the control stream, each research agent stream's
  thesis records) and the runtime's tails do not see again a record they have read. So a change in
  the hot store to rows before a kept head, leaving the head row as it was, breaks the stored chain
  where only a whole read sees it, and that can last until a restart. DEC-2145 item 3 closes the
  same gap for the API server's read cache: each held stream is read whole again once its last whole
  read is a minute old, and a read that fails there drops that workspace's held rows and fails
  closed.
  **The fix:**
  - Give `executord`'s kept folds and the runtime's tails the same bound: read each kept stream whole
    again, onto a fresh fold, once its last whole read passes a fixed age, and fold on only if it
    verifies.
  - A whole read that fails holds as a failed read does today (DEC-2847 item 3): nothing kept is
    folded on, and nothing adds risk.
  - Keep `LIVENESS` (DEC-2847 item 5) in mind: the whole re-read must fit a pass, or run between
    passes.
  **Done when** a test that changes a row below a kept head, leaving the head row as it was, shows
  the executor failing that stream's reads within the bound and not before, as
  `crates/mandate-api-server/tests/read_cache.rs`'s
  `a_change_below_the_held_head_is_seen_at_the_next_whole_read_within_reverify` does for the API.
<!-- section: E21 Operations and infrastructure -->
