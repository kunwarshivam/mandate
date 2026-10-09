# Journal verification and artifact commands

- **Spec:** `docs/specs/journal.md` §6.2 (the segment format the command reads), §6.3, §10, §11,
  §12; backlog E5-4; DEC-115; `docs/project/tasks/E5-4-verification-cli.md`.
- **Code:** `crates/mandate-cli/src/journal.rs` (`mandate journal verify`: reads a segment file back
  into stored rows, runs `verify_events`, then the range checks of the stream's type
  (`stream_checks`, DEC-782): on a control stream `verify_anchor_self` and
  `verify_break_glass_causes`, on an agent stream `verify_agent_stream_anchored` with no hold
  anchor; then `verify_anchor`, reports the first failure with its spec check code and a non-zero
  exit; `verify-cold` in `journal/cold.rs` runs the same `stream_checks` after its cold walk),
  `crates/mandate-cli/src/artifact.rs` (`mandate artifact put` and `get` over `FsArtifactStore`,
  the get re-hashed through `get_artifact`). The checks themselves are `mandate-journal`'s and
  `mandate-artifacts-fs`', unchanged by this story.
- **Tests:** `crates/mandate-cli/tests/journal_verify.rs` (every tamper vector an export can
  express, replayed through the command against `fixtures/refcases/journal.json`, with the two it
  cannot asserted as inexpressible; the exact report; malformed lines, trusted starts, anchors,
  mixed streams, artifacts present, absent, and altered), `crates/mandate-cli/tests/artifact.rs`
  (the FIPS 180-2 addresses a put prints, the round trip, the re-hash on read, reference forms),
  `crates/mandate-cli/tests/journal_verify_ranges.rs` (E12-3, DEC-782: the control stream's
  `anchor_self_mismatch` and `break_glass_cause_mismatch`, wired into `verify` and `verify-cold`
  after checks 1 to 6 and before the anchor and token checks, from the `cold_records` and
  `records_access` range-check vectors),
  `crates/mandate-cli/tests/journal_verify_agent_ranges.rs` (E12-3 W2, DEC-782: the agent stream's
  `intent_action_mismatch`, `mode_event_mismatch` and `held_mismatch` through both commands with no
  hold anchor, so a tail range fails closed, from the `agent_stream` and `hold` range vectors).
  The control, agent and account streams' checks are wired (DEC-782, DEC-890).
  `crates/mandate-cli/tests/journal_verify_connection_ranges.rs` (E12-3 W3, DEC-782 item 5,
  DEC-885, DEC-890: `connection_lifecycle_mismatch` on a control or account stream through both
  commands, a range from `seq` 1 judged as that stream's full chain and a tail range failing closed
  with no connection anchor, at the lowest `seq` with the control checks and before the anchor and
  token; no single-stream export runs `connection_cause_mismatch`, and the report says so in
  DEC-890's `not run:` line just before `result:`, which no agent stream, connection-free export,
  or run stopped at checks 1 to 6 prints; from the `connections`, `connection_requests` and
  `connection_ranges` sequences).
  Planted bugs per test: the task brief.
- **Reference cases:** `journal::tamper::*` and `journal::export_line_seq_1` in
  `fixtures/refcases/journal.json`, read directly rather than through `mandate-refcases`.
- **Run:** `cargo nextest run -p mandate-cli --test journal_verify --test journal_verify_ranges --test journal_verify_agent_ranges --test journal_verify_connection_ranges --test artifact`.
