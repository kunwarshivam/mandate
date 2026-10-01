# Task: E5-6 the journal cold store — segments, manifests, and the per-range verify checks

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** E5-6, minted by claim [#374](https://github.com/kunwarshivam/mandate/issues/374) —
  the cold-store follow-up the tracker's "Next, in order" item 5 names: segment manifests, the
  `segment_*` and `tsa_token_invalid` checks.
- **Acceptance criteria (verbatim):** [journal spec §6.2](../../specs/journal.md) — "A manifest
  (canonical JSON: stream, first and last `seq`, first `prev_hash`, last `hash`, file SHA-256) is
  hashed and referenced by a `SegmentExported` event"; §11 — the per-range checks
  "`segment_manifest_mismatch`, `segment_gap`" and "`tsa_token_invalid`", over "a stream range
  with a trusted start (`from_seq`, `trusted_prev_hash`) taken from a manifest or anchor", where
  "a reference to an event before the range's trusted start is not checked by that range"; §12 —
  "Canonical export: segment files, manifests, the relevant anchors with inclusion proofs,
  timestamp tokens with certificate chains, and the verifier's digest."
- **PRD / HLD / spec anchors:** journal spec §6.2 (segments and manifests), §10 (the anchor tree
  and the RFC 3161 imprint), §11 (verification), §12 (export); DEC-76 (the nightly's checks),
  E5-1/E5-2/E5-4 (the merged journal stories this follows).
- **Decisions that apply (DEC-NN):** DEC-263 (this brief's crate and manifest form), DEC-264
  (the segment checks' order and reachability), DEC-265 (the TSA structural scope and the export
  digest), DEC-85 (fail loud, never a silent guess), DEC-253 item 2 (the live pin for the codes).

## Scope

- **Reference cases that must move from pending to passing:** none — journal.yaml's vectors cover
  the hot store's checks (E5-1); the cold checks are new pure behaviour with no case family.
- **Invariants touched:** ES-21 (determinism: the manifest and the digest are byte-exact
  functions of their inputs; no clock, no randomness), ES-09 (no new journaled reason code: the
  cold check codes are the spec's own §11 words), the §11 rule that nothing before the trusted
  start is checked.
- **Crates in scope:** the new `mandate-journal-cold` (layer 3, `safety_critical = true`,
  `pure = true`, `allowed_external = ["thiserror"]`, over `mandate-canon` and `mandate-journal`),
  with its `xtask/layers.toml` entry, CODEOWNERS line and lint header as normal changes in this
  PR (AGENTS.md: a new crate gets them in the same change).
- **Crates out of scope:** `mandate-journal` (the hot store; the cold crate *uses* its
  `verify_events`, `export_segment`, `Anchor`, `merkle_root`, `tsa_imprint` and never holds a
  second copy of a check), `mandate-journal-pg` (the Postgres store), `mandate-cli`, and every
  crate on the frozen list.
- **New dependencies allowed:** none (thiserror is already a workspace dependency). Full RFC 3161
  signature verification would need a crypto dependency and is Proposed in DEC-265, not taken;
  if the founder approves it later, that PR adds the `docs/dependencies.md` row with its reason.
- **Safety-critical:** yes. This task is the **tests PR** of the two-PR flow.
- **Size budget:** the module's non-test surface at implementation is expected under 400
  non-test lines: the manifest, the importer, the range walk, the imprint containment, the proof
  walk, the digest.

## The design (DEC-263 to DEC-265)

1. **A new pure crate, not a wing of the hot store** (DEC-263): every piece of the slice — the
   manifest form, the segment checks, the export digest — is deterministic arithmetic over bytes
   and digests, and the Postgres crate stays the hot store. Layer 3 is the lowest layer above
   `mandate-journal` (2) and `mandate-canon` (0).
2. **The manifest's six fields and their JSON names are pinned** (DEC-263): `stream`,
   `first_seq`, `last_seq`, `first_prev_hash`, `last_hash`, `file_sha256`, canonicalized by the
   same rules as every journal byte. The spec names the fields in prose; the names are this
   design's, pinned by a test that builds the object itself. The manifest hash a
   `SegmentExported` event references is the digest of exactly these bytes.
3. **`of` derives, `parse` reads, and both refuse** (DEC-263): `of` takes one contiguous
   ascending run of one stream — no rows, a skipped `seq`, or a second stream is a typed error,
   never a guess (DEC-85). `parse` accepts only canonical bytes of exactly the six fields.
4. **The importer splits, it does not judge** (DEC-263): `import_line` reads
   `{"body": <canonical>, "hash": "<64 hex>"}` back to the event without re-hashing. Whether the
   hash is true is §11 check 4's question; an importer that rejected the lie would take the
   check's job and leave the verifier nothing to verify.
5. **The range walk's order and reachability** (DEC-264): for each segment, in order —
   the manifest must parse (a manifest that is not a manifest is a `segment_manifest_mismatch`
   at the seq the range expected), the file's SHA-256 and its edges (the first and last lines'
   own `seq`, `prev_hash` and `hash`) must match the manifest (`segment_manifest_mismatch` at
   the manifest's `first_seq`), the segment must begin where the previous ended and the first
   must carry the trusted start — beginning exactly there or covering it, because a range may
   enter mid-segment (`segment_gap` at the seq the range expected), and then the events are
   walked by `mandate_journal::verify_events` from the trusted start, whose per-event failures
   surface unchanged. An event before the trusted start is not checked by that range (§11's own
   rule), and the walk honours it by starting at `from_seq`, never at the segment's first event.
6. **The TSA check's structural scope** (DEC-265): `tsa_token_invalid` fires unless the token
   artifact contains the anchor's imprint — SHA-256 of the 32 raw root bytes (§10,
   `tsa_imprint`) — so a token that does not even claim this anchor's root fails. Containing
   the root itself is not containing the imprint. Signature verification, the chain, and
   revocation are the crypto half: Proposed, because each needs a dependency this crate does not
   carry.
7. **The inclusion proof lists sibling subtree roots** (DEC-265): `inclusion_proof` returns, from
   the leaf's level upward, the root of each sibling subtree, so a verifier rebuilds §10's tree
   by its own split rule with the leaf's index. For three leaves `a b c`, `a`'s proof is
   `[leaf_hash(b), root([c])]` and `c`'s is `[root([a b])]`. A stream the anchor does not cover
   is the named refusal `StreamNotAnchored`, never a silent `None` (DEC-85), and an anchor whose
   leaves do not produce its root is `MalformedAnchor`.
8. **The verifier's digest is length-prefixed** (DEC-265): §12's "the verifier's digest" is one
   digest over the bundle's parts — each segment's manifest bytes then file bytes, each anchor's
   root bytes, each token's bytes, in order — with every part prefixed by its byte length as a
   `u64` in little-endian, so no two different part lists can concatenate to the same bytes.
9. **The codes are the spec's words** (ES-09): `segment_manifest_mismatch`, `segment_gap`,
   `tsa_token_invalid`, pinned by a live in-module test because the mutation gate runs only live
   tests while the crate holds pending ones (DEC-253 item 2).

10. **The stubs report the crate's own `Unimplemented`** (DEC-77, DEC-137): `ColdError::Unimplemented`
    for the manifest and the line, `ColdFailure::Unimplemented` for the range walk — so every
    pending test's failure carries its stub's report, and the implementation PR removes both arms
    when the last stub goes. The token test's anchor derives from a segment manifest — §10 anchors
    stream heads, which the manifest's own edges name — so it exercises the cold store end to end
    and stops at the manifest's stub.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-journal-cold
cargo xtask ci pending
```

## Interpretations (the readings this brief takes)

1. §6.2's manifest prose names the fields; their JSON keys are this design's (item 2), recorded
   in DEC-263 and pinned by the test that builds the object itself.
2. "A manifest is hashed" — the manifest's own bytes, not the file's: the file's SHA-256 is
   already a field, and the `SegmentExported` reference must change when any field changes,
   which hashing the manifest's bytes gives and hashing the file does not.
3. A range may begin inside a segment (§11's trusted start "taken from a manifest or anchor"
   points at a chain edge, not a segment boundary), so the first segment's own head is not
   checked by that range, and a tampered event before the trusted start must not fail it.
4. A gap is reported at the seq the range expected, not the seq the segment claims: the failure
   names where the chain broke, which only the range knows.
5. An unparsable manifest is a `segment_manifest_mismatch`, not a new error code: §11's list is
   closed, and a manifest that is not a manifest fails the only check it can.
6. The token's structural check reads the artifact's bytes as stored; the artifact's existence
   and content hash are the per-event artifact checks' business (§11 checks 6 and 7), not the
   TSA check's.
7. The export bundle's anchors enter the digest by their roots: the leaves are a function of the
   heads and the root, and §12's digest is over the export's parts, not a re-serialization of
   the anchor type.

## Planted bugs (each tried against a throwaway implementation; a named test must catch each)

| # | Bug | Caught by |
|---|---|---|
| 1 | `of` takes the first row's `hash`, not its `prev_hash` | `a_manifest_of_one_contiguous_run_names_its_edges` |
| 2 | `of` hashes the lines without the trailing LF | `a_manifest_of_one_contiguous_run_names_its_edges` (the test's own digest is of the file as `export_segment` writes it) |
| 3 | `of` skips the contiguity validation | `rows_not_in_one_ascending_run_are_refused` |
| 4 | `of` ignores the stream column | `rows_of_two_streams_are_refused` |
| 5 | `to_canonical_bytes` names a field differently (for example `sha256`) | `the_manifest_s_bytes_are_the_six_fields_in_canonical_form` |
| 6 | `parse` re-canonicalizes silently instead of refusing non-canonical bytes | `a_manifest_in_the_wrong_member_order_is_refused` |
| 7 | `parse` accepts a seven-field object, ignoring the extra | `a_manifest_in_the_wrong_member_order_is_refused` (its third case feeds a seventh member) |
| 8 | `import_line` re-hashes and rejects the lie | `a_wrong_hash_imports_and_the_event_check_catches_it` |
| 9 | `import_line` re-orders the members, accepting swapped bytes | `a_line_in_the_wrong_member_order_is_refused` |
| 10 | `verify_range` checks the file's SHA-256 but not the edges | `a_manifest_whose_edges_disagree_with_its_file_fails_the_mismatch_check` |
| 11 | `verify_range` chains from the first segment's own `first_seq`, not the trusted start | `a_first_segment_that_does_not_carry_the_trusted_start_fails_segment_gap` |
| 12 | `verify_range` walks events from the segment's first event, not `from_seq` | `an_event_before_the_trusted_start_is_not_checked` |
| 13 | `verify_range` reports a gap at the offending manifest's `first_seq` | `a_gap_between_segments_fails_segment_gap_at_the_expected_seq` |
| 14 | `verify_tsa` checks containment of the root bytes, not the imprint | `a_token_verifies_only_with_the_anchor_s_imprint` |
| 15 | `inclusion_proof` returns the leaf hashes, not the sibling subtree roots | `an_inclusion_proof_lists_the_sibling_roots_and_rebuilds_the_root` |
| 16 | `verifier_digest` concatenates without the length prefixes | `the_export_digest_follows_the_parts_and_changes_with_them` |
| 17 | a cold check code drifts from §11's words | `the_cold_check_codes_are_pinned` (live) |

## Not done

- The implementation PR (DEC-77 stage 2): the manifest, the walk, the imprint containment, the
  proof walk, and the digest — the throwaway this PR's bugs were tried against is ready to
  become it once this merges.
- Full RFC 3161 signature verification, the certificate chain, and revocation: Proposed in
  DEC-265 item 6, with the dependency row that PR will need.
- The object-storage client, the retention job, and the second-region replica (§6.2's
  operational halves): the store's mechanics, not the pure checks; a later story.
- Wiring the checks into `mandate journal verify` (the CLI is another stream's crate; the cold
  crate exposes the checks and the CLI calls them when its story says).
- The examination bundle (§12's second bullet): a later story; this one ships the canonical
  export's parts and digest.
- The property suite (proptest) lands with the implementation PR.
