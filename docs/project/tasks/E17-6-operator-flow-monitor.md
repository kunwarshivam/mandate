# Task: E17-6 see and stop research-agent flow

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** [E17-6](../06-backlog-v1.md) — "As an operator, I want to see and stop research-agent
  flow across the accounts of a deployment, so that one thesis cannot concentrate orders from many
  accounts in one instrument unnoticed ([DEC-100](../04-decision-log.md#decisions))."
- **Acceptance criteria (verbatim):** "no workspace's gate reads another workspace's state (a
  two-workspace test shows one's positions never change the other's decisions); the aggregate-flow
  monitor sums research-agent exposure per instrument over its deployment's workspaces, in dollars
  and as a share of average daily dollar volume, alerts the operator above the thresholds, writes to
  no workspace, and sends nothing to the global control plane; the operator per-thesis halt,
  journaled in each workspace as a `PlatformOperatorAction`, stops matching research-agent
  admissions and openings in every workspace of the deployment while exits and protection continue,
  and never permits anything a workspace's own limits deny; openings on a new thesis wait for the
  workspace's deterministic stagger offset within the conduct controls; bring-your-own-strategy
  agents keep per-account controls only."
- **PRD / HLD / spec anchors:** mandate spec §2.3 (the working universe at runtime; the removal
  reasons; the exits-only semantics of §2.2), §8.4 ("Correlated flow", DEC-100 and DEC-123's
  thresholds, the halt, the stagger, the bring-your-own-strategy sentence), §8.5 check 9
  (`operator_halt`), §8.6 (removal); journal spec §9's `UniverseChanged` row (thesis and lineage
  ids for every reason) and `PlatformOperatorAction`'s row (`research_thesis_halt`, journal spec
  v0.4); DEC-100 (the three controls), DEC-123 (the thresholds and the 900 s stagger floor), DEC-05
  (exits and protection continue), DEC-09 (no workspace's state is a gate input), DEC-132 (the
  crate's readings, item 9's halt facts), DEC-103 (the thin slice the monitor watches).
