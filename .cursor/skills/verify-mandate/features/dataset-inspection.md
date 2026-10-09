# Dataset inspection

- **Spec:** backlog E2-2; trading domain spec §4.1, §4.2; DEC-89;
  `docs/project/tasks/E2-2-inspect.md`; the data-quality warnings of
  `docs/project/tasks/inspect-quality.md`.
- **Code:** `crates/mandate-marketdata/src/inspect.rs` (coverage with closed-day counts, exact
  statistics, gaps between bars, duplicates, the `Quality` warnings, untrusted partitions),
  `read_manifest` in `crates/mandate-marketdata/src/dataset.rs`; `crates/mandate-cli/src/inspect.rs`
  (`mandate inspect`, the text report).
- **Tests:** `crates/mandate-marketdata/tests/inspect.rs` (hand-built datasets through the real
  store; a slot-grid oracle for gaps), `crates/mandate-marketdata/tests/inspect_quality.rs`
  (warnings and inconsistent bars over Alpaca records from the M1 rehearsal),
  `crates/mandate-cli/tests/inspect.rs` (the exact report).
- **Run:** `cargo nextest run -p mandate-marketdata -p mandate-cli inspect`.
