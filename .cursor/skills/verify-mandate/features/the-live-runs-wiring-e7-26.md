# The live run's wiring: the cycle's call of the grant, quote, watch and evidence seams (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (row G1b's wiring, LT-4, LT-10, the
  founder's run's steps 4 to 6); `docs/project/decisions/DEC-529.md` item 12;
  `docs/project/decisions/DEC-914.md` (the row's readings); DEC-920 (the release on the
  runtime's admitted grant); DEC-912 (the merged seams' own
  contract); DEC-858 item 4, DEC-853 item 5, DEC-877 item 1 (the watch's terminal states and
  cap); mandate spec §6.4; trading-domain spec §4.2, §9.6; journal spec §2, §9.6, §9.7;
  notifications spec §4.2's `protection` row.
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `crates/mandate-paper/src/wiring.rs` — `asked` (the `Asked` read verbatim off the run's own
  journaled `ApprovalRequested`: the row's event id, its `content_hash` member never recomputed,
  its `deadline` seconds; the youngest ask is the run's own, a member it cannot read refuses as
  a difference, a dead youngest ask denies by its timeout), `answers` (the journal's own
  `ORDER BY seq` control and agent rows, mapped one-to-one to their `Journaled` records — event
  id, `causation_id`, record — and folded by `grant` verbatim in journal order, never re-sorted;
  an unreadable row refusing only when its type is one the fold reads, DEC-920 item 4), `collar_x` (the collar's own tier for the asset
  class, read through the registered gate configuration's members so no second number sits
  between the gate's collar and `cross_check`'s band), `watched` (the `Watched` each wake's
  LT-10 question observes: terminal in exactly `filled`/`canceled`/`expired`/`rejected` — the
  codec's other nine states, `unknown` and `pending_cancel` included, leave the run reading —
  `uncovered` as the executor answers, `alerted` only on the report's own
  `unprotected_interval_limit` key), `session_cap` (DEC-877 item 1's instant: the bound's
  regular session's end, 16:00 New York on a full day, the early close on a short one, a bound
  no session holds failing closed), and `reported` (the evidence named at the run's report from
  the journal the run appended to: placing runs name the agent, account and control streams of
  journal spec §2, a run without `--place-one-order` names nothing), implemented since this row
  closed its pins. The runner's call frames beside them: `wait_for_the_grant` (the ask wait that
  re-reads both streams per wake (`WakeRead`) at DEC-853 item 6's interval, releases the submit only on the
  runtime's admitted, re-validated grant of the ask's first deciding answer and never submits
  itself), `cross_check_the_opening` (the
  gate's own members read into the collar's `x` first, then `cross_check` over the two asks,
  before the intent leaves) and `watch_may_end` (the watch's per-wake LT-10 question over
  `watched`, at the cap `session_cap` reads, in place of the paper watch's bare terminal read),
  each wired over row inputs only; `run()` names its placed cycle's evidence at its outcome
  (`mandate_paper::Cycle`, beside the shell's `Report`, which cannot hold a `mandate-paper`
  type). `connection.rs`'s `member` reads the wiring's text members (`pub(crate)`, the flip
  #1311 reserved to the implementation).
- **Tests:** `crates/mandate-paper/tests/live_wiring.rs`, every pin live since the wiring
  landed: the ask pins (the ask's members verbatim off the journaled row, a
  drifted/rebuilt order's hash never releasing; the run waiting on its own youngest ask — a
  dead youngest one denying without falling back on an older live ask; an ask it cannot read
  refusing as a difference), the fold pins (the control rows folded verbatim in journal order,
  the first answer deciding whatever follows; the committed answer releasing the wait and a
  command row deciding nothing; the wait holding unanswered until the deadline denies; the
  runtime's records linking by their own envelopes; an unreadable row refusing only where the
  fold reads it), the
  collar pins (the spec's own tier — at-threshold liquid, below-threshold other, unknown
  narrower, crypto its own member — and the same two quotes refusing at the liquid `x` and
  agreeing at the other's, so no constant sits where the collar's `x` belongs), the watch pins
  (terminal only in the four reported states; the interval's own alert key alone reporting;
  covered-or-flat ending, uncovered holding, the cap failing an unsettled watch closed), the cap
  pins (16:00 New York on the full day, the early close on the short day, a bound no session
  holds refusing), and the evidence pins (a placing run naming the three §2 streams from its
  own typed ids; a run that wrote nothing — no DSN, or a dry run holding one — naming nothing);
  the fifth-state fixtures (#1311's review: the codec's whole thirteen walked against
  DEC-858 item 4's four, `unknown` keeping the run reading and `pending_cancel` failing it
  closed at the cap, never a clean end), the wait-cadence fixture (both streams re-read per
  wake, a raw answer holding until the runtime's admission lands; an answer never admitted
  denying at the ask's own bound) and the cross-check frame's tier
  fixture beside them. `tests/run.rs`'s dry run names no exportable stream.
- **Run:** `cargo nextest run -p mandate-paper` (the whole suite live and passing, no ignored
  test in the crate); `cargo check -p mandate-paper --features live` (the wiring compiles
  under the founder's feature, which no CI or release build enables).