- **Decisions that apply (DEC-NN):** DEC-100, DEC-123, DEC-05, DEC-09, DEC-67 (the pinned content
  hash a hash-scoped halt names), DEC-132, DEC-103; DEC-293 (this story's design), DEC-294 and
  DEC-295 (reserved for the implementation readings and the follow-ups).

## Scope

- **Reference cases that must move from pending to passing:** none. Family N already covers check 9
  (MC-N10, an operator halt refuses the admission) and the stagger (MC-N23, whose three offsets are
  two workspaces' derivations of the same thesis: `ws_a`/`th-1` = 317 against `ws_b`/`th-1` = 682).
  The monitor and the halt's application have no reference-case family, like E17-5's detector and
  E17-8's evaluator.
- **Invariants touched:** MI-19 (a removed instrument is exits-only and keeps its claim until
  flat — the halt's removal is one more way to get there, never a new semantics), MI-20 (a pinned
  universe admits nothing — the mirror reading: a halt never touches one, because a pinned mandate
  has no research flow to stop), DEC-09 (no workspace's state is a gate input — isolation is
  representable at this crate's seams and pinned by tests), ES-21 (no clock, no randomness, no
  floats: exact decimals and ordered collections throughout), ES-09 (one new code,
  `negative_exposure`, joins `ResearchError`'s stable registry and is pinned live).
- **Crates in scope:** `mandate-research` (a new `flow` module; `lib.rs` gains `pub mod flow;`, the
  `NegativeExposure` variant and its ES-09 code, and the crate-doc lines).
- **Crates out of scope:** `mandate-spec` (no validator, envelope, or fixture change — the
  admitting model's content hash arrives as an argument, so the narrow envelope view is untouched),
  `mandate-risk` (the gate is stream G's under open claim #123; its `not_in_working_universe`
  denial already stops openings in a removed instrument), `mandate-runtime` and `mandate-shell`
  (the halt's wiring into the fold and the `PlatformOperatorAction` control entry are later
  stories), `mandate-journal` (the catalogue already accepts `PlatformOperatorAction` with
  `research_thesis_halt`), every crate on the frozen list.
- **New dependencies allowed:** none.
- **Safety-critical:** yes. This task is the **tests PR** of the two-PR flow.
- **Size budget:** the module's non-test surface is types and three entry points — expected well
  under 300 non-test lines at implementation.

## The design (DEC-293)

The story's admission half is already merged: §8.5 check 9 refuses a thesis for a halted
instrument, and the halt set arrives as an `AdmissionFacts` field (E17-3, MC-N10). This slice
builds the other two controls' pure halves in `mandate-research`'s new `flow` module.

1. **The monitor is deterministic pure code.** No I/O, no clock, no randomness, no floats (ES-21);
   the function's signature cannot write to a workspace, which is what "writes to no workspace, and
   sends nothing to the global control plane" is as code: a pure function over typed rows returning
   a report value. DEC-100's operator *service* — the process in the workspace deployment that
   gathers the rows and shows the report — is wiring outside this crate, exactly as E17-2's model
   invocation and E17-5's escalation seam are.
2. **Rows are per agent, grouped by workspace.** `AgentFlow { workspace, pinned, exposure }`: one
   agent's research exposure per instrument, in dollars. For a dynamic mandate every instrument in
   the working universe is research-admitted, so the agent's exposure over its working universe is
   its research flow; the caller derives the dollar values (facts produced elsewhere, the crate's
   house pattern). A pinned agent contributes nothing — bring-your-own-strategy keeps per-account
   controls only, and its instruments are the owner's choice, not correlated research flow.
3. **The threshold is DEC-123's, per instrument per deployment: the lower of 1% of the
   instrument's 20-day average daily dollar volume and 1,000,000 USD.** The alert is strictly
   above: at the bar the row is quiet, one cent above it alerts. Both arms are pinned at the bar
   and one cent over, and the `min` is pinned by cases that discriminate against a `max`.
4. **The share is reported, not only compared.** Each row carries the instrument's summed exposure
   in dollars and its share of ADV as an exact ratio at the 12-place report scale (one rounding of
   one quotient, the E4-2 report discipline). An instrument with exposure and no ADV entry keeps
   the share `None` and alerts: the threshold is undecidable (the 1% arm is unknown and may be
   below the exposure), and reading missing data as quiet is the fail-open direction AGENTS.md
   rule 3 bars — even below the 1,000,000 USD arm, because the lower of the two may bind. A zero
   ADV decides — the 1% arm is zero, so any exposure alerts — while the share stays `None`
   (nothing divides by zero).
5. **The report holds only instruments with research exposure above zero**, ordered by instrument
   id, each row naming its contributing workspaces in order, deduplicated (two agents of one
   workspace contribute one entry). The operator's next action — a halt — needs the workspace
   list, which is the "see" half of "see and stop".
6. **Negative exposure is refused** (`ResearchError::NegativeExposure`): v1 is long-only
   (AGENTS.md rule 12), so a negative row is a caller bug, and folding it would under-state the
   aggregate — the fail-open direction again.
7. **The halt's resolution is per workspace: `halted_instruments(halts, admitting_model)`.** An
   unscoped halt (no content hash) matches every workspace. A hash-scoped halt matches only a
   workspace whose admitting model's content hash is the named one; a pinned mandate (no admitting
   model) is matched by no hash-scoped halt. The admitting hash arrives as an argument — a fact
   the caller reads from the mandate — because the crate's narrow envelope view deliberately
   carries no model triple, so no shared-type ripple crosses streams F and G. DEC-100 names "its
   pinned content hash (DEC-67)" and nothing else.
8. **The halt's application removes, and only removes: `apply_operator_halts(halted, entries,
   universe)`.** An unread universe is an error (`UniverseUnavailable`) — the fold cannot know
   what it holds, and a silently dropped halt is fail-open. A pinned universe is returned
   unchanged with an empty journal: the pinned list is the owner's, and a pinned mandate has no
   research flow to stop (MI-20's mirror). A dynamic universe: every halted instrument the
   entries hold is removed, journaled as `UniverseChanged` (removed, `operator_halt`) citing the
   entry's thesis and lineage — journal spec §9's row requires both ids for every reason — with
   `universe_size_after` sinking by one per removal, in instrument order; a halted instrument the
   entries do not hold removes nothing and journals nothing (idempotent; check 9 holds its
   admissions anyway). Duplicate entries are refused (`DuplicateInstrument`, `expire_theses`'
   rule), and the dynamic output universe is rebuilt from the entries minus the halted, exactly
   as `expire_theses` reads them. The removal is exits-only (§2.2 through §8.6): the restriction
   emitted is `RemovedInstrument` — never a mode change, never a permission. "Never permits
   anything a workspace's own limits deny" is structural: the application produces removals only.
9. **Isolation is representable, and the tests pin it at this crate's seams.** No entry point
   takes two workspaces' state except the monitor, whose output is a report no admission, fold, or
   halt input accepts; a halt crosses workspace boundaries only as each workspace's own resolved
   halt set, derived from its own journaled `PlatformOperatorAction`. The gate itself is stream
   G's under an open claim and is structurally single-workspace (its inputs carry one workspace's
   mandate, policy, state, and market data, DEC-09); the two-workspace gate run the acceptance
   clause names is pinned here at the research plane — one workspace's admissions, folds, and
   halts are bit-identical whether or not another workspace exists — and the full gate-level
   two-workspace run lands with the deployment wiring story.
10. **The stagger is already two-workspace**: MC-N23 pins `ws_a`/`th-1` = 317 against
    `ws_b`/`th-1` = 682 — the same thesis, two workspaces, two independently derived offsets
    inside the conduct controls. This slice adds nothing to it.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-research --test flow
cargo xtask ci pending
```

## Interpretations (the readings this brief takes)

1. "Sums research-agent exposure" is read **per agent, grouped by workspace**: a workspace may
   host several agents, and only its research agents' exposure is research flow. DEC-100 and the
   mandate spec's monitor sentence both sum "research-agent exposure ... over the workspaces",
   which is a per-agent quantity grouped by workspace, not a workspace-intrinsic one.
2. A pinned agent contributes nothing, and a halt never touches a pinned universe: the acceptance
   clause's "bring-your-own-strategy agents keep per-account controls only" excludes them from
   the aggregate (their instruments are owner-chosen, not correlated by a shared research agent)
   and from the halt's reach (a pinned mandate has no research admissions or openings to stop).
   The alternative — counting every workspace's gross exposure — would put a cross-workspace
   control on agents the clause holds out of them.
3. "Alerts the operator above the thresholds" is strict: exposure exactly at the threshold is
   quiet. Both DEC-123 arms are pinned at the bar and one cent above it, and the "whichever is
   lower" is pinned against "whichever is higher" with cases that alert under the min and read
   quiet under the max.
4. A missing ADV is undecidable, not quiet: the row alerts with the share named absent. Refusing
   the whole report instead — one missing figure blinding the monitor to every other instrument —
   was not taken, because a monitor's liveness is its own safety property; the row-level `None`
   keeps the rest of the report alive while naming the gap.
5. A zero ADV is decidable: the 1% arm is zero, so any exposure alerts; the share stays `None`
   because nothing divides by zero.
6. The halt's `UniverseChanged` cites the halted instrument's active thesis and lineage: journal
   spec §9's row carries thesis and lineage ids for every reason, `operator_halt` included.
7. The halt's application rebuilds the dynamic universe from the entries minus the halted, as
   `expire_theses` reads them: the entries name the dynamic universe's instruments, once each;
   the `universe` argument carries the two states that decide at all — unread, and pinned.
8. The hash-scoped halt matches on the admitting model's content hash alone, not the whole pinned
   triple: DEC-100 names "its pinned content hash (DEC-67)" and the halt's purpose is to stop one
   model's theses; the id and version travel with the hash inside the mandate.
9. Negative exposure is a caller bug, refused fail-loud: v1 is long-only, and a negative row
   under-states the aggregate.
10. The monitor's report is per deployment, not per workspace: DEC-123's alerting unit is "per
    instrument per deployment", so the report sums the deployment's agents, and each row names
    its contributing workspaces — the list the operator needs before issuing a halt.
11. `halted_instruments` is total — the matching has no failure mode — so its `Result` exists for
    the DEC-77 stub protocol alone and always returns `Ok` once implemented.

## Planted bugs (each tried against a throwaway implementation; a named test must catch each)

| # | Bug | Caught by |
|---|---|---|
| 1 | The sum keeps only one row per instrument (the first workspace's) | `the_deployment_sums_research_exposure_per_instrument` |
| 2 | A pinned agent's exposure counts toward the aggregate | `a_pinned_agent_contributes_nothing_to_the_aggregate` |
| 3 | The threshold takes the higher arm (`max`) | `the_threshold_is_whichever_arm_is_lower` |
| 4 | The alert comparison is `>=` (or `<`), so the bar alerts (or one cent above reads quiet) | `a_thin_name_alerts_above_one_percent_of_its_adv` and `a_megacap_alerts_above_the_dollar_threshold` |
| 5 | The share divides ADV by exposure (inverted) | `the_report_states_each_instrument_s_share_of_adv` |
| 6 | A missing ADV reads quiet | `a_missing_adv_alerts_even_below_the_dollar_threshold` |
| 7 | A zero ADV reads quiet | `a_zero_adv_alerts_on_any_exposure` |
| 8 | A negative exposure folds silently | `a_negative_exposure_is_refused` |
| 9 | The report is unordered, or ordered by exposure | `the_report_is_ordered_by_instrument_id` (the fixture puts the largest exposure on the middle instrument — the trial's first fixture put it on the first, where both orders agree, and the plant passed; the fixture was reordered before anything merged) |
| 10 | Zero-exposure instruments appear as rows | `an_instrument_with_no_research_exposure_is_absent_from_the_report` |
| 11 | Contributing workspaces are not deduplicated (or unordered) | `the_deployment_sums_research_exposure_per_instrument` |
| 12 | A hash-scoped halt matches every workspace (the hash is ignored) | `a_hash_scoped_halt_matches_only_its_own_research_agent` |
| 13 | An unscoped halt is dropped (only hash-scoped halts match) | `an_unscoped_halt_matches_every_workspace` |
| 14 | The removal journals another reason (`thesis_expired`, say) | `a_halt_removes_the_instrument_from_the_working_universe` |
| 15 | The removal journals `UniverseChanged` admitted, or nothing at all | `a_halt_removes_the_instrument_from_the_working_universe` |
| 16 | A pinned universe is emptied, unpinned, or halted through | `the_same_halt_leaves_a_pinned_universe_alone_and_removes_from_a_dynamic_one` |
| 17 | An unread universe returns an empty outcome instead of the error | `an_unread_universe_is_never_halted_silently` |
| 18 | `universe_size_after` counts before the removal, or writes the input size on every row | `removals_are_journalled_in_instrument_order_with_sinking_sizes` |
| 19 | Removals follow the entry list's order, not instrument order | `removals_are_journalled_in_instrument_order_with_sinking_sizes` (the fixture's entries arrive out of order) |
| 20 | The halt also removes — or journals — instruments the universe does not hold | `a_halt_removes_the_held_instrument_and_leaves_one_the_universe_does_not_hold` |
| 21 | The halt lifts a `RemovedInstrument` restriction, or journals an admission | `the_halt_only_ever_removes` |
| 22 | Duplicate entries fold silently (the last one wins) | `duplicate_entries_are_refused` |
| 23 | A hash-scoped halt matches any workspace that has an admitting model, whatever its hash (the hash never compared) | `a_hash_scoped_halt_matches_only_its_own_research_agent` and `a_halt_reaches_a_workspace_only_through_its_own_halt_set` |
| 24 | The monitor refuses the whole report when any instrument's ADV is missing (interpretation 4's rejected alternative: one gap blinds the monitor to every other instrument) | `a_missing_adv_alerts_even_below_the_dollar_threshold` (the present companion row must still report) |

The trial could not seed "one workspace's rows change another workspace's admission or fold
outcomes" without changing a signature outside this module — no admission, fold, or halt input
accepts another workspace's state, which is the property itself: isolation here is
representational, pinned behaviourally by the two isolation tests rather than by a seeded bug.

## Not done

- The operator service itself: gathering the rows in the deployment, showing the report, and
  issuing the halt are wiring outside this crate (DEC-100's "operator service in the workspace
  deployment"); the pure halves here are what it calls.
- The `PlatformOperatorAction` control entry: its writing into each workspace's control stream and
  its payload schema are E7-10's (stream L, claim #171); journal spec v0.4 already carries the
  `research_thesis_halt` action, so no spec change is needed for it.
- The halt's `UniverseChanged` causation: journal spec §2's copy list names the executor's copies
  from `ThesisProposed`/`ThesisRevised` and `MandateVersionApplied` but not yet the operator
  halt's copy from the `PlatformOperatorAction`. The executor's copy rule is a journal-spec seam
  the wiring story (or a DEC-176-tightening spec change) closes; this slice returns the entry
  values, as it does for every other entry.
- The halt's lifetime (whether an operator lifts one, and how) is the operator service's; the
  halt set this crate reads is whatever the workspace's journal resolves to.
- The gate-level two-workspace run (the acceptance clause's own words, "one's positions never
  change the other's decisions" through the gate) lands with the deployment wiring story; the
  research-plane isolation here is the seam this crate owns.
- The thresholds as envelope or policy fields: DEC-123 fixes them ("Revisit the values before
  live trading"), and a code constant is the conservative start; moving them is a founder
  question, recorded, not taken.
- The property suite (proptest) lands with the implementation PR, where real behaviour exists to
  fuzz; the tests PR carries the one oracle the #415 review asked for — the cross-workspace sum
  and its contributing-workspace set, a plain pending function driving a `TestRunner` over
  generated deployments — because DEC-110's no-macro rule bars a pending test a macro generates,
  not an oracle a plain function computes.
