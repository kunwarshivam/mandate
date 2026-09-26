# Task: E5-4 journal verification and artifact commands

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** E5-4 in the [backlog](../06-backlog-v1.md) (E5, Must; M4's verification tool, deferred
  from the [E5-1](E5-1-journal-core.md) and [E5-2](E5-2-artifact-store.md) briefs)
- **Acceptance criteria (verbatim):** "for each [tamper case](../../specs/reference-cases/journal.yaml)
  its input can express, the command reports the expected first failure; a deleted or altered
  artifact reports `artifact_missing` or `artifact_mismatch` ([journal spec](../../specs/journal.md)
  §11)."
- **PRD / HLD / spec anchors:** journal spec [§6.2](../../specs/journal.md#62-cold-store-segments-and-retention)
  (segment format), [§6.3](../../specs/journal.md#63-artifacts) (artifacts),
  [§10](../../specs/journal.md#10-anchoring) (anchors), [§11](../../specs/journal.md#11-verification)
  (the ordered checks and their codes), [§12](../../specs/journal.md#12-export) (export and
  examination bundles); PRD 6.7 (FR-7.1 to FR-7.7); [HLD](../../HLD.md) journal.
- **Decisions that apply (DEC-NN):** DEC-115 (this story's input contract, below); DEC-107 (the
  artifact store's traits and checked read); DEC-79 (agents land their own work); DEC-80 (no plain
  comments).

## Scope

- **Reference cases that must move from pending to passing:** none. The nine `journal::tamper::*`
  cases already pass on stored rows (E5-1); this story replays the ones an export can express
  through the command, in `crates/mandate-cli/tests/journal_verify.rs`, against the same fixture
  file. `crates/mandate-refcases/status.toml` is untouched.
- **Invariants touched:** none of MI-n or I-n. The claims this story must hold are: the *first*
  failing check of §11's order is the one reported, with the spec's code; identical inputs give
  identical output; any failure exits non-zero; nothing is reported as verified that was not read.
- **Crates in scope:** `mandate-cli`.
- **Crates out of scope:** `mandate-journal` and `mandate-artifacts-fs` (both safety-critical) are
  read, not changed: `verify_events`, `verify_anchor`, `export_line`/`export_segment`,
  `FsArtifactStore`, and `get_artifact` already carry everything the command needs.
  `mandate-refcases` is not touched either; the tests read the fixture directly.
- **New dependencies allowed:** none. `mandate-cli` gains three workspace crates
  (`mandate-journal`, `mandate-artifacts-fs`, `mandate-canon`, the last promoted from a
  dev-dependency) and `serde_json` as a dev-dependency for reading the fixture; no new external
  package, so `docs/dependencies.md` gains no row (the `serde_json` row's "Used by" is extended).
- **Safety-critical:** no. `mandate-cli` is a shell crate (layer 7), so the DEC-77 two-PR flow does
  not apply: tests and code ship in one PR, the tests as its first commit against stubs.
- **Size budget:** about 490 lines of source and 1,400 of tests.

## Data shapes

```rust
pub enum JournalCommand { Verify(VerifyArgs) }

pub struct VerifyArgs {
    pub export: PathBuf,                        // the §6.2 segment file
    pub store: Option<PathBuf>,                 // --store: the artifact store
    pub anchor: Option<PathBuf>,                // --anchor: leaves and root
    pub from_seq: Option<u64>,                  // --from-seq, with
    pub trusted_prev_hash: Option<String>,      // --trusted-prev-hash: the trusted start
}

/// Everything verified, or the one failure the spec reports.
pub enum Outcome { Verified(Option<Span>), Event(EventFailure), Range(RangeCheck) }
pub struct Span { stream_id: String, first_seq: u64, last_seq: u64, last_hash: Digest }
impl Outcome { fn code(&self) -> Option<&'static str>; fn failed(&self) -> bool }

pub fn verify(args: &VerifyArgs, report: &mut impl Write) -> anyhow::Result<Outcome>;

/// Why an input was refused before any check of §11 could run; the code is the message's first word.
pub enum Refusal { TrustedStart, ArtifactStore, Anchor, AnchorStream, ExportStreams, ExportStreamId }
impl Refusal { pub fn code(self) -> &'static str }

pub enum ArtifactCommand { Put(PutArgs), Get(GetArgs) }
pub struct PutArgs { pub file: PathBuf, pub store: PathBuf }
pub struct GetArgs { pub reference: String, pub store: PathBuf }
pub fn put(args: &PutArgs, report: &mut impl Write) -> anyhow::Result<ArtifactRef>;
pub fn get(args: &GetArgs, out: &mut impl Write) -> anyhow::Result<ArtifactRef>;
```

The report is six lines, in this order and nothing else, so two runs over the same bytes are
byte-identical:

```text
export: <path as given>
lines: <n>
trusted start: seq <n>, prev_hash <64 hex>
artifact store: <path as given> | none given
anchor: <path as given> | none given
result: verified, stream <id>, seq <first> to <last>, last hash <64 hex>
```

with `result: verified, no events` for an empty export, `result: failed, seq <n>, <check code>` for
a per-event failure, and `result: failed, <check code>` for a per-range one. The process exits
non-zero whenever `Outcome::failed`, with the same text on standard error.

`mandate artifact put` prints `sha256:<64 hex>` and nothing else; `mandate artifact get` writes the
artifact's bytes to standard output and nothing else.

## Spec clause → test

| Spec clause or invariant | Test name |
|---|---|
| §6.2 the export line is canonical `{"body": <body>, "hash": "<hex>"}`, the body an exact slice | `the_export_line_is_the_one_the_vectors_publish` |
| §6.2 LF separators and a trailing LF | `a_final_line_without_its_line_feed_is_non_canonical`, `carriage_returns_are_not_line_separators` |
| §6.2 a segment is one stream's contiguous `seq` range | `an_export_that_mixes_streams_is_refused` |
| §11 input: a trusted start from a manifest or anchor, never from the file | `a_partial_segment_verifies_from_its_trusted_start`, `a_trusted_start_the_segment_does_not_chain_to_is_prev_hash_mismatch`, `a_trusted_start_out_of_range_or_malformed_is_refused` |
| §11 check 1 `non_canonical` | `whitespace_inserted_and_rehashed_is_non_canonical` (vector `whitespace_inserted_and_rehashed`), `a_line_that_is_not_an_export_line_is_non_canonical_at_the_seq_it_should_hold` |
| §11 check 2 `column_mismatch` | `a_body_missing_an_envelope_field_is_column_mismatch`; the vectors' two column cases: `swapped_seq_members_are_a_seq_gap_because_an_export_carries_no_columns`, `a_column_only_tamper_cannot_be_expressed_by_an_export` |
| §11 check 3 `seq_gap` | `a_deleted_event_is_seq_gap` (vector `event_deleted`) |
| §11 check 4 `rehash_mismatch` | `a_modified_payload_is_rehash_mismatch` (vector `payload_modified`), `a_prev_hash_changed_without_rehashing_is_rehash_mismatch` (vector `prev_hash_changed_without_rehash`) |
| §11 check 5 `prev_hash_mismatch` | `a_prev_hash_rewritten_and_rehashed_is_prev_hash_mismatch` (vector `prev_hash_rewritten_and_rehashed`) |
| §11 check 6 `artifact_missing`, `artifact_mismatch` | `an_event_whose_artifact_is_not_stored_is_artifact_missing`, `an_event_whose_artifact_was_altered_is_artifact_mismatch`, `an_event_whose_artifact_is_stored_and_rehashes_verifies`, `without_a_store_an_event_that_names_an_artifact_is_artifact_missing` |
| §11 "the first failure is reported", in the listed order | `the_first_failure_in_spec_order_is_the_one_reported` |
| §11 per-range `anchor_head_mismatch` | `a_tail_truncated_after_an_anchor_passes_every_per_event_check_and_fails_the_anchor` (vector `tail_truncated_after_anchor`), `a_chain_rewritten_from_seq_3_passes_every_per_event_check_and_fails_the_anchor` (vector `chain_rewritten_from_seq_3`) |
| §11 per-range `anchor_root_mismatch` | `an_anchor_whose_root_does_not_match_its_leaves_is_anchor_root_mismatch` |
| §10 the anchor's leaves and root as `AnchorComputed` records them | `an_untampered_export_passes_the_anchor_it_is_covered_by`, `an_anchor_file_that_is_not_one_is_refused_before_anything_verifies` |
| §11 an anchor is never reported as checked when it cannot cover the export | `an_empty_export_with_an_anchor_is_anchor_head_mismatch`, `an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused`, `an_anchor_covering_more_streams_than_the_export_still_checks_its_own` |
| ES-09 every input refusal names a stable code first | `every_refusal_reports_its_stable_code_first` |
| The report's line count and a failure's `seq` agree | `the_reported_line_count_agrees_with_the_seq_a_failure_names` |
| Every vector case is covered or its inexpressibility is stated | `every_tamper_case_of_the_vectors_is_covered_by_a_test` |
| Replay: identical inputs give identical output | `identical_inputs_give_identical_output` |
| Exit status: any failure is non-zero | `Outcome::failed` asserted in `assert_tamper` and in every failure test |
| The whole report, exactly | `an_untampered_export_verifies_and_the_report_names_every_input` |
| §6.3 the address is SHA-256 of the stored bytes | `put_prints_sha256_of_the_stored_bytes` (FIPS 180-2 vectors) |
| §6.3 putting stored bytes changes nothing | `putting_bytes_that_are_already_stored_changes_nothing` |
| §6.3 every read re-hashes | `get_of_an_altered_object_is_artifact_mismatch` |
| §6.3 the reference form (`sha256:` and 64 lowercase hex) | `get_rejects_anything_that_is_not_an_artifact_reference` |
| A get returns the bytes that went in, and only those | `put_creates_the_store_and_get_reads_the_bytes_back`, `get_writes_the_bytes_and_nothing_else`, `an_artifact_put_and_got_again_is_the_file_that_went_in` |
| Arguments and their help | `verify_takes_the_export_path_and_the_optional_store_and_anchor`, `from_seq_and_the_trusted_prev_hash_are_given_together`, `put_and_get_take_one_path_and_a_required_store` |

## Interpretations (recorded as DEC-115)

1. **The export format is the spec's, unchanged.** Journal spec §6.2 already defines it — JSON Lines
   whose every line is the canonical form of `{"body": <body>, "hash": "<hex>"}`, LF separators and
   a trailing LF — and `mandate_journal::export_line` already writes it. This story adds only the
   reader, so no new export format is defined. The reader is the inverse of `export_line`: it
   strips the `{"body":` prefix and the `,"hash":"<64 hex>"}` suffix and takes the body as an exact
   byte slice, never re-serializing it, exactly as the format's "the body bytes are an exact slice
   of the line" promises.
2. **The reader lives in `mandate-cli`, not beside `export_line`.** Symmetry would put it in
   `mandate-journal`, but that crate is safety-critical and needs no change for this story. If a
   second consumer appears (the cold-store exporter), move it next to `export_line` with its own
   tests.
3. **An export carries no separate columns, so check 2's columns come from the body.** Spec §11
   check 2 compares the stored columns of §6.1 with the body; an export stores only the body and the
   hash. The reader therefore derives the eight columns from the body, and leaves any it cannot read
   at a value the body does not hold (`seq` at the line's expected value, the others empty or 64
   zeros), so a body missing or mistyping an envelope field fails check 2 instead of being accepted.
   Two consequences, both asserted:
   - `column_altered` (the `event_type` column alone changed) **cannot be expressed** by an export:
     its input leaves the exported bytes identical.
   - `seq_values_swapped` (bodies' `seq` swapped, columns left in order) cannot be expressed
     either; the export of the body change alone is reported as `seq_gap` at the line claiming
     seq 3, which is the first failure the checks give for the input an export *can* hold.
4. **A line no canonical body can be read from fails check 1.** Check 1 is "the body parses and
   re-canonicalizes to the same bytes"; a line that is not `{"body":…,"hash":"<64 hex>"}` at all, or
   a final line without its line feed, presents no canonical body where one must be, so it is
   reported as `non_canonical` at the `seq` that line should have held. Every failure the command
   reports is therefore one of the spec's codes.
5. **The trusted start is an argument, never the file.** Spec §11 takes it from a manifest or
   anchor. Manifests are deferred to the cold-store story (E5-1, "Not done"), so `--from-seq` and
   `--trusted-prev-hash` carry it, together or not at all; the default is the genesis start
   (seq 1, 64 zeros). Reading the start from the export's own first line would let a rewritten
   segment vouch for itself.
6. **The anchor file's serialization.** Spec §12 lists anchors in an export but gives them no file
   format. An anchor file is JSON of §4's grammar holding what `AnchorComputed` records:
   `{"leaves":[{"hash":"<64 hex>","seq":<int>,"stream_id":"<id>"}],"root":"<64 hex>"}`, leaves
   sorted by `stream_id` bytes. A file that is not one is refused before anything is reported, so a
   malformed anchor never reads as a pass. Inclusion proofs, timestamp tokens, and the
   `segment_manifest_mismatch`, `segment_gap`, and `tsa_token_invalid` checks stay with the
   cold-store story (E5-1's list).
7. **No `--store` means every reference is missing.** Verification runs without a store, and any
   event naming an artifact then fails `artifact_missing`: the safe direction. A `--store` path that
   is not a directory is refused rather than created, so a typo cannot report an export as verified
   against a store that was never read. `--store` on `artifact put` does create the store, which is
   what a put is for.
8. **A segment covers one stream.** `verify_events` chains by `prev_hash` and `seq` only, so a
   spliced chain whose events belong to different streams would pass. After the per-event checks
   pass, the command refuses an export whose lines do not all carry one `stream_id`, and the report
   names the stream that verified. This is an input refusal, not a §11 code, because §11's
   per-range vocabulary for it (`segment_manifest_mismatch`) belongs to the deferred manifest work.
9. **Range checks run after the per-event checks, in spec order.** An anchor is checked only once
   every per-event check has passed, which is what the vectors' `per_event: pass` expects.
10. **An anchor never vouches for an export it cannot cover.** Both halves of this were false passes
   the independent review of #108 found, each resolving a gap the wrong way:
   - An export holding **no event** holds no anchored head, so with `--anchor` the result is
     `anchor_head_mismatch`, never `verified, no events`. Otherwise a wholly truncated segment
     passes the very anchor that covers its head — `tail_truncated_after_anchor` taken to its limit.
   - An anchor that **names no leaf for the export's stream** is refused
     (`anchor_covers_another_stream`). `verify_anchor` rightly skips the head check when it is asked
     about a stream the anchor says nothing about; the command would otherwise print `verified` with
     `anchor: <path>` in the report, as though the anchor had been checked. An anchor covering more
     streams than the export is fine, and its own leaf is still checked.
11. **Every input refusal carries a stable code first.** `Refusal::code` gives
   `trusted_start_invalid`, `artifact_store_unusable`, `anchor_invalid`,
   `anchor_covers_another_stream`, `export_mixes_streams`, and `export_stream_id_invalid`, as
   `ArtifactError::code` already did for check 6, so a script reads the same word an auditor does
   (ADR-0001 ES-09). The codes are asserted verbatim, because changing one changes what auditors'
   scripts see.

## Planted bugs (each removed after the named test caught it)

| # | Planted bug | Caught by |
|---|---|---|
| F1 | Skip the re-hash on read: `artifact get` returns the stored bytes through `read_artifact` instead of the checked `get_artifact` | `artifact::get_of_an_altered_object_is_artifact_mismatch` |
| F2 | Report a later check instead of the first: the row's hash is recomputed from its body, so check 4 can never fail and a changed payload surfaces as check 5 on the next event | `journal_verify::a_modified_payload_is_rehash_mismatch`, `a_prev_hash_changed_without_rehashing_is_rehash_mismatch`, `identical_inputs_give_identical_output` |
| F3 | Accept a chain with a gap: a row keeps the `seq` its file position implies instead of the one its body states | `journal_verify::a_deleted_event_is_seq_gap`, `swapped_seq_members_are_a_seq_gap_because_an_export_carries_no_columns` |
| F4 | Treat a missing artifact as a mismatch: the no-store source returns `Corrupt` | `journal_verify::without_a_store_an_event_that_names_an_artifact_is_artifact_missing` |
| F5 | Exit zero on failure: `Outcome::failed` counts only per-event failures, so a failing anchor exits 0 | `journal_verify::a_tail_truncated_after_an_anchor_passes_every_per_event_check_and_fails_the_anchor` |
| F6 | Accept a final line without its line feed (treat end-of-file as a separator) | `journal_verify::a_final_line_without_its_line_feed_is_non_canonical` |
| F7 | Take the trusted start from the export's own first line instead of the argument | `journal_verify::a_trusted_start_the_segment_does_not_chain_to_is_prev_hash_mismatch` |
| F8 | Skip the stream-consistency check, so a spliced foreign event is accepted | `journal_verify::an_export_that_mixes_streams_is_refused` |
| F9 | Miss a failure on the last event: the checks stop at the first prefix that fails | `journal_verify::an_event_whose_artifact_is_not_stored_is_artifact_missing` and the two other artifact cases |

The independent review of #108 found four more. Each was reproduced on the pre-fix code after the
fix landed, so the test is shown to fail on the defect it names:

| # | Defect the review found | Caught by |
|---|---|---|
| R1 | An empty export with `--anchor` returns `Verified(None)` before `verify_anchor` runs, so a wholly truncated segment passes its anchor | `an_empty_export_with_an_anchor_is_anchor_head_mismatch` |
| R2 | An anchor naming no leaf for the export's stream is reported as `verified` with the anchor path, as though it had been checked | `an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused`, `every_refusal_reports_its_stable_code_first` |
| R3 | The report's line count skips empty lines, so a blank final line reports `lines: 5` and `seq 6` | `the_reported_line_count_agrees_with_the_seq_a_failure_names` |
| R4 | An input refusal without its stable code | `every_refusal_reports_its_stable_code_first`, `an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused` |

## What the tests cannot catch

- **`column_mismatch` from a real column.** An export has no stored columns, so the check can only
  fire on a body that cannot supply one. The column cases against real rows are
  `mandate-journal-pg`'s (E5-3) and `mandate-refcases`'.
- **The anchor's inclusion proof and timestamp token.** The command checks that the anchor names the
  export's stream, that the anchored head is present with the anchored hash, and that the root
  matches its leaves; it does not check an inclusion proof against a published root, or an RFC 3161
  token. A forged anchor file therefore passes, which is why the anchor must come from outside the
  export.
- **Very large exports.** The whole file and all its rows are held in memory. Segments are bounded
  by the exporter, which this story does not build.

## Not done here (with the story that owns each)

- Segment manifests and the `segment_manifest_mismatch`, `segment_gap`, and `tsa_token_invalid`
  checks, inclusion proofs, and timestamp-token validation: the cold-store and export story
  (E5-1's list).
- Writing exports: `mandate journal export` belongs with the cold store. `export_segment` already
  produces the bytes this command reads.
- Recording a `VerificationRun` event for a command-line run (§11 "results journaled"): the command
  is a read-only auditor tool with no journal to write to; the scheduled verification that journals
  its result belongs to the runtime story.
- Naming the anchor file format in the spec itself: `docs/specs/journal.md` is a protected path, so
  a spec-only change follows with its own DEC citation.
- The examination bundle and the human-readable report of §12.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-cli
cargo nextest run -p mandate-cli --test journal_verify --test artifact
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
```

By hand, over a segment the journal's own writer produced:

```bash
mandate journal verify segment.jsonl --store artifacts --anchor anchor.json
mandate artifact put response.json --store artifacts
mandate artifact get sha256:<64 hex> --store artifacts > response.json
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

None was hit: the spec defines the export format, the verifier's entry point already took stored
rows plus an `ArtifactSource`, and the choices the spec leaves open are DEC-115's nine
interpretations.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails. (No case moves;
      the tamper vectors are replayed through the command against the same fixture file.)
- [x] Tests came first: one commit of tests against stubs, 40 of 46 failing; the 6 that passed are
      argument parsing and assertions about the vector file's shape, which do not call the commands.
      Nine planted bugs and the review's four defects, each caught by the named test above; 67 tests
      pass in `mandate-cli`.
- [x] New state changes emit journal events (none: the commands only read).
- [x] Docs updated: this brief, DEC-115 and its Reserved identifiers row, the feature map, the work
      tracker's E5-4 rows, and the `serde_json` row's "Used by" in `docs/dependencies.md`.
- [x] `cargo xtask check` is green (summary in the PR).
- [x] The PR description is complete (see the PR template).
