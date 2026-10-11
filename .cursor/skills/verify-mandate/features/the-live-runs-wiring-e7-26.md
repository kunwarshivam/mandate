# The live run's wiring: the cycle's call of the grant, quote, watch and evidence seams (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (row G1b's wiring, LT-4, LT-10, the
  founder's run's steps 4 to 6); `docs/project/decisions/DEC-529.md` item 12;
  `docs/project/decisions/DEC-914.md` (the row's readings); DEC-912 (the merged seams' own
  contract); DEC-858 item 4, DEC-853 item 5, DEC-877 item 1 (the watch's terminal states and
  cap); mandate spec §6.4; trading-domain spec §4.2, §9.6; journal spec §2, §9.6, §9.7;
  notifications spec §4.2's `protection` row.
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `crates/mandate-paper/src/wiring.rs` — `asked` (the `Asked` read verbatim off the run's own
  journaled `ApprovalRequested`: the row's event id, its `content_hash` member never recomputed,
  its `deadline` seconds; the youngest ask is the run's own, a member it cannot read refuses as
  a difference, a dead youngest ask denies by its timeout), `answers` (the journal's own
  `ORDER BY seq` control rows, mapped one-to-one to their records and folded by `grant`
  verbatim in journal order, never re-sorted), `collar_x` (the collar's own tier for the asset
  class, read through the registered gate configuration's members so no second number sits
  between the gate's collar and `cross_check`'s band), `watched` (the `Watched` each wake's
  LT-10 question observes: terminal in exactly `filled`/`canceled`/`expired`/`rejected`,
  `uncovered` as the executor answers, `alerted` only on the report's own
  `unprotected_interval_limit` key), `session_cap` (DEC-877 item 1's instant: the bound's
  regular session's end, 16:00 New York on a full day, the early close on a short one, a bound
  no session holds failing closed), and `reported` (the evidence named at the run's report from
  the journal the run appended to: placing runs name the agent, account and control streams of
  journal spec §2, a run without `--place-one-order` names nothing) — re-exported by
  `crates/mandate-paper/src/lib.rs`; every stub reports `Unimplemented { story: "E7-26" }`
  until the wiring implementation lands (the call sites inside `run()`'s live arm are that PR's,
  per DEC-914 item 7).
- **Tests:** `crates/mandate-paper/tests/live_wiring.rs`, every pin pending on E7-26 until the
  wiring lands: the ask pins (the ask's members verbatim off the journaled row, a
  drifted/rebuilt order's hash never releasing; the run waiting on its own youngest ask — a
  dead youngest one denying without falling back on an older live ask; an ask it cannot read
  refusing as a difference), the fold pins (the control rows folded verbatim in journal order,
  the first answer deciding whatever follows; the committed answer releasing the wait and a
  command row deciding nothing; the wait holding unanswered until the deadline denies), the
  collar pins (the spec's own tier — at-threshold liquid, below-threshold other, unknown
  narrower, crypto its own member — and the same two quotes refusing at the liquid `x` and
  agreeing at the other's, so no constant sits where the collar's `x` belongs), the watch pins
  (terminal only in the four reported states; the interval's own alert key alone reporting;
  covered-or-flat ending, uncovered holding, the cap failing an unsettled watch closed), the cap
  pins (16:00 New York on the full day, the early close on the short day, a bound no session
  holds refusing), and the evidence pins (a placing run naming the three §2 streams from its
  own typed ids; a run that wrote nothing — no DSN, or a dry run holding one — naming nothing).
- **Run:** `cargo nextest run -p mandate-paper` (the live suite plus this row's pending pins);
  `cargo nextest run -p mandate-paper --run-ignored ignored-only --no-fail-fast` (every pending
  pin fails at its stub).
