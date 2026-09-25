# Task: E5-1 Hash-chained journal core

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story.

## Story

- **Story:** E5-1 ([backlog](../06-backlog-v1.md#e5-journal))
- **Acceptance criteria (verbatim):** "the [journal test vectors](../../specs/reference-cases/journal.yaml)
  (chain, string escaping, decimals, export line, Merkle anchor) reproduce byte for byte; the
  verification tool reports each tamper case's expected first failure; and the append protocol
  returns each append case's expected outcome ([journal spec](../../specs/journal.md))."
- **PRD / HLD / spec anchors:** PRD 6.7 (FR-7.1 to FR-7.7); HLD "Record before acting" and
  "Event-sourced state"; journal spec §2–§6.2, §9–§12; trading domain spec §12.
- **Decisions that apply:** DEC-71 (Tier 1 specs), DEC-72 (ADR-0001: ES-02, ES-04, ES-05, ES-07,
  ES-09, ES-11, ES-15, ES-21, ES-22, ES-23, ES-25), DEC-77 (proposed: two-PR mechanics).

## Scope

- **Reference cases that must move from pending to passing:** all 46 `journal::*` cases in
  `fixtures/refcases/journal.json`: `journal::version`; 11 `journal::decimal::accept::*`; 7
  `journal::decimal::reject::*`; `journal::string_escaping`; `journal::chain::seq_1` to `seq_5` and
  `journal::chain::append`; `journal::export_line_seq_1`; `journal::merkle`; 9 `journal::tamper::*`;
  9 `journal::append::*`.
- **Invariants touched** (journal spec clauses, each a named test):

  | Clause | Test |
  |---|---|
  | §1.1 append-only; §1.3 one writer per stream, gapless | `properties::appends_are_append_only_gapless_fenced_and_idempotent` (model oracle) |
  | §5.1 idempotency first; equivalent decimals are the same draft | same property; `properties::equivalent_decimals_are_the_same_draft` |
  | §1.5 tamper-evident; M4 exit "tampering with any event is detected" | `properties::any_tampering_is_detected` (byte, column, deletion, reorder) |
  | §10 a consistent rewrite is caught only by the anchor | `properties::rewritten_chains_pass_per_event_checks_and_fail_the_anchor` |
  | §10 RFC 6962 tree | `properties::merkle_root_matches_bottom_up_construction` (independent construction) |
  | §4 canonical form is RFC 8785 | `canon::matches_independent_rfc8785_implementation` (differential against `serde_json_canonicalizer`) |
  | §4.8 never relies on member order; whitespace and escapes irrelevant | `canon::whitespace_member_order_and_escapes_do_not_change_the_canonical_form` |
  | §4.6 decimals rejected, never rounded | `decimal::more_than_28_fraction_digits_are_rejected_not_rounded`, `decimal::magnitudes_from_the_limit_up_are_rejected` |
  | §4.6 normalization | `decimal::equivalent_spellings_normalize_to_the_canonical_form` (oracle builds spellings from the canonical form) |
  | §4.7 timestamps | `time::parse_matches_naive_count_and_round_trips` (naive day count), `time::order_is_chronological` |
  | §9 stream types and required `config_refs` | `catalogue::stream_types_and_required_config_refs_match_the_spec` (the spec table written out again) |

  Seeded bugs: `cargo mutants -p mandate-canon -p mandate-time -p mandate-journal` leaves no
  survivors (443 mutants: 391 caught, 52 unviable). One equivalent mutant is excluded in
  `.cargo/mutants.toml` for founder approval. Eight hand-planted bugs (escaping, decimal trailing
  zeros and rounding, check order, idempotency order, Merkle split, required refs, a column check)
  each fail the expected reference case.

- **Crates in scope:** `mandate-canon`, `mandate-time` (layer 0), `mandate-journal` (layer 2),
  `mandate-refcases` (tool). All four are safety-critical (ES-02).
- **Crates out of scope:** `mandate-num`, `mandate-domain`, `mandate-journal-pg` (E5-3),
  `mandate-cli`.
- **New dependencies allowed:** those ADR-0001 names for these crates: `sha2` (ES-07), `thiserror`
  (ES-09), `proptest` and `libtest-mimic` (ES-11), `serde_json_canonicalizer` (ES-07, dev only).
  `serde_json` and `toml` are already registered. Each has a registry row in this change.
- **Safety-critical:** yes. Delivered as a stack: tests then implementation for canon and time,
  tests then implementation for the journal and harness, then the status change (DEC-77).
- **Size budget:** 400 non-generated lines per safety-critical PR (ES-13); the actual sizes are
  in the PR descriptions.

## Interpretations the founder should confirm (tests PR review)

1. **Payload schemas.** Journal spec §9 says schemas "live in code". This story registers
   schema version 1 only for the event types the vectors use: `StreamOpened` (account streams),
   `IntentReceived`, `GateDecided`, `OrderSubmitted`, `FillApplied`, and `MarkUpdated` (draft F).
   Field sets are taken from the vectors. Decimals, timestamps, dates, and ULIDs are typed;
   domain vocabularies (`side`, `tif`, `verdict`, …) are non-empty strings until domain types
   exist, except gate check IDs, which must come from the §9 list. Appending an event type without
   a registered schema is `Invalid` (`unknown_schema`).
2. **Append-case drafts.** "New draft F" is a `MarkUpdated` (event ID `01J8Z3M4F0000000000000000F`,
   price `150.01`) caused by the seq 5 fill. "New GateDecided draft" is seq 3's draft with event ID
   `01J8Z3M4G0000000000000000G`. `recorded_at` for untimed appends is `2026-09-21T14:00:02.000000000Z`.
3. **Check order in `append`.** Drafts are validated first (an invalid draft cannot be compared),
   then idempotency, then fencing and the head check, then stream rules (seq 1 is `StreamOpened`,
   `StreamOpened` only at seq 1, one environment per stream), then sealing.
4. **Epochs.** Streams exist implicitly with head 0 and epoch 0. `take_ownership` increments the
   epoch. Any epoch other than the current one is `Fenced`, including a higher one.
5. **Reported seq.** Verification reports the row's `seq` column (`seq_values_swapped` expects 2).
6. **Stream membership.** `ClockAdvanced` is allowed in account streams (spec §2 has the executor
   copy it there), although the §9 account table does not list it.

## Decisions needed (found while tracing the specs)

1. **`FillApplied` and `risk_clock`.** Journal spec §2 requires a `risk_clock` payload field on
   account-stream risk-state inputs, but the vector `FillApplied` (schema version 1) has none, and
   the vectors must reproduce byte for byte. As built, `FillApplied` v1 has no `risk_clock`. Either
   the risk-state events get a schema version 2 with `risk_clock` when E6 defines them, or the vectors
   change (needs a DEC).
2. **`OwnerAcknowledged` stream.** Trading domain spec §12 lists it among account-stream domain
   events (from `owner_ack`); journal spec §9 puts it in the workspace control stream. As built, it
   is control-only.
3. **DEC-77** (proposed): the two-PR mechanics this story uses.

## Not done here (with the story that owns each)

- Postgres store, roles, triggers, and the real-database tamper and append replays: E5-3.
- Artifact store: E5-2. Verification takes an `ArtifactSource` trait; tests use a map.
- `risk_clock` monotonic check at append: with the first schema that carries `risk_clock` (E6).
- Snapshots, replay, upcasters, segment manifests, and `segment_*` and `tsa_token_invalid` checks:
  a later journal story (export and cold store).
- Event JSON Schema export to `schemas/events/` (a protected path, so its own PR) and the
  ES-22 drift check.
- Python `rfc8785` and `ref.canon()` differential runs, and cargo-fuzz targets: they need the
  `mandate-cli` JSON-lines driver and the nightly fuzz workspace.
- A verification CLI: the library is the tool for now; `mandate-cli journal verify` comes with
  `mandate-cli`.
- A CI guard that blocks implementation PRs from editing tests beyond removing pending markers
  (DEC-77), and cargo-mutants in CI (ES-12 "mutants on the diff"): xtask work, proposed separately.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-canon -p mandate-time -p mandate-journal
cargo test -p mandate-refcases -- --include-ignored   # pending reference cases
cargo mutants -p mandate-canon -p mandate-time -p mandate-journal
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none: this story builds the journal itself).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
