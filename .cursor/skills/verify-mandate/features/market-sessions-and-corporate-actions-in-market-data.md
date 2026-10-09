# Market sessions and corporate actions in market data

- **Spec:** backlog E2-4; trading domain spec §1 principle 2, §2.2, §4.2, §4.3, §4.5, §8.5; DEC-82,
  DEC-89, DEC-91; `docs/project/tasks/E2-4-sessions-and-corporate-actions.md`.
- **Code:** `crates/mandate-time/src/session.rs` and `crates/mandate-time/data/us-equities.calendar`
  (the NYSE calendar 2018 to 2028, four sessions per trading day, and
  `last_completed_regular_session`, the one answer for the shell's dataset check and the E15-13
  model host to share, refusing a clock past the range, DEC-517); `mandate-marketdata`:
  - `crates/mandate-marketdata/src/model/corporate_action.rs`: splits, dividends, other actions,
    and point-in-time adjustment through `mandate_num::SplitRatio::mark`;
  - `crates/mandate-marketdata/src/alpaca.rs` and `client.rs`: `/v1/corporate-actions`;
  - `crates/mandate-marketdata/src/venue.rs` with `data/sip.venue` and `data/iex.venue`: each
    feed's venue hours;
  - `crates/mandate-marketdata/src/actions.rs`: `corporate-actions.json` next to a dataset;
  - `download.rs`: records the actions of the stored span;
  - `inspect.rs`: `classify` and the actions report.

  `mandate-cli`: `crates/mandate-cli/src/inspect.rs` (classes, adjusted prices, and the action
  list in the report) and `crates/mandate-cli/src/download.rs` (the actions line).
- **Tests:**
  - `crates/mandate-time/tests/session.rs`: typed NYSE closure lists and its own DST rule, and a
    hand-written table of last completed sessions;
  - `crates/mandate-marketdata/tests/venue.rs`: its own 2026 schedule oracle;
  - `crates/mandate-marketdata/tests/inspect.rs`: hand-built datasets, and a per-slot oracle
    for stretches;
  - `crates/mandate-marketdata/tests/corporate_actions.rs`, `actions.rs`, and `download.rs`;
  - `crates/mandate-cli/tests/inspect.rs` and `download.rs`: the exact report lines.
- **Run:** `cargo nextest run -p mandate-time -p mandate-marketdata -p mandate-cli`.
