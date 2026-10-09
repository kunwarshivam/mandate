# Cold-store verification command

- **Spec:** `docs/specs/journal.md` §6.2 (the segment files and manifests the command reads),
  §6.3, §10, §11 (the per-range checks), §12; backlog E5-8; DEC-490 (the command's shape, the
  directory layout and walk order, the refusals, the check order and the report), DEC-263 to
  DEC-265; `docs/project/tasks/E5-8-cold-verify-cli.md`.
- **Code:** `crates/mandate-cli/src/journal/cold.rs` (`mandate journal verify-cold <dir>`: reads
  the directory's `<name>.jsonl` and `<name>.manifest.json` pairs, refuses an incomplete export or
  an unreadable path with a stable code, walks the segments in manifest order through
  `mandate_journal_cold::verify_range`, checks the anchor over the walked rows and the token
  through `verify_tsa`, which never verifies until DEC-265 item 1's crypto half lands, and
  reports the first failure with its code and a non-zero exit); the shared refusals and helpers in
  `crates/mandate-cli/src/journal.rs`.
- **Tests:** `crates/mandate-cli/tests/journal_verify_cold.rs` (every tamper vector a cold export
  can express, replayed against `fixtures/refcases/journal.json`, with the three it re-expresses
  asserted; the segment checks at their `seq`, manifest order, the mid-segment entry, the
  trusted start, the anchor over earlier segments and before the start, the token's two answers
  and its order, the incomplete and unreadable refusals with their codes, artifacts, and the
  exact report); `crates/mandate-cli/tests/binary.rs` (the `mandate`
  binary run as a process: `journal verify` and `journal verify-cold`, each
  printing its report and exiting on the outcome). Planted bugs per test: the task brief.
- **Reference cases:** `journal::tamper::*` in `fixtures/refcases/journal.json`, read directly.
- **Run:** `cargo nextest run -p mandate-cli --test journal_verify_cold --test binary`;
  `cargo xtask ci pending`.
