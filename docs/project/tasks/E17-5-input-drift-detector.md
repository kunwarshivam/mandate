# Task: E17-5 the input-drift detector

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** [E17-5](../06-backlog-v1.md) — "As an owner, I want the input-drift detector
  (`unusual_input`, V-018) so that unusual inputs escalate before the research agent acts on them
  (DEC-101)."
- **Acceptance criteria (verbatim):** the backlog row carries no "Accepted when" of its own; the
  criteria are [mandate spec §8.4](../../specs/mandate.md)'s sentence — "The input-drift detector
  (`unusual_input`, V-018) escalates unusual inputs before the agent acts on them; prompt injection
  through news and filings is the top technical risk (RAID R-05), and the allowlist,
  corroboration, the eligibility floor, `max_instruments`, and `autonomy.admission` are the
  defenses" — plus DEC-101's "The input-drift detector (E17-5, `unusual_input`, V-018) becomes
  Must in Phase 1", and V-018's own gate: "Rules and delegations do not use `unusual_input` until
  the input-drift detector ships."
- **PRD / HLD / spec anchors:** mandate spec §8.4 (inputs, the detector, R-05), §6.3 (the
  `unusual_input` condition fact, line 708's "Reserved: not usable until the input-drift detector
  ships"), §12 item 2 (the detector's design is an open question), V-018; DEC-60, DEC-101, DEC-132
  (the crate's readings); RAID R-05.
- **Decisions that apply (DEC-NN):** DEC-60 (V-018's reservation), DEC-101 (inputs and admission
  hardening; the detector is Must in Phase 1), DEC-132 (the crate's 21 interpretations), DEC-99
  and DEC-103 (the thin slice the detector serves), DEC-120 (the cost cap, the neighbour check 7).

## Scope

- **Reference cases that must move from pending to passing:** none. The detector is new pure
  behaviour with no reference-case family; family N's cases end at the crate's existing checks.
- **Invariants touched:** MI-16 (nothing here changes an envelope field — the detector reads
  inputs, never the mandate), ES-21 (no clocks, no randomness, no floats: the fold is over
  nanosecond timestamps and integer counts), ES-09 (the one code this slice mints,
  `observation_out_of_order`, joins `ResearchError`'s stable code registry and is pinned live;
  no journal §8.5 reason code is minted).
- **Crates in scope:** `mandate-research` (a new `drift` module; `lib.rs` gains `pub mod drift;`
  alone).
- **Crates out of scope:** `mandate-spec` (V-018's lift is stream F's, with the fixture's V-018
  case), the journal's `ThesisProposed` schema (any escalation field is a §9 change), every crate
  on the frozen list.
- **New dependencies allowed:** none.
- **Safety-critical:** yes. This task is the **tests PR** of the two-PR flow.
- **Size budget:** the module's non-test surface is types, constants, and the fold — expected
  well under 400 non-test lines at implementation.

## The design (DEC-266)

The spec leaves the detector's design open (§12 item 2). DEC-266's proposal, which this brief
pins:

1. **The detector is deterministic pure code in `mandate-research`.** No model call (the crate
   never calls one), no clock, no randomness (ES-21), no floats: every measure is integer
   arithmetic over nanosecond timestamps, byte counts, and observation counts.
2. **Content never reaches the detector as text.** The observation type carries a `ContentHash`,
   a byte length, a source, a class, and a timestamp — nothing else. The R-05 surface (prompt
   injection through news and filings) is defended by construction: the detector measures the
   *shape* of a source's traffic (what arrived, how fast, how big, how repeated), because an
   injected payload's shape is what deterministic code can see without a model. Changing the
   field to text breaks every test that constructs an observation, which is the catch for that
   bug.
3. **Per-source windows.** Each allowlisted source keeps a baseline (its first 32 observations)
   and a recent window (its last 8; the two may overlap when fewer than 40 have arrived). Drift
   is measured per source and never crosses sources: a compromised source escalates only theses
   that cite it.
4. **The measures, in evaluation order; the first crossed is the reported one** (the house style
   of §8.5: the first failure decides):
   1. `NoBaseline` — fewer than 32 observations ever seen. **Unusual, fail-safe** (AGENTS.md rule
      3: ambiguity resolves to the side that never adds risk — an unproven source's inputs
      escalate until its baseline fills).
   2. `DuplicateContent` — the most-repeated content hash in the recent window appears at least
      4 of 8 times (a replayed or spammed payload).
   3. `ArrivalRate` — `baseline_span_nanos > 12 × recent_span_nanos`: the recent 8 arrived in
      less than a twelfth of the time the baseline's 32 did (a flood).
   4. `GapCollapse` — `4 × 31 × min_recent_gap_nanos < baseline_span_nanos`: the smallest
      inter-arrival gap in the recent window is under a quarter of the baseline's mean gap
      (31 gaps across 32 observations).
   5. `LengthShift` — the recent window's mean byte length is more than 4× the baseline's mean,
      or under a quarter of it (an injected payload, or a stripped feed).
5. **The fact.** `unusual_input` (§6.3) is true for a proposal exactly when a source it cites is
   unusual — and a cited source never observed is unusual, because zero observations is fewer
   than 32 (the fail-safe reading of an unproven citation; the report's per-source verdicts cover
   observed sources, and the fact itself consults the fold for the unseen one). The crate
   computes it; making it rule-usable is V-018's lift, which is stream F's spec change with the
   founder, not this slice's.
6. **The escalation seam is Proposed, not taken.** §8.4 says the detector "escalates unusual
   inputs before the agent acts on them", and every way to wire that into the admission path
   changes a founder-owned contract: a new §8.5 reason code (ES-09's registry), an escalation
   field on `ThesisProposed` (journal spec §9), or V-018's lift alone (rules decide). DEC-266
   records all three and recommends the journaled escalation (the owner is asked; "escalates" is
   an escalation, not a refusal) — the founder picks, in the next spec window, and the
   implementation PR wires whichever is chosen. Until then the detector computes and reports, and
   `admit()` is untouched.
7. **Thresholds are code constants in the implementation PR**, pinned from both sides by the
   pending boundary cases — each measure's bar with its just-under neighbour, so a threshold
   moved by one in either direction, or a strict comparison relaxed to equality, fails a test —
   and by a live test there. Recorded here: baseline 32, recent 8, duplicate 4-of-8, arrival
   factor 12 (which compares spans, so crossing needs roughly a 2.7x rate, not a 12x flood: 8
   arrivals at the baseline's pace already span 7/31 of it), gap factor 4 (with the 31 gaps),
   length factor 4. Moving them into envelope fields (AGENTS.md rule 11 names signal-model
   thresholds envelope fields) is a founder question recorded in DEC-266; a code constant is the
   conservative start because an owner cannot be talked out of a threshold they never
   confirmed.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-research --test drift
cargo xtask ci pending
```

## Interpretations (the readings this brief takes)

1. "Escalates … before the agent acts" places the detector's verdict at the proposal boundary,
   not inside the model loop: the crate receives typed observations the shell records as inputs
   arrive (E17-2's invocation is the shell's), and the verdict is available to whatever admission
   becomes.
2. A source with no baseline escalates (item 4.1 above), and so does a citation of a source
   never observed: zero observations is fewer than 32. The alternative — treating a new source
   as quiet until proven unusual — reads a missing answer as "normal", which is the fail-open
   direction AGENTS.md rule 3 bars.
3. Per-source isolation is a property, not a convenience: the report's unusual set is computed
   source by source, so one source's burst never escalates another's theses.
4. The recent window may overlap the baseline. A source that has just filled its baseline is
   judged on its own history so far; requiring 40 disjoint observations before any verdict would
   read "not yet measurable" as "not unusual", the fail-open direction again.
5. The fold keeps arrival order. The timing measures are the signal; a fold that collapsed its
   observations into a set would drop a same-instant double arrival and read a burst as quiet, so
   a test asserts the tie is kept. Timestamps are monotone per source — ties allowed, a
   strictly earlier instant refused with `ResearchError::ObservationOutOfOrder` (DEC-85's
   fail-loud reading of a shell that hands observations over out of order) — and the check is per
   source: one source's timeline never constrains another's.
6. Determinism is total: the report depends on each source's own observation order and nothing
   else — which source arrived first, or how two sources' observations interleave with each
   source's order kept, changes nothing (ES-21's replay equality at the detector's scale), and a
   test asserts it against reordered and interleaved folds, so a report that remembers fold
   order fails it.
7. A refused observation quarantines its source (round 1's tightening reading): a backwards
   timestamp breaks the timing integrity both windows read, so both read only the observations
   after the last refusal and the source is unusual (`NoBaseline`) until a full new baseline has
   formed. The alternative — a refusal count on `SourceDrift` with any recent refusal unusual —
   was not taken, because the quarantine also stops the untrusted timing from feeding any
   measure.

## Planted bugs (each tried against a throwaway implementation; a named test must catch each)

| # | Bug | Caught by |
|---|---|---|
| 1 | `observe` folds into a set keyed by timestamp, dropping a same-instant double arrival | `the_fold_keeps_same_instant_arrivals` |
| 2 | The report marks every observed source unusual when any one crosses a threshold (isolation lost) | `a_compromised_source_does_not_escalate_others` |
| 3 | `NoBaseline` reads as quiet (fail-open) | `a_source_without_a_baseline_escalates_fail_safe` |
| 4 | The duplicate measure counts distinct hashes, not repeats | `replayed_content_crosses_the_duplicate_measure` |
| 5 | The arrival measure compares window *counts* (32 vs 8) instead of spans, so it never crosses | `a_volume_spike_crosses_the_arrival_measure` |
| 6 | The length measure rounds the mean to zero on small baselines (integer division before the comparison) | `a_length_shift_crosses_the_length_measure` |
| 7 | `unusual_input` ORs over all observed sources, not the cited ones | `the_fact_is_true_only_for_a_cited_unusual_source` |
| 8 | The report forgets the baseline and compares the recent window to itself, so nothing ever crosses | `a_volume_spike_crosses_the_arrival_measure` (a spike read against itself is quiet) and `a_length_shift_crosses_the_length_measure` |
| 9 | The observation type gains a text field (R-05 surface) | every test that constructs an observation — the change breaks their construction sites |
| 10 | A backwards timestamp is folded silently, or the order check is global rather than per source | `an_observation_before_its_source_s_last_is_refused` |
| 11 | `unusual_input` reads a citation of a never-observed source as quiet (fail-open) | `a_cited_source_never_observed_is_unusual` |
| 12 | `BASELINE_WINDOW` moved to 31 or 33, or its `<` relaxed to `<=` | `the_baseline_bar_is_pinned_at_32` (32 quiet at the bar, 31 unusual just under) |
| 13 | `RECENT_WINDOW` moved to 7 (or 9) | `the_duplicate_bar_is_pinned_at_4_of_the_recent_8` — the replays sit in the window's older half, so 7 reads three — and, for 9, `the_length_bar_is_pinned_at_4x_the_baseline_mean` and the arrival crossing |
| 14 | `DUPLICATE_MINIMUM` moved to 3 or 5, or its `>=` tightened to `>` | `the_duplicate_bar_is_pinned_at_4_of_the_recent_8` (4-of-8 at the bar, 3-of-8 just under) |
| 15 | `ARRIVAL_FACTOR` moved to 11 or 13, or its `<` relaxed to `<=` | `the_arrival_bar_is_pinned_at_a_twelfth_of_the_baseline_span` (a 155 s span at the bar, 154 s just under) |
| 16 | `GAP_FACTOR` moved to 3 or 5, or its `<` relaxed to `<=` | `the_gap_bar_is_pinned_at_a_quarter_of_the_baseline_mean_gap` (a 15 s smallest gap at the bar, 14 s just under) |
| 17 | `LENGTH_FACTOR` moved to 3 or 5, or either strict comparison moved onto equality | `the_length_bar_is_pinned_at_4x_the_baseline_mean` (4 000 and 250 at the bar, 4 001 and 249 just outside) |
| 18 | A refusal leaves no residue: the pre-refusal history still feeds the measures | `a_refused_observation_quarantines_the_baseline_until_a_new_one_forms` |
| 19 | The report remembers fold order, so which source arrived first changes it | `the_report_is_independent_of_the_sources_arrival_order` |
| 20 | The duplicate measure is evaluated after the timing measures | `replayed_content_crosses_the_duplicate_measure` (the window crosses both, and duplicate must be the one reported) |
| 21 | The gap measure divides by 32 (or 30) gaps instead of the baseline's 31 | `the_gap_divisor_is_the_baseline_s_31_gaps` (a 14.8 s collapse crosses under 31 and not 32; a 15.2 s neighbour is quiet under 31 and not 30) |
| 22 | The duplicate measure counts the whole history, not the recent window | `replays_confined_to_the_baseline_do_not_cross` (four replays inside the baseline, outside the recent 8, read quiet) |

## Not done

- The escalation seam (item 6): Proposed to the founder in DEC-266; wired in the implementation
  PR only once the founder picks.
- V-018's lift in `mandate-spec`, the fixture's V-018 case, and any rule that names
  `unusual_input`: stream F's, after this lands.
- The thresholds as envelope fields (item 7): a founder question, recorded, not taken.
- The shell's recording of observations (E17-2's invocation): the crate receives them typed; how
  the shell produces them is the shell's story.
- The property suite (proptest) lands with the implementation PR, where real behaviour exists to
  fuzz; the tests PR's invariants are plain pending functions, per DEC-110's no-macro rule.
