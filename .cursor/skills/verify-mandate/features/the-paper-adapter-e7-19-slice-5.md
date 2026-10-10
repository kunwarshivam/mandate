# The paper adapter (E7-19 slice 5)

- **Spec:** `docs/project/tasks/first-paper-trade.md` (slice E1a, FT-1 to FT-12); DEC-502,
  DEC-503 item 2, DEC-505, DEC-846 (the ports, the observation the adapter stores, the order of a
  run); DEC-853 items 5 and 6, DEC-858 and DEC-877 (E1b, the watch).
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical): `src/lib.rs`
  (`Args`, `parse`, `Ports`, `Outcome`, `PaperError`, `run`, `stderr_line`), which hands the model
  host's output and its stored closes to `mandate-shell`'s `ProductionCycle::run_observed_watched`
  with a `Watch` built from `close_window_bound` and `poll_interval` over the registered rule set;
  the closes come from `mandate_shell::paper::daily_closes`. The `mandate-paper` binary
  (`src/main.rs`) prints what `process` returns, or `stderr_line` on stderr, over the `Production`
  ports: the control stream read from the Postgres
  journal, `Credentials::from_env` with `AlpacaPaperHttp`, and the system clock.
- **Tests:** `crates/mandate-paper/tests/run.rs` (one order through the cycle after the closes are
  stored, a dry run that sends nothing and keeps no journal, a `Flat` model, the refusals before
  any credential, SPY and AAPL from journaled inputs alone, a held position, stale bars, the
  closing window, a stop's message, and the watch to the registered bound), over `crates/mandate-shell/tests/common/mod.rs`'s
  deployment and recorded broker answers; `crates/mandate-paper/tests/args.rs` (the arguments);
  `crates/mandate-paper/tests/binary.rs` (the binary's refusals on stderr, none naming a DSN or
  a key, a variable that is not Unicode, and the control stream read from Postgres when
  `MANDATE_PG_URL` is set); and `crates/mandate-paper/tests/lines.rs` (the lines a run that
  refused nothing prints, and the stderr line: the cap's code alone, every other stop's message).
  The watch itself is `crates/mandate-shell/tests/watch.rs`.
- **Run:** `cargo nextest run -p mandate-paper`.
