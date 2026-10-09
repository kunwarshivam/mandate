# Audit causal trace (E12-1)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 "Causal trace" (J2, FR-7.2), AU-1, AU-3, AU-4,
  API-18; DEC-761 (the closed link table), DEC-762 (bounds and statuses), DEC-772 (what §4.8.1
  leaves open).
- **Code:** `crates/mandate-audit/src/trace.rs`: `TraceRead` over `MemoryRead` and the response
  types. Stubs until the E12-1 implementation lands.
- **Tests:** `crates/mandate-audit/tests/trace.rs`, pending E12-1: cycles and self-links, and
  forged links into another workspace.
- **Run:** `cargo nextest run -p mandate-audit --run-ignored only -E 'binary(trace)'`.
