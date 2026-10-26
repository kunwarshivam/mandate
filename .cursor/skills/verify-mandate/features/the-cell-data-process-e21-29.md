# The cell's data process: the deployed instrument's recorded daily bars (E21-29, lane SC)

- **Spec:** [data-plane spec](../../../../docs/specs/data-plane.md) §3.6 (signal inputs recorded
  as `ObservationRecorded` with the bar window as an artifact), §3.7 (eligibility statistics from
  daily bars); [infrastructure design](../../../../docs/design/infrastructure.md) §3.1 (the
  `data` process); DEC-89 (the dataset format), DEC-846 item 2 and DEC-470 item 4 (the shell's
  trust check and the figures it feeds), DEC-961 item 3 (the rehearsal's rising closes).
- **Code:** `mandate-datad` (`crates/mandate-datad`): `src/series.rs` (`sessions`: the last 30
  completed regular sessions, ending on `last_completed_regular_session(now)`; `close_on`: each
  close a function of its date alone, the anchor's close plus 0.1 a trading day; `publish`: one
  `mandate_marketdata::dataset::Store::put_day` per session under `cell-recorded/`;
  `session_minutes` and `publish_minutes`: the current trading day's regular-session minute bars,
  written once), `src/observation.rs` (`put_closes`: the closes object of the trusted closes, in
  the artifact store), and the
  `mandate-datad` binary (`run`, whose first write is its start-up check, again every 60 s; and
  `probe` for `exec` probes; SIGTERM drains). The chart runs it as `data`, writing
  `/var/lib/mandate/artifacts/datasets`, with `data.symbol` and `data.anchor`, which only the
  local cell's values set.
- **Tests:** `crates/mandate-datad/tests/series.rs`: the sessions end on the last completed session;
  a close is a function of its date (an oracle walking the calendar its own way); closes rise every
  session; an anchor and its reach are checked; the stored dataset passes the trust check's
  conditions under `mandate-marketdata`'s own inspection, and writing again conflicts with nothing; the closes object is stored for the trusted closes;
  the minute bars cover the regular session, an early close's included, and the trailing minutes
  are the complete ones before the clock.
  `tests/process.rs`: ready only once stored, a store it cannot write refuses the start with a code
  and no symbol, probes, and SIGTERM drain. `deploy/tests/test_cell.py`: the data process's
  launch, mounts and egress, and no render without its series.
- **Run:** `cargo nextest run -p mandate-datad`; the chart's with
  `uv run --directory python pytest ../deploy/tests` (helm required), as `cargo xtask ci cell` runs
  them.
