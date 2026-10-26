# The cell's data process: each listed instrument's recorded daily and minute bars (E21-29, lane SC)

- **Spec:** [data-plane spec](../../../../docs/specs/data-plane.md) §3.6 (signal inputs recorded
  as `ObservationRecorded` with the bar window as an artifact), §3.7 (eligibility statistics from
  daily bars); [infrastructure design](../../../../docs/design/infrastructure.md) §3.1 (the
  `data` process); DEC-89 (the dataset format), DEC-846 item 2 and DEC-470 item 4 (the shell's
  trust check and the figures it feeds), DEC-961 item 3 (the rehearsal's rising closes).
- **Code:** `mandate-datad` (`crates/mandate-datad`): `src/series.rs` (`sessions`: the last 30
  completed regular sessions, ending on `last_completed_regular_session(now)`; `close_on`: each
  close a function of its date alone, the anchor's close plus 0.1 a trading day; `publish`: one
  `mandate_marketdata::dataset::Store::put_day` per session under `cell-recorded/`;
  `session_minutes`, `minute_day` and `publish_minutes`: the regular-session minute bars of the
  latest trading day on or before the New York date, written once; `Instrument::parse_list`: the
  `--instruments` list), `src/observation.rs` (`put_closes`: the closes object of the trusted closes, in
  the artifact store), and the
  `mandate-datad` binary (`run`, whose first write is its start-up check, again every 60 s; and
  `probe` for `exec` probes; SIGTERM drains). The chart runs it as `data`, writing
  `/var/lib/mandate/artifacts/datasets`, with `data.instruments` (SPY and QQQ), which only the
  local cell's values list. The executor drill (`xtask/src/cell/executor.rs` `bars`) inspects
  each instrument's daily and minute datasets.
- **Liquidity inputs (DEC-2274):** `mandate_bars::liquidity` (`crates/mandate-bars/src/liquidity.rs`)
  reads the four figures the liquidity floor and participation need from the stored daily and
  minute datasets at the reader's own clock. The shell's daily half goes through
  `trusted_daily_liquidity`. The runtime's reader is `mandate_daemon::bars` (`BarsReader::opening`,
  openings only; `liquidity_deferred`). The chart hands `MANDATE_BARS` to the runtime and the executor
  only with `images.data`. Tests: `crates/mandate-bars/tests/liquidity.rs` (the figures against an
  independent oracle, every untrusted input refused, a replay over copied partitions, each clock's
  own window), `crates/mandate-daemon/tests/bars.rs` (the shared figures for an opening, both
  checks deferred without trusted bars, no exit reads), and
  `deploy/tests/test_artifacts.py::test_only_the_gates_read_the_bars_and_only_where_they_are_stored`.
- **Tests:** `crates/mandate-datad/tests/series.rs`: the sessions end on the last completed session;
  a close is a function of its date (an oracle walking the calendar its own way); closes rise every
  session; an anchor and its reach are checked; the stored dataset passes the trust check's
  conditions under `mandate-marketdata`'s own inspection, and writing again conflicts with nothing; the closes object is stored for the trusted closes;
  the minute bars cover the regular session, an early close's included, the trailing minutes
  are the complete ones before the clock (a clock just past 00:00Z and a closed day read none),
  the minute day is the latest trading day, and the instruments list is parsed strictly.
  `tests/process.rs`: ready only once both instruments are stored, one closes object each, a store it cannot write refuses the start with a code
  and no symbol, probes, and SIGTERM drain. `deploy/tests/test_cell.py`: the data process's
  launch, mounts and egress, no render without its series, and the local cell lists both demo
  agents' instruments.
- **Run:** `cargo nextest run -p mandate-datad -p mandate-bars`, and
  `cargo nextest run -p mandate-daemon -E 'binary(bars)'`; the chart's with
  `uv run --directory python pytest ../deploy/tests` (helm required), as `cargo xtask ci cell` runs
  them.
