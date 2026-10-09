# Audit causal trace (E12-1)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 "Causal trace" (J2, FR-7.2), AU-1, AU-3, AU-4,
  API-18; DEC-761 (the closed link table), DEC-762 (bounds and statuses), DEC-772 (what §4.8.1
  leaves open), DEC-773 (Proposed: who quoted output is attributed to).
- **Code:** `crates/mandate-audit/src/trace.rs`: `TraceRead` over `MemoryRead` and the response
  types; the walk follows `causation_id`, `ModelOutputRecorded`'s `payload.evidence[]`, and
  `IntentReceived`'s `payload.intent_id`, every lookup through the workspace-scoped reads. A link
  at a bound is decided from the trace's own nodes and reads no stream (DEC-772 items 2 and 6).
- **Slice A2 follows only those three links.** A trace from a fill, an order request, or any event
  whose only links are the other rows of §4.8.1's table ends without hops until slice A2b. No
  endpoint may serve the trace before A2b lands.
- **Tests:** `crates/mandate-audit/tests/trace.rs`: an independent breadth-first
  walk over random `causation_id` and `payload.evidence[]` graphs of two workspaces, cycles and
  self-links, forged links into another workspace, the depth, event and hop bounds at and one past
  each (an absent or wrong-type target past a bound is `beyond_bound`), model text only quoted, and
  `intent_id` lookups on the named streams, with an empty "every `GateDecided`" as `not_recorded`
  and `as_of` covering a stream read with no match; at the depth or event bound the `intent_id` row
  reads its `IntentProposed` `already_shown` or `beyond_bound` and adds no agent stream to `as_of`.
  The other link rows are slice A2b (backlog).
- **Run:** `cargo nextest run -p mandate-audit -E 'binary(trace)'`.
