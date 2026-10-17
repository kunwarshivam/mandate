# The scheduler process: the clock stream's single writer, its ticks and clock measurements (E21-29)

- **Spec:** [journal spec](../../../../docs/specs/journal.md) §2 (the scheduler stream), §5.1
  (writer epochs), §5.4 (clock), §9.18 (the scheduler stream's records, rules 143 to 148);
  [infrastructure design](../../../../docs/design/infrastructure.md) §3.1, §3.4, OPS-14; DEC-1010,
  DEC-1011.
- **Code:** `mandate-scheduler` (`crates/mandate-scheduler`): `src/tick.rs` (each tick the later of
  the last plus a second and the host clock's second; the first tick of each trading date),
  `src/measure.rs` (`Measurement::judge`: the median by magnitude of the answering servers, out of
  tolerance when none answered; `due`: at start, before the regular session, every five minutes),
  `src/records.rs` (the five records as drafts with deterministic ids), `src/store.rs`
  (`ClockStore` over `MemoryJournal` and Postgres), `src/scheduler.rs` (`Scheduler`: one writer
  epoch, replay, a batch in doubt retried as the same bytes, `mandate-process`'s lifecycle),
  `src/sntp.rs` (one SNTP request per server over UDP), and the `mandate-scheduler` binary (`run`,
  and `probe` for `exec` probes; SIGTERM drains). The closed records are registered in
  `mandate-journal`'s `src/scheduler.rs`.
- **Tests:** `crates/mandate-journal/tests/scheduler.rs`: every member type and rule 143 to 148,
  with an integer oracle for rules 146 and 147. `crates/mandate-scheduler/tests/clock.rs`: ticks
  never stop, repeat, or run backwards; trading days start once, in order; the median and the
  schedule. `tests/scheduler.rs`: the stream opened once, an out-of-tolerance clock journaled while
  ticks go on, no server answering, a host clock stepping back, a second scheduler fencing the
  first before it writes, restart, a batch in doubt written once, drain, paper only.
  `tests/host_clock.rs`: no order-path crate reads a host clock. `tests/sntp.rs`: offsets, delays,
  and refused replies. `tests/process.rs`: the binary's probes and SIGTERM drain.
- **Run:** `cargo nextest run -p mandate-scheduler -p mandate-journal`.
