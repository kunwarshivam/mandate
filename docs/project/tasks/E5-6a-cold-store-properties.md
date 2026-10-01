# Task: E5-6a the cold store's property suite — the range walk's invariants over random segments

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** [E5-6a](../06-backlog-v1.md) — the property-based invariant tests E5-6 deferred,
  owed by AGENTS.md's "include property-based tests for invariants" and named by #404's review
  (minor 3) and the coordinator's ruling.
- **Acceptance criteria (verbatim):** "proptest drives `mandate-journal-cold` over random
  segment sequences — contiguous runs split at random boundaries, tampered files, lying
  manifests, gaps and overlapping copies, mid-segment entries — and an independent oracle (its
  own accumulator, never the implementation's walk) checks the invariants: a verified range's
  state is the trusted start advanced by exactly the events inside it; every tamper is refused
  by the check that owns it, at the `seq` the range expected; nothing before the trusted start
  is checked; and the token entry point never answers `Ok` (DEC-263 to DEC-265)."
- **PRD / HLD / spec anchors:** journal spec §6.2, §10, §11, §12; E5-6's brief and merged PRs
  ([#391](https://github.com/kunwarshivam/mandate/pull/391),
  [#404](https://github.com/kunwarshivam/mandate/pull/404)).
- **Decisions that apply (DEC-NN):** DEC-263 to DEC-265 (the cold store's design, unchanged),
  DEC-287 (this suite's oracle and generators), DEC-72 (proptest, 256 cases per PR), DEC-77
  (this PR changes tests only; the implementation is #404's, merged).

## Scope

- **Reference cases that must move from pending to passing:** none — properties are not
  reference cases.
- **Invariants touched:** none are changed; the four invariants the suite checks are E5-6's
  own: the verified state (ES-21 determinism), the failing check and its seq (DEC-264's order),
  the reachability rule (§11's "an event before the range's trusted start is not checked by that
  range"), and the fail-closed token entry point (DEC-265 item 1).
- **Crates in scope:** `mandate-journal-cold`'s `tests/` only — the new `properties.rs`.
- **Crates out of scope:** every crate's `src/` — the implementation is #404's, unchanged
  (this PR adds no source line to any crate).
- **New dependencies allowed:** none new to the workspace; `proptest` joins this crate as a
  dev-dependency (already registered in [dependencies](../../dependencies.md), DEC-72).
- **Safety-critical:** yes. This is a **tests-only PR**: DEC-77's flow with nothing to stub,
  because the implementation it exercises is merged.
- **Size budget:** one test file, 7 properties at 256 cases (ES-11).

## The design (DEC-287)

1. **The oracle builds everything the crate reads** (`to_canonical` of its own six-field
   object, `export_segment` of the rows) and computes every expectation from the scenario it
   built, never from the crate's walk: the verified state by advancing the trusted start over
   the walked rows; the failing check and its seq by DEC-264's order over the tamper it knows.
2. **A scenario is a random run** (1 to 12 rows of one stream, real journal hashes from
   `MemoryJournal`), **cut at random boundaries**, entered at the genesis, inside the first
   segment, or exactly at any later segment's first seq with the earlier segments omitted — a
   range may be entered at any supplied segment's first seq — with `prev_hash` the hash of the
   event before `from_seq`.
3. **Every tamper names its owner** (DEC-264, and DEC-263 item 5 for the canonical form): a
   flipped file byte or a lying manifest field fails `segment_manifest_mismatch` at the
   manifest's own claimed `first_seq`; bytes that are not a manifest — or the six fields in a
   non-canonical member order, which only `parse`'s canonical-bytes guard can refuse — fail it
   at the seq the range expected; a dropped segment or a stale overlapping copy fails
   `segment_gap` at the expected seq — the copy may be prepended in front of the trusted start,
   where one ending before the entry is refused at the start itself by the first segment's
   upper carries bound alone; and a row whose hash is a lie, with the parts rebuilt
   from the lied row so every segment check passes, fails the per-event check 4 and surfaces
   as the `Event` arm unchanged. The tamper's indices are drawn against the very scenario
   they corrupt.
4. **Reachability**: swapping the first two rows' hashes leaves a range entered at 3 or later,
   with the whole run supplied, verifying, because the walk starts at `from_seq` — the parts
   are rebuilt from the swapped rows so the export stays self-consistent and only the rule can
   save it.
5. **The token's two answers** are pinned as a property: `verify_tsa` is never `Ok`; it is
   `TsaVerificationIncomplete` exactly when the token contains the imprint, by the suite's own
   containment search, and `TsaTokenInvalid` otherwise.
6. **The proof's root** is pinned as a property: an anchor's inclusion proof, folded with the
   oracle's own §10 walk (the leaf's side at each level from the split rule, then the siblings
   combined leaf-upward on the far side), rebuilds the anchor's root — so swapped or wrong-length
   siblings fail it.
7. **The oracle's chain rule is derived from DEC-264's wording**, not from the
   implementation's expression: "each begins where the previous ended, exactly, in both
   directions", and the first "carries the trusted start" — read as the start's `from_seq` not
   sitting outside the first segment's span.
8. **The do-nothing check still applies**: with every entry point's body replaced by a
   refusal or a constant — the codes and the accessor included — 0 of the 7 properties pass,
   for every one of the 37 flavours; the one test an identity `verify_range` answers is
   `cold::an_empty_range_returns_the_trusted_start` (#391's test of the degenerate case, whose
   subject is exactly that answer), so no test this PR adds passes under any flavour.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-journal-cold
```

## Interpretations (the readings this brief takes)

1. The properties are live, not pending: the implementation exists, so DEC-77's stage-1 markers
   have nothing to pin — the suite is a tests-only PR in DEC-77's flow (the coordinator's
   ruling).
2. A tamper the generator offers must fail the oracle's walk by construction, so the failure
   property's expectation is always an `Err` — the positive direction is pinned by its own
   property, and the two together leave no constant answer that passes.
3. The stale copy is built from the run's own first rows, so its manifest and file agree with
   each other and only the segment chain can refuse it — the same shape #391's hand test pins.
4. The oracle's containment search, not the crate's, decides whether a token "contains" the
   imprint, so the structural check is compared against an independent reading of containment.

## Not done

- The crypto half of the token check (DEC-265 item 1, Proposed): the property pins the
  fail-closed refusal, which is all that can be checked without a crypto dependency.
- The object-storage client, the retention job, the second-region replica, the CLI wiring, and
  the examination bundle: E5-6's own Not done list carries them.
