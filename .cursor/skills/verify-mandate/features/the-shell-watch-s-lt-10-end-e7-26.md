# The shell watch's LT-10 end (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (LT-10, row G1b's **bounded reads until
  LT-10**, the founder's run's step 5); `docs/project/decisions/DEC-912.md` item 3;
  `docs/project/decisions/DEC-858.md` items 4 and 5; `docs/project/decisions/DEC-853.md`
  item 5; `docs/project/decisions/DEC-877.md` item 1; trading-domain spec §5.4;
  notifications spec §4.2's `protection` row.
- **Code:** `mandate-shell`, `crates/mandate-shell/` (layer 8, safety-critical):
  `crates/mandate-shell/src/lt10.rs` (`watch_may_end` — the per-wake LT-10 question the
  shell's own watch asks in place of its bare terminal read, DEC-475 keeping the cycle's
  private `Session` inside the crate: the run may end only on a terminal entry in DEC-858
  item 4's exact four with any filled quantity covered or the unprotected interval's own
  `unprotected_interval_limit` key in the report, keeps reading while either half is
  unsettled — an entry still working included — and fails closed past the cap, read here off
  the same calendar the bound read: 16:00 New York on a full day, the early close on a short
  one, a bound no session holds refusing absent, with the shell's existing
  `cancel_unconfirmed` refusal), exported by `crates/mandate-shell/src/lib.rs`. **Pending
  stub only**: the wake loop that asks the question is `Session::watch` in
  `crates/mandate-shell/src/tracer.rs`, where the implementation PR wires it in, deleting the
  stub body alone.
- **Tests:** `crates/mandate-shell/tests/lt10.rs`, every pin `pending E7-26` and failing at
  the stub's own report (DEC-77, DEC-137) until that PR: the covered-or-flat pins (a terminal
  covered or flat entry ending the run; an uncovered quantity keeping it reading; an uncovered
  quantity ending only with its alert key reported; another interval's alert key —
  `stop_watchdog`, the other key of the notifications spec §4.2 `protection` row — holding the
  run open; an entry still working, and a read absent, keeping it reading; the executor
  codec's whole thirteen states walked against DEC-858 item 4's four), the cap pins (the
  full-day cap at 16:00 New York and the early-close cap at 13:00 on 2026-12-24, each
  inclusive — an unsettled watch failing closed with the shell's existing
  `cancel_unconfirmed` refusal, a settled one ending clean at it), and the fail-closed pin (a
  bound no session holds refusing absent, FT-8). The run-level watch tests that stay green
  through the implementation — the unsettleable cap's stop before any read at 16:00 on the
  full day and at the early close, the terminal ends with the bracket legs placed at once,
  the cancel at the bound, the dry run watching nothing — live in
  `crates/mandate-shell/tests/watch.rs` (DEC-858, DEC-877).
- **Run:** `cargo nextest run -p mandate-shell` (the living suite, every green test
  unchanged); `cargo nextest run -p mandate-shell --run-ignored ignored-only --no-fail-fast`
  (the nine pending pins, each failing at the stub's own report until the implementation PR).
