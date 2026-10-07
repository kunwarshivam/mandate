# Task: E5-8 `mandate journal verify-cold` over a cold-store export

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** E5-8 in the [backlog](../06-backlog-v1.md) (E5, Should; the CLI wiring of E5-6's
  checks, which the [E5-6 brief](E5-6-journal-cold-store.md) left to "the CLI calls them when its
  story says"); claim [#645](https://github.com/kunwarshivam/mandate/issues/645).
- **Acceptance criteria (verbatim):** "the command runs §11's per-range checks through
  `mandate-journal-cold` over a cold export, reports the first failure with its code and a
  non-zero exit, never vouches for an export it cannot cover, and never answers `Ok` for a
  timestamp token until DEC-265 item 1's crypto half lands."
- **PRD / HLD / spec anchors:** journal spec [§6.2](../../specs/journal.md#62-cold-store-segments-and-retention)
  (segments and manifests), [§6.3](../../specs/journal.md#63-artifacts),
  [§10](../../specs/journal.md#10-anchoring) (the anchor and the RFC 3161 imprint),
  [§11](../../specs/journal.md#11-verification) (the per-event and per-range checks and their
  codes), [§12](../../specs/journal.md#12-export) (the canonical export); PRD FR-7.5;
  [HLD](../../HLD.md) journal.
- **Decisions that apply (DEC-NN):** [DEC-490](../decisions/DEC-490.md) (this story's command
  shape, directory layout, refusals, check order and report); DEC-263 to DEC-265 and DEC-287
  (the cold crate's manifest, walk order, token entry point and property suite); DEC-115 (E5-4's
  input contract, whose flags, refusal codes and report shape this command reuses); DEC-77 and
  DEC-137 (the tests PR and the stub's report); DEC-79, DEC-80.

## Scope

- **Reference cases that must move from pending to passing:** none. The journal vectors'
  `tamper_cases` are replayed through a cold export as E5-4 replays them through a segment file,
  against the same fixture file (`fixtures/refcases/journal.json`); `status.toml` is untouched.
- **Invariants touched:** none of MI-n or I-n. The claims this story must hold: the first failing
  check in range order (DEC-264) is the one reported, with its code; any failure exits non-zero;
  nothing is reported verified that was not walked (an incomplete directory, an unreadable path,
  an anchored head outside the range); a token never verifies; identical inputs give identical
  output.
- **Crates in scope:** `mandate-cli` (`crates/mandate-cli/src/journal/cold.rs`, the `Refusal`
  codes and the shared helpers in `crates/mandate-cli/src/journal.rs`, the `main.rs` arm).
- **Crates out of scope:** `mandate-journal-cold` and `mandate-journal` (both safety-critical) are
  called, not changed: `verify_range`, `verify_tsa`, `import_line`, `SegmentManifest::parse`,
  `verify_anchor`. `mandate-shell`, `mandate-executor`, `mandate-runtime` and `web/` are not
  touched.
- **New dependencies allowed:** none. `mandate-cli` gains the workspace crate
  `mandate-journal-cold`; no external package, so `docs/dependencies.md` gains no row.
- **Safety-critical:** yes (`mandate-cli`, `xtask/layers.toml`). This brief covers the **tests
  PR** of the DEC-77 sequence; the implementation PR follows on `claude/hejxo4-e5-8-impl-2` ([#662](https://github.com/kunwarshivam/mandate/pull/662)) and
  changes the test file only by deleting its `#[ignore = "pending E5-8"]` lines.
- **Size budget:** about 330 lines of source (the stubs and types in the tests PR, the
  implementation after) and 2,000 of tests.

## Data shapes

```rust
pub enum JournalCommand { Verify(VerifyArgs), VerifyCold(cold::VerifyColdArgs) }

pub struct VerifyColdArgs {
    pub export: PathBuf,                   // the cold export directory (DEC-490 item 2)
    pub store: Option<PathBuf>,            // --store: the artifact store
    pub anchor: Option<PathBuf>,           // --anchor: leaves and root
    pub token: Option<PathBuf>,            // --token: the anchor's RFC 3161 token, requires --anchor
    pub from_seq: Option<u64>,             // --from-seq, with
    pub trusted_prev_hash: Option<String>, // --trusted-prev-hash: the trusted start
}

/// Everything verified, or the one failure reported first.
pub enum ColdOutcome { Verified(Span), Cold(ColdFailure), Range(RangeCheck) }
impl ColdOutcome { fn code(&self) -> Option<&'static str>; fn failed(&self) -> bool }
pub const TSA_VERIFICATION_INCOMPLETE: &str = "tsa_verification_incomplete";

pub fn verify(args: &VerifyColdArgs, report: &mut impl Write) -> anyhow::Result<ColdOutcome>;

/// E5-4's refusals, plus this command's two: the code is the message's first word.
pub enum Refusal { …, Unreadable, ColdExportIncomplete }
```

The report is seven lines, in this order and nothing else (DEC-490 item 7):

```text
export: <directory as given>
segments: <n>
trusted start: seq <n>, prev_hash <64 hex>
artifact store: <path as given> | none given
anchor: <path as given> | none given
token: <path as given> | none given
result: verified, stream <id>, seq <first> to <last>, last hash <64 hex>
```

with `result: failed, seq <n>, <code>` for a per-event failure and a segment check, and
`result: failed, <code>` for the anchor's and the token's checks. The process exits non-zero
whenever `ColdOutcome::failed`, with the result text on standard error. A refusal is reported
through the error, code first, and writes no report line.

## Spec clause → test

| Spec clause or invariant | Test name |
|---|---|
| DEC-490 item 1: the command line, `--token` requires `--anchor`, the trusted start comes as a pair | `verify_cold_takes_the_export_directory_and_the_optional_inputs`, `a_token_needs_its_anchor_and_the_trusted_start_comes_as_a_pair` |
| DEC-490 items 6 and 7: the outcome codes and result lines, pinned | `the_cold_outcome_codes_and_result_lines_are_pinned`, `the_cold_refusal_codes_are_pinned` |
| §6.2 the manifest the oracle reads off a file is the one the crate derives | `the_oracle_s_manifest_is_the_one_the_cold_crate_derives` |
| Every vector case is covered or its re-expression stated | `every_tamper_case_of_the_vectors_is_covered_by_a_cold_test`, `a_column_only_tamper_cannot_be_expressed_by_a_cold_export` |
| The whole report, exactly, and the verified span | `an_untampered_cold_export_verifies_and_the_report_names_every_input` |
| DEC-490 item 2: segments walk in manifest order, whatever their names | `segments_are_walked_in_manifest_order_whatever_their_names` |
| §11 check 4 `rehash_mismatch` | `a_modified_payload_is_rehash_mismatch` (vector `payload_modified`), `a_prev_hash_changed_without_rehashing_is_rehash_mismatch` (vector `prev_hash_changed_without_rehash`) |
| §11 check 3 `seq_gap` | `a_deleted_event_is_seq_gap` (vector `event_deleted`), `swapped_seq_members_are_a_seq_gap_because_an_export_carries_no_columns` (vector `seq_values_swapped`, DEC-115 item 3) |
| §11 check 5 `prev_hash_mismatch` | `a_prev_hash_rewritten_and_rehashed_is_prev_hash_mismatch` (vector `prev_hash_rewritten_and_rehashed`) |
| DEC-264 items 2 and 4: a non-canonical body fails the segment check before the walk | `whitespace_inserted_and_rehashed_is_segment_manifest_mismatch_in_a_cold_export` (vector `whitespace_inserted_and_rehashed`, DEC-490 item 9) |
| §11 per-range `anchor_head_mismatch` after every segment and per-event check passes | `a_tail_truncated_after_an_anchor_passes_every_check_and_fails_the_anchor` (vector `tail_truncated_after_anchor`), `a_chain_rewritten_from_seq_3_passes_every_check_and_fails_the_anchor` (vector `chain_rewritten_from_seq_3`) |
| §10 an anchor over the verified range passes, wherever its head sits | `an_untampered_export_passes_the_anchor_it_is_covered_by`, `an_anchor_whose_head_sits_in_an_earlier_segment_is_checked_there` |
| §11 per-range `anchor_root_mismatch` | `an_anchor_whose_root_does_not_match_its_leaves_is_anchor_root_mismatch` |
| DEC-490 item 5: an anchored head before the trusted start was not verified | `an_anchor_whose_head_lies_before_the_trusted_start_is_anchor_head_mismatch` |
| DEC-115 item 10: an anchor naming no leaf for the stream is refused | `an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused` |
| §11 `segment_manifest_mismatch` (DEC-264 items 1 and 2) | `a_flipped_byte_in_a_segment_file_is_segment_manifest_mismatch_at_its_first_seq`, `a_final_line_without_its_line_feed_is_segment_manifest_mismatch`, `a_manifest_whose_edges_lie_is_segment_manifest_mismatch`, `a_manifest_that_names_another_stream_than_its_file_is_segment_manifest_mismatch`, `a_manifest_that_is_not_one_is_segment_manifest_mismatch_at_the_seq_the_range_expected` |
| §11 `segment_gap` (DEC-264 item 3, both directions) | `a_gap_between_segments_fails_segment_gap_at_the_expected_seq`, `a_stale_or_overlapping_copy_beside_verified_segments_fails_segment_gap`, `a_first_segment_that_does_not_carry_the_trusted_start_fails_segment_gap` |
| §11 reachability: a range enters mid-segment and checks nothing before its start | `a_range_may_enter_mid_segment_from_its_trusted_start`, `an_event_before_the_trusted_start_is_not_checked` |
| DEC-264: the first failure in range order is the one reported | `the_first_failure_in_range_order_is_the_one_reported` |
| DEC-490 item 3: a cold export is one stream's range | `a_cold_export_that_mixes_streams_is_refused` |
| DEC-265 item 1, DEC-490 item 6: a token never verifies | `a_token_whose_imprint_matches_is_reported_incomplete_never_verified`, `a_token_without_the_anchor_s_imprint_is_tsa_token_invalid`, `a_token_is_checked_only_after_the_range_and_the_anchor_pass` |
| DEC-490 item 3: an export the command cannot cover is refused | `a_directory_with_no_segment_is_refused`, `an_unpaired_segment_or_manifest_is_refused` |
| DEC-490 item 4: every path has a code | `an_unreadable_export_anchor_or_token_path_is_refused_with_a_code`, `every_cold_refusal_reports_its_stable_code_first` |
| §11 check 6 `artifact_missing`, `artifact_mismatch` through the cold walk | `without_a_store_an_event_that_names_an_artifact_is_artifact_missing`, `an_altered_artifact_is_artifact_mismatch` |
| Replay: identical inputs give identical output | `identical_inputs_give_identical_output` |
| Exit status: any failure is non-zero | `ColdOutcome::failed` asserted in `assert_tamper`, the pinned codes, and every failure test |
| The binary's edge: `main` dispatches, prints the report and exits on the outcome (the mutation gate's `main` mutant) | `journal_verify_prints_its_report_and_exits_on_the_result` (live, E5-4's command) and `journal_verify_cold_prints_its_report_and_exits_on_the_result` in `crates/mandate-cli/tests/binary.rs` |

## Interpretations (recorded as DEC-490)

The nine items of [DEC-490](../decisions/DEC-490.md): a sibling subcommand; the directory's
layout and the manifest-order walk; an incomplete directory refused, never reported; every path
with a code; the checks' order; the token never verified; the seven-line report; the stubs'
report; and the vector cases a cold export re-expresses.

## Planted bugs (each tried against the implementation on `claude/hejxo4-e5-8-impl`, carried to `claude/hejxo4-e5-8-impl-2` as #662, then removed)

| # | Planted bug | Caught by |
|---|---|---|
| B1 | Walk the segments in file-name order | `segments_are_walked_in_manifest_order_whatever_their_names`, `a_stale_or_overlapping_copy_beside_verified_segments_fails_segment_gap` |
| B2 | Walk a manifest that does not parse first, not last | `a_manifest_that_is_not_one_is_segment_manifest_mismatch_at_the_seq_the_range_expected` |
| B3 | Report an empty directory as verified | `a_directory_with_no_segment_is_refused`, `every_cold_refusal_reports_its_stable_code_first` |
| B4 | Skip an unpaired segment file or manifest silently | `an_unpaired_segment_or_manifest_is_refused` |
| B5 | Take the trusted start from the first manifest instead of the arguments | `a_first_segment_that_does_not_carry_the_trusted_start_fails_segment_gap`, `a_range_may_enter_mid_segment_from_its_trusted_start`, `an_anchor_whose_head_lies_before_the_trusted_start_is_anchor_head_mismatch`, `an_event_before_the_trusted_start_is_not_checked` |
| B6 | Offer the anchor every row, those before the trusted start too | `an_anchor_whose_head_lies_before_the_trusted_start_is_anchor_head_mismatch` |
| B7 | Report a token whose imprint matches as verified (ignore `TsaVerificationIncomplete`) | `a_token_whose_imprint_matches_is_reported_incomplete_never_verified` |
| B8 | Check the token before the walk and the anchor | `a_token_is_checked_only_after_the_range_and_the_anchor_pass` |
| B9 | Skip the refusal of an anchor naming no leaf for the stream | `an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused`, `every_cold_refusal_reports_its_stable_code_first` |
| B10 | Accept an export whose manifests name two streams | `a_cold_export_that_mixes_streams_is_refused` |
| B11 | Report an unreadable file, or an unreadable export directory, through `anyhow` context with no code | `an_unreadable_export_anchor_or_token_path_is_refused_with_a_code`, `every_cold_refusal_reports_its_stable_code_first` |
| B12 | `failed()` counts the cold failures only, so a failing anchor exits 0 | `a_tail_truncated_after_an_anchor_passes_every_check_and_fails_the_anchor`, `the_cold_outcome_codes_and_result_lines_are_pinned` |
| B13 | Write the report before a refusal | `a_cold_export_that_mixes_streams_is_refused`, `an_anchor_that_names_no_leaf_for_the_exports_stream_is_refused`, `every_cold_refusal_reports_its_stable_code_first` |
| B14 | Report the span from the first segment's `first_seq` rather than the trusted start | `a_range_may_enter_mid_segment_from_its_trusted_start`, `an_event_before_the_trusted_start_is_not_checked` |
| B15 | Treat a missing artifact as corrupt when no store is given | `without_a_store_an_event_that_names_an_artifact_is_artifact_missing` |
| B16 | Count files and manifests in the `segments:` line | `an_untampered_cold_export_verifies_and_the_report_names_every_input`, `segments_are_walked_in_manifest_order_whatever_their_names` |
| B17 | Offer the anchor the last segment's rows only | `an_anchor_whose_head_sits_in_an_earlier_segment_is_checked_there` |
| B18 | Compare the streams of the first segment alone | `a_cold_export_that_mixes_streams_is_refused` |
| B19 | Read the token lazily, after the walk, with an empty default | `an_unreadable_export_anchor_or_token_path_is_refused_with_a_code` |

The do-nothing check: a stub that writes a seven-line report and returns `Verified` over an empty
span passes 0 of the 37 pending library tests (the binary's pending test fails on its exit status) (`cargo nextest run -p mandate-cli --test journal_verify_cold
--run-ignored only --no-fail-fast`: 37 run, 0 passed).

## What the tests cannot catch

- **The token's signature, chain and revocation.** Until DEC-265 item 1's crypto half lands,
  `verify_tsa` never answers `Ok`, and the command reports `tsa_verification_incomplete`; a
  forged token holding the imprint is refused the same way a genuine one is.
- **A forged anchor file.** As in E5-4: the command checks the anchor's leaf for the stream, the
  anchored head's presence and hash, and the root over its leaves, not an inclusion proof against
  a published root. The anchor must come from outside the export.
- **`column_mismatch` from a real column.** A cold export stores each body and its hash; the
  column cases against real rows are `mandate-journal-pg`'s and `mandate-refcases`'.
- **Very large exports.** Every segment file is held in memory; segments are bounded by the
  exporter (E5-7).

## Not done here (with the story that owns each)

- The export job that writes the directory this command reads, retention, legal holds and the
  replica: E5-7.
- The examination bundle, the inclusion proofs and the verifier's digest over a bundle: E5-9.
- RFC 3161 signature verification: Proposed in DEC-265 item 1, the founder's call.
- Giving E5-4's `verify` the `path_unreadable` code (the tracker's known issue): its own touch,
  because E5-4's tests do not pin it; the helper this command uses (`parse_anchor`) is ready for
  it.
- Journaling a `VerificationRun` for a command-line run: the command is a read-only auditor tool.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-cli --test journal_verify_cold
cargo nextest run -p mandate-cli --test journal_verify_cold --run-ignored only --no-fail-fast
cargo xtask ci pending
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
```

By hand, over a directory E5-7's export job wrote:

```bash
mandate journal verify-cold cold/ --store artifacts --anchor anchor.json --token anchor.tsr \
  --from-seq 1001 --trusted-prev-hash <64 hex>
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

Hit, and resolved in DEC-490: §6.2 names no directory layout, §11 no order between the per-range
checks and no word for a check that cannot finish, §12 no file format for a token. One vector,
`whitespace_inserted_and_rehashed`, is re-expressed by a cold export as the segment check's
failure (DEC-490 item 9), which tightens: the export still fails, earlier, with a §11 code.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails (none move; the
      tamper vectors are replayed through the command against the same fixture file).
- [x] Tests came first: 46 tests, 38 pending and failing on the stub with its own report, 8 live
      (argument parsing, the pinned codes, the oracle's manifest, the vector file's shape, and the
      binary's `journal verify` edge); the do-nothing check passes 0; 19 planted bugs each caught
      by the named test.
- [x] New state changes emit journal events (none: the command only reads).
- [x] Docs updated: this brief, DEC-490, the feature map, the work tracker's E5-8 rows, the
      backlog's E5-8 row.
- [ ] `cargo xtask check` is green (summary in each PR).
- [ ] The PR description is complete (see the PR template).
