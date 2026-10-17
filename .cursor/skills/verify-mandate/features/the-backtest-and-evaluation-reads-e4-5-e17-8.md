# The backtest and evaluation reads (E4-5, E17-8)

- **Spec:** [workspace API spec](../../../../docs/specs/workspace-api.md) §4.2 (`GET
  /backtests/{id}`), §4.7 (`GET /evaluations/{id}`), API-2, API-9, API-14;
  [journal spec](../../../../docs/specs/journal.md) §9.18 (`BacktestRunRecorded`,
  `EvaluationRunRecorded`, rule 143); DEC-1001 item 5, DEC-1002 items 6 and 7, DEC-103, DEC-1143
  (fail closed: the internal paper workspace setting, a completed run without its report, a
  duplicate id, the permission row).
- **Code:** `crates/mandate-api-server`. `src/backtests.rs` (the pure folds: the one record naming
  an id, by equality, or `Unreadable` for a duplicate or a record failing a member it serves from; the report
  object's `kind`, `report_version`, and mandate version; a control stream shown `paper`),
  `src/routes.rs` (`one_backtest`, `one_evaluation`: `reader()`'s authentication and the view row,
  the report read from the store and digest-checked through `src/documents.rs`'s `checked`, the
  one 404, `journal_unavailable` for a report the read cannot serve, and the evaluation's 404
  unless the deployment is marked internal and its stream is paper), `src/config.rs`
  (`MANDATE_API_INTERNAL_PAPER_WORKSPACE` and its start check), `src/members.rs`
  (`Hosted::internal_paper`), and the binary's start check, which reads the hosted control stream
  within five seconds.
- **Tests:** `tests/backtests.rs`: the folds against hand-built §9.18 rows (completed, refused,
  equality only, the id grammar, a duplicate, every malformed member, an unparsable body), the
  reports against the schemas' own examples, and the setting's values and start check.
  `tests/backtests_http.rs`: both routes' bodies equal to the schemas' examples, with the members
  each schema requires and allows read from the schema files; the one 401, the 403 without the view
  row, the same 404 for an absent id, another workspace, and no membership; 503 for an absent,
  corrupt, or mismatched report, no store, a duplicate record, and an unreadable stream; the
  evaluation's 404 with the setting off (a recorded id included) and on a stream not paper
  throughout. `tests/process.rs`: the setting refused at start on a journal not shown paper, a
  malformed value refused, and `false` or empty starting. `tests/backtest_rows/mod.rs` builds the
  rows directly, since `mandate-journal` does not register either record yet.
- **Run:** `cargo nextest run -p mandate-api-server -E 'binary(backtests) | binary(backtests_http) | binary(process)'`.
