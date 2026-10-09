# Concurrent dataset writes

- **Spec:** DEC-89 (compare-before-write, never replace a stored partition);
  `docs/project/tasks/marketdata-write-safety.md`.
- **Code:** `Store::put_day` and its write helpers in `crates/mandate-marketdata/src/dataset.rs`
  (advisory lock on the dataset directory, unique temporary names, hard-link publish).
- **Tests:** `crates/mandate-marketdata/tests/concurrent_writes.rs` (threads and processes, leftover
  temporary files).
- **Run:** `cargo nextest run -p mandate-marketdata --test concurrent_writes`.
