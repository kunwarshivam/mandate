# Version backtest and forward-paper evaluation

- **Spec:** DEC-1000 (the version backtest runs the live decision path on recorded outputs and
  theses, no look-ahead by receive time), DEC-1001 (the report artifact at `report_version` 2 and
  `BacktestRunRecorded`), DEC-1002 (the forward-paper harness, one scorecard per signal model,
  `EvaluationRunRecorded`), DEC-1003 (disclosures as placeholder names, Proposed);
  `docs/specs/workspace-api.md` §4.2 and §4.7 (`read-models/backtest`, `read-models/evaluation`);
  `docs/specs/journal.md` §9.18; `docs/specs/trading-domain.md` §6.4 (the fill model's slippage
  past the bar); PRD FR-4.1, FR-4.2, FR-4.5; backlog E4-4, E4-5, E17-8, E15-3.
- **Code:** `mandate-backtest`: `crates/mandate-backtest/src/version.rs` (the walk, the decision
  path in mandate spec §6.2's order through `mandate-spec`'s risk state, `mandate-risk`'s flatten,
  trim and gate, `mandate-builder`'s `propose` and `classify`, and `mandate-executor`'s protection),
  `crates/mandate-backtest/src/version/gate.rs` (the mandate and risk state in the gate's
  vocabulary), `crates/mandate-backtest/src/version/report.rs` (the report and its digests),
  `crates/mandate-backtest/src/version/record.rs` (`BacktestRunRecorded`'s payload and the
  configuration digest). `mandate-evaluation`: `crates/mandate-evaluation/src/lib.rs` (the record
  check, the open, closed and outside-window sort, the round-trip cost, the grouping, and
  `mandate_research::score::evaluate` per group), `crates/mandate-evaluation/src/report.rs`,
  `crates/mandate-evaluation/src/record.rs`.
- **Tests:** `crates/mandate-backtest/tests/version.rs` (one case per DEC-1000 and DEC-1001 item,
  the golden report `crates/mandate-backtest/tests/golden/version-report.json`),
  `crates/mandate-backtest/tests/version_properties.rs` (purity, fills inside their bars as §6.4
  allows, no look-ahead, no unanswered ask trades, no exit denied and no risk exit held; each
  oracle shown to fail on a seeded bug), `crates/mandate-evaluation/tests/evaluation.rs` (a
  hand-calculated scorecard row, open and outside-window counts, refused theses scored, one card
  per model version, a rejected record), `crates/mandate-evaluation/tests/properties.rs`.
- **Reference cases:** none yet; the records' vectors are E4-5's and E17-8's registration.
- **Run:** `cargo nextest run -p mandate-backtest -p mandate-evaluation`.
