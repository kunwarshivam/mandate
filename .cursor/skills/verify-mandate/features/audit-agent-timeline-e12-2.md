# Audit agent timeline (E12-2)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 "Timeline" (J1, FR-7.3), AU-1, AU-5; DEC-760
  (bounds, the one 404), DEC-764 (which account-stream events are the agent's, the k-way merge, one
  cursor per stream, the 10,000-event cap), DEC-777 (the merge over raw heads, where a page stops,
  `more`, `next`, membership lookups in the page's snapshot), DEC-778 (a `client_order_id` is the
  agent's only through a version-2 `OrderSubmitted`'s `causation_id`).
- **Code:** `crates/mandate-audit/src/timeline.rs`: `TimelineRead` over `MemoryRead` in
  `mandate-audit`. The agent's stream and its account streams, all through the workspace-scoped
  reads, are merged by `(recorded_at, stream_id)` over every stream's next unconsumed event; filters
  serve or skip but every event walked is consumed; a page stops after its `limit`-th served event
  or when one stream reaches 10,000 consumed.
- **Tests:** `crates/mandate-audit/tests/timeline.rs`: a clock step back, excluded events consumed,
  exact `limit`, the per-stream cap, a merge-oracle fuzz of every walk under appends, foreign and
  unknown agents and cursors as the one `NotFound`, the DEC-764 membership rules, lookups read only
  the page's snapshot, and version-2-only order links across timeline streams.
- **Run:** `cargo nextest run -p mandate-audit -E 'binary(timeline)'`.
