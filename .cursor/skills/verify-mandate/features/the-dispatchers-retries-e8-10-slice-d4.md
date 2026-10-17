# The dispatcher's retries, read from the notice stream (E8-10, slice D4)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §5.1 step 4 (a notice
  with no terminal attempt is due again), §5.3 (at once, then 15 s, 60 s, 5 min, then every
  15 min; `safety` 24 hours, `info` 6 hours, `abandoned` with `retry_window_ended`), §7 ("A
  provider is down"); journal spec §9.15 Lifecycle; DEC-706 items 5 and 6; DEC-707; DEC-710 item 5;
  DEC-725 item 6.
- **Code:** `crates/mandate-dispatcher/src/due.rs` (`due`, `Due`, `Next`), a stub in the tests PR
  (DEC-77); `crates/mandate-dispatcher/src/step.rs` (`step`) acts on it in the code PR.
- **Tests:** `crates/mandate-dispatcher/src/due/tests.rs` pins `due` over notice streams written
  row by row: attempt 1 at once, each gap of §5.3's table at its exact instant, the terminal
  statuses and verdicts, a version-1 `failed` read as `permanent`, the window's end (an attempt due
  exactly at it is made; abandoned at the first instant at or after it, never before), a late
  attempt after a lapse, `action` never abandoned before D7, `event_time` as the time base, and one
  schedule and key per send. `crates/mandate-dispatcher/src/step/tests/retry.rs` drives `step`
  over `MemoryJournal`: the next attempt number under one key with its variant's verdict, one
  `abandoned` at the window's end for `safety` and `info`, one record per send per step after a
  lapse, and a property over random answers, clock ticks (some landing exactly on the oracle's next
  instant), and restarts, whose oracle derives each channel's next step from the committed records
  and §5.3's table typed in the test (never `next_attempt`). Seeded bugs it catches: abandoning at
  the last failure instead of the window's end, and a window measured from the last attempt.
- **Run:** `cargo nextest run -p mandate-dispatcher` (the D4 tests are pending until the code PR;
  `cargo nextest run -p mandate-dispatcher --run-ignored ignored-only` shows them failing).
