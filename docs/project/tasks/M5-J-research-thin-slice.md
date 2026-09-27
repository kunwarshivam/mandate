# Task: stream J, `mandate-research` (the E17 thin slice as code)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). This stream turns the
research-agent contract of [mandate spec §8](../../specs/mandate.md#8-signal-models-and-the-order-builder)
into Rust: the thesis as typed data, the ordered admission checks of §8.5, the revision lineages and
retirement of §8.6, thesis expiry, the deterministic stagger offset of §8.4, and the three journal
events the crate produces. It is the [DEC-103](../04-decision-log.md#decisions) thin slice, so it
runs only in the team's internal paper workspaces, over the fixed research basket, with every
admission `ask` and scorecards on.

**The LLM itself is out of scope.** The crate takes a model output as typed input and never calls a
model: no HTTP, no prompt, no artifact store, no provider. `python/research_spike/` shows the shape
the LLM produces today ([RS-1](RS-1-research-spike.md)); turning that shape into a platform model
invocation is E17-2's shell work, named under "Not done".

The file is named after the stream because one crate carries four stories' worth of §8; one PR still
implements one story's worth at a time.

## Story

- **Stories:** [E17-3](../06-backlog-v1.md#e17-research-agent-and-dynamic-universe) (admission),
  [E17-7](../06-backlog-v1.md#e17-research-agent-and-dynamic-universe) (the vetted-source allowlist
  and corroboration), [E17-2](../06-backlog-v1.md#e17-research-agent-and-dynamic-universe) (the
  thesis contract and its journal entries, without the model invocation), and the **fold only** of
  [E17-9](../06-backlog-v1.md#e17-research-agent-and-dynamic-universe) (the lineage state, the
  revision cap, and retirement).
- **Acceptance criteria (verbatim):**
  - E17-3: "As an owner, I want instruments admitted into the working universe only through the
    eligibility floor, the policy's asset classes, `max_instruments`, instrument-group claims, and my
    autonomy rules (`new_instrument`, `thesis_confidence`; default `ask`), journaled as
    `UniverseChanged`, and removed to exits-only when a thesis is invalidated. *Accepted when:*
    simulation fuzzing over random theses never admits an ineligible instrument or exceeds the
    envelope; prompt-injection fixtures never reach an order."
  - E17-7: "As an owner, I want the research agent to read only vetted sources and to admit an
    instrument only on corroborated evidence, so that one planted source cannot admit an instrument
    (DEC-101). *Accepted when:* the source allowlist is versioned configuration; the agent reads no
    source outside it; an admission without corroboration by an independent source, or by market data
    consistent with the thesis, is rejected; the corroboration is recorded in `ThesisProposed`;
    prompt-injection fixtures for every input source never reach an order."
  - E17-2: "As an owner, I want a research agent that turns market data, news, filings, and the
    agent's memory into theses (instrument, direction, horizon, evidence, invalidation), journaled as
    `ThesisProposed`, so that the agent has ideas without me."
  - E17-9: "As an owner, I want the research agent to revise a thesis that failed on forward paper,
    with its autopsy recorded, so that the platform improves its ideas without hiding its failures
    (DEC-111). *Accepted when:* a revision is journaled as `ThesisRevised` linked to its predecessor
    and names the failure it addresses; it starts with an empty scorecard and is scored only by the
    E17-8 evaluator; it passes the eligibility floor, corroboration, and the autonomy rules like a
    new thesis and cannot loosen any envelope field; past `max_revisions_per_lineage` the lineage is
    retired and the owner is told. Depends on E17-8 and on one completed DEC-99 evaluation on the
    DEC-103 thin slice."

  E17-3's "never reaches an order" and E17-9's whole loop span four streams; this brief takes the
  parts that are §8.5 and §8.6, and interpretation 21 states why landing the fold is not the revision
  loop DEC-111 item 5 gates. See "Decisions needed" item 6.
- **PRD / HLD / spec anchors:** PRD 6.3 FR-3.9, 6.5; [mandate spec](../../specs/mandate.md) §1.1
  (MI-15 to MI-20), §2.3 (the working universe at runtime and its state machine), §4.3 (the policy
  hierarchy, the internal research profile, and the keys this crate reads through the overlay),
  §5.9 and §5.10 (instrument restrictions and the universe events as risk inputs), §8.1 (the signal
  model contract, the research agent as its one exception, and scorecards), §8.2 (the output shape,
  `expires_at = as_of + horizon_s`, freshness), §8.4 (the research agent: inputs, the thesis fields,
  the cost cap, correlated flow, staggered execution, evidence, the thin slice), §8.5 (admission and
  removal: the 17 ordered checks, refusal, success, removal), §8.6 (thesis lifetime, expiry,
  renewal, invalidation, and the revision lineages), §11 (the reference cases);
  [trading domain spec](../../specs/trading-domain.md) §3.2 (the eligibility floor and the leveraged
  ETP condition), §7.1 (instrument-group claims), §9.6 (the conduct controls the stagger sits
  inside); [journal spec](../../specs/journal.md) §1 and §2 (one stream, one fold; `causation_id`),
  §9 (the `ThesisProposed`, `ThesisRevised`, `UniverseChanged`, and `PlatformOperatorAction` rows);
  [ADR-0002](../../adr/0002-autonomous-ideation-and-retail.md); [HLD](../../HLD.md) §5 (the research
  agent beside the signal models), §8 (operator-level kill switches); ADR-0001 ES-02 (crates and
  layers), ES-04 (decimals), ES-09 (error codes), ES-11 (tests), ES-20 and ES-21 (determinism),
  ES-22 (spec anti-drift).
- **Decisions that apply:** DEC-03, DEC-04, DEC-05 (the mandate binds; models produce opinions, never
  orders; reducing risk needs no approval), DEC-32 (long only in v1), DEC-47 (fixed weights, no
  calibration), DEC-52 and DEC-67 (the signal model contract and no substitution), DEC-72
  (ADR-0001), DEC-77 (brief, tests PR, implementation PR, status PR), DEC-79, DEC-80 (no plain
  comments), DEC-83 (tests PRs hold stubs only), DEC-85 (harness interpretations fail loudly until
  owned), DEC-89 (exact arithmetic), DEC-90 (the research basket is internal data, never a user's
  instrument choice), DEC-97 and DEC-98 (the envelope and strategy split; the retail profile),
  DEC-99 (forward paper is the only evidence of thesis quality), DEC-100 (correlated flow:
  monitoring, the operator per-thesis halt, staggered execution), DEC-101 (the vetted allowlist,
  corroboration, the drift detector, injection fixtures), DEC-102 (counsel now), DEC-103 (the thin
  slice), DEC-110 (every pending test fails on the stubs), DEC-111 (the revision loop and the
  lineage cap), DEC-117 to DEC-126 (**Accepted (founder, 2026-09-26)**; the items this brief depends
  on are listed below), DEC-128 (stream F's types), DEC-129 (stream G's gate), DEC-130 (stream H's
  builder and autonomy), DEC-131 (stream I's runtime), and DEC-132 (this brief's interpretations).
- **The rewrite answers this brief depends on.** The founder confirmed DEC-117 to DEC-126 on
  2026-09-26 ([#133](https://github.com/kunwarshivam/mandate/pull/133)), after the spec PR #109
  merged, so each is settled rather than provisional:
  **DEC-117** (`max_instruments`, the platform ceiling of 20 and the default of 5; check 17),
  **DEC-118** (thesis lifetime: `expires_at = as_of + horizon_s`, no automatic renewal, expiry makes
  the instrument removed; checks 2 and `thesis_expired`), **DEC-120** (the cost cap as an envelope
  field enforced by deterministic code; check 7), **DEC-121** (the pinning switch, which is stream
  F's classification and reaches this crate only as the **envelope field** `universe.pinned`, never as
  `WorkingUniverse::Known { pinned }`; check 5, interpretation 22),
  **DEC-123** (the 900 s stagger minimum and the operator-issued halt; §8.4's offset and check 9),
  **DEC-126** (how a thesis is shown on an approval screen, which fixes what the crate must carry
  into the approval payload: the §8.2 and §8.4 fields, the lineage's revision count, and no price
  target). DEC-119, DEC-122, DEC-124, and DEC-125 do not bear on this crate's rules.

## Scope

### Reference cases

`fixtures/refcases/mandate.json` (the machine form of
[`docs/specs/reference-cases/mandate.yaml`](../../specs/reference-cases/mandate.yaml), `version: 4`,
298 cases). This stream owns **family N, 28 cases**:

| Group | Kind | Cases | Count | Story |
|---|---|---|---|---|
| Admission | `admission` | MC-N01 to MC-N16, MC-N25, MC-N26 | 18 | E17-3, E17-7 |
| Lineage | `lineage` | MC-N17 to MC-N19, MC-N24, MC-N27, MC-N28 | 6 | E17-9 (the fold only) |
| Thesis expiry | `thesis_expiry` | MC-N20 to MC-N22 | 3 | E17-3 (removal) |
| Stagger | `stagger` | MC-N23 | 1 | E17-3 (DEC-100, DEC-123) |

Families owned elsewhere, which this stream never interprets: S, V, P, C, R, T, and L (stream F,
DEC-128); G and F (stream G, DEC-129); A and B (stream H, DEC-130). Stream F's harness dispatches
every family to its owning story from its first tests PR, so no case of any family can silently
pass.

No case moves to `passing` in a tests PR or an implementation PR: `crates/mandate-refcases/status.toml`
is founder-owned and protected, so the `[mandate]` rows for family N arrive in a status PR per
DEC-77, after stream F's `mandate` harness module exists.

### Fixture check before any code

Every figure below was recomputed from the spec text, by hand, before any type was written
(AGENTS.md, "Validate fixtures against the rules"). All agree with the fixture.

| Case | Claim | Recomputed |
|---|---|---|
| MC-N23 | The stagger offset is `SHA-256(workspace_id ‖ 0x00 ‖ thesis_id)` read big-endian, modulo the window | `SHA-256("ws_a\x00th-1")` begins `0x35791baa50c83366…`; that integer mod 900 = **317**. `ws_b`/`th-1` gives **682** and `ws_a`/`th-2` gives **615**. All three match, and all three are below the 900 s window |
| MC-N09 | The cost cap binds at equality | The `research_equity` base sets `cost_cap_usd_per_day: '5'` and the case's spend today is `'5'`, so the comparison is `spend >= cap` (a `>` would admit). `cost_cap_reached` |
| MC-N02 | A full universe never displaces an active instrument | `max_instruments: 5` and the input universe holds five instruments, so check 17 refuses; the universe in `expect` is the input universe unchanged and `universe_size_after` is 5 |
| MC-N01, MC-N14 | The first order's autonomy is the admission ceiling | `order_usd` 300 fails `large_orders` (> 900), `combined_score` 0.8 fails `low_score` (< 0.65), and `routine` matches (`purpose` in `[increase, open]`) and returns `auto`; the `ask` ceiling with `new_instrument: true` tightens it to `ask` `by: admission_ceiling`, `approvers_required` 1 (`two_approver_above_usd` is null), `on_timeout: skip`. MC-N14 is a **renewal** and gets the same ceiling, so `new_instrument` is true on a renewal too |
| MC-N08 | A pinned universe admits nothing, but check **4** decides, not check 5 | `research_equity_pinned` has `research: null` and `admits_instruments: false` on both models, which is what V-036 and V-037 require of a pinned mandate. So `research_disabled` fires before `universe_pinned`, exactly as the fixture expects, and check 5 is unreachable for any **validated** mandate (interpretation 20) |
| MC-N16, MC-N25, MC-N26 | The leveraged-ETP condition is the **exact** disclosure version | The `research_equity_etp` base sets `leveraged_etp_disclosure_version: sha256:bbbb…`. MC-N16 (base `research_equity`, `leveraged_etps_enabled: false`) is refused; MC-N25 accepts `sha256:cccc…`, a different version, and is refused; MC-N26 accepts `sha256:bbbb…` and is admitted. This is V-005's condition (trading spec §3.2) |
| MC-N17 | Revisions 1 to 3 are admitted; revision 4 retires | The `research_equity` base sets `max_revisions_per_lineage: 3`. Revision 0 is admitted and adds `UniverseChanged`; revisions 1, 2, and 3 are **renewals** of the same instrument, so each journals only its `ThesisRevised` entry and the universe stays at size 1. Revision 4 > 3, so `lineage_retired` refuses it and the retirement removes the instrument in the same fold step: size 0. The final lineage state is `{revisions: 3, admitted: 4, retired: true}` — `revisions` is the highest **admitted** revision, `admitted` counts admissions |
| MC-N24 | Retirement removes the instrument the lineage holds | `research_equity_cap_one` sets the cap to 1, so revision 2 is over the cap: step 3 refuses with `lineage_retired` and emits `UniverseChanged` (removed, `lineage_retired`, `universe_size_after: 0`) |
| MC-N27 | An earlier check's refusal retires nothing | Same cap-one base, and the over-cap thesis has `direction: short`, so check **1** (`direction_not_allowed`) decides. The lineage stays `retired: false` at `revisions: 1`, and the universe keeps the instrument: retirement follows the **journaled reason**, never the revision number |
| MC-N28 | Retirement never removes what another lineage now holds | Cap one. Step 3 admits `th-80` (lineage `th-80`, revision 0) for the same instrument as a renewal, which moves the holder from `th-70` to `th-80`. Step 4's revision 2 in lineage `th-70` is over the cap and retires it, but `th-70` holds nothing, so nothing is removed and the universe still holds the instrument at size 1 |
| MC-N19 | A revision with no predecessor is ignored | Revision 1 with `predecessor_thesis_id: null` fails check 3. The entry is journaled as `ThesisRevised` (the type follows the revision number, not the verdict), `ignored` is true, and the lineage exists in the fold's state at `{revisions: 0, admitted: 0, retired: false}` |
| MC-N20 | The horizon removes at exactly the horizon | `now` equals `expires_at`, so the comparison is `now >= expires_at` (a `>` would keep it) and the instrument is removed with reason `thesis_expired` |
| MC-N21, MC-N22 | Invalidation and retirement outrank the horizon | MC-N21's thesis has 20 hours left and `invalidated: true`, so `thesis_invalidated`. MC-N22 has two unexpired theses and `lineages.th-1.retired: true`, so `th-1`'s instrument goes with reason `lineage_retired` and `th-2`'s stays: the removal reason order is invalidated, then retired, then expired |

### Invariants touched

Each gets a named test whose oracle computes the answer its own way (AGENTS.md, "Independent
oracles"; ES-11). Test paths are `crates/mandate-research/tests/`, and **every name below is a test
that exists**: `refcases::mc_n*` are the 25 family-N cases loaded from
`fixtures/refcases/mandate.json`, `rules::*` run unignored because they pin rule logic this PR carries
live (interpretation 23), and everything else is pending until its story lands.

| Clause or invariant | Test |
|---|---|
| §8.5 the checks are evaluated **in order** and the first failure is the journaled reason | `admission::check_1_*` to `admission::check_16_*` (one named case per check, seventeen in all with the renewal case), `admission::checks_reports_all_seventeen_in_the_spec_order`, `admission::the_lower_numbered_check_decides_when_several_fail`, `properties::the_reason_is_the_lowest_numbered_failing_check` |
| §8.5 the ordinals and journaled codes themselves | `rules::every_refusal_reason_carries_its_own_ordinal_once`, `rules::every_refusal_reason_has_the_codes_the_spec_table_states` |
| §8.2, §8.5 checks 1 to 3 are **ignored outputs** as well as refusals | `rules::ignored_outputs_are_exactly_the_first_three_checks`, `admission::check_1_a_short_thesis_is_ignored`, `admission::check_2_an_expiry_that_disagrees_with_the_horizon_is_ignored`, `admission::check_3_a_revision_without_a_predecessor_is_ignored`, `admission::check_3_a_revision_zero_carrying_a_predecessor_is_ignored`, `properties::ignored_is_exactly_the_first_three_checks`, `refcases::mc_n12`, `refcases::mc_n13`, `refcases::mc_n19` |
| §8.5 a refusal admits nothing and leaves the universe as it was, with the one `lineage_retired` exception | `properties::a_refusal_never_grows_the_universe`, `properties::only_a_retirement_lets_a_refusal_change_the_universe`, `refcases::mc_n03` to `refcases::mc_n13`, `refcases::mc_n15`, `refcases::mc_n16`, `refcases::mc_n25` |
| MI-15 the universe never exceeds the effective `max_instruments`, holds no duplicate, and every member passed every check when admitted | `properties::the_universe_never_exceeds_its_ceiling_or_repeats`, `properties::every_member_was_admitted_by_a_passing_check_set`, `refcases::mc_n02` |
| MI-15, §8.5 a **full universe never displaces** an active instrument; `universe_full` is the only check a renewal skips | `admission::mc_n02_a_full_universe_refuses_rather_than_displacing`, `admission::a_renewal_skips_only_the_full_check`, `admission::a_lowered_ceiling_refuses_and_never_removes`, `properties::a_renewal_and_a_first_admission_differ_only_in_check_17`, `refcases::mc_n02` |
| MI-16 admission never changes an envelope field | `properties::admission_changes_no_envelope_field` |
| MI-17 the first order in a newly admitted instrument is at least as strict as `autonomy.admission` | `admission::mc_n01_a_corroborated_thesis_in_an_allowed_asset_class_is_admitted`, `admission::the_internal_research_profile_makes_every_admission_ask_without_refusing_any`, `rules::the_admission_ceiling_only_ever_tightens`, `properties::first_order_facts_always_carry_new_instrument_and_the_ceiling` (the strictness comparison itself is stream H's, interpretation 3) |
| MI-18 a lineage never admits a revision past `max_revisions_per_lineage`, and no revision carries a predecessor's score | `lineage::mc_n17_revisions_one_to_three_are_admitted_and_the_fourth_retires_the_lineage`, `lineage::mc_n18_a_revision_never_carries_its_predecessor_score_forward`, `admission::check_16_a_revision_at_the_cap_is_admitted`, `admission::check_16_a_revision_past_the_cap_is_refused`, `admission::check_16_a_policy_ceiling_lowers_the_revision_cap`, `properties::a_lineage_never_admits_past_its_cap`, `refcases::mc_n17`, `refcases::mc_n18`, `refcases::mc_n24` |
| §8.6 item 4 retirement follows the **journaled refusal reason**, not the revision number | `lineage::mc_n27_an_over_cap_revision_an_earlier_check_refuses_retires_nothing`, `properties::retirement_happens_exactly_on_a_lineage_retired_refusal`, `refcases::mc_n27` |
| §8.6 item 4 retirement removes the instrument the lineage holds, in the same fold step, and never one another lineage now holds | `lineage::mc_n24_retirement_removes_the_instrument_it_holds`, `lineage::mc_n17_the_retiring_step_journals_the_removal_with_the_size_after`, `lineage::mc_n28_retirement_never_removes_an_instrument_another_lineage_now_holds`, `lineage::a_lineage_retires_and_removes_only_once`, `properties::retirement_removes_at_most_its_own_holder`, `refcases::mc_n24`, `refcases::mc_n28` |
| MI-19 exactly the theses that expired, were invalidated, or whose lineage retired remove their instrument | `expiry::mc_n20_a_thesis_at_its_horizon_removes_its_instrument`, `expiry::mc_n21_an_invalidated_thesis_removes_at_once_before_its_horizon`, `expiry::mc_n22_a_retired_lineage_removes_its_instrument_and_an_unexpired_thesis_stays`, `properties::exactly_the_ended_theses_remove_their_instrument`, `refcases::mc_n20` to `refcases::mc_n22` |
| §8.6 expiry is inclusive at the horizon, and the reason order is invalidated, retired, expired | `expiry::the_horizon_removes_at_exactly_the_horizon_and_not_before`, `expiry::the_removal_reason_is_the_first_that_holds`, `expiry::a_retired_lineage_outranks_an_expired_horizon`, `properties::the_removal_reason_is_the_first_that_holds` |
| MI-19 removal restricts that instrument only | `properties::a_removal_touches_no_other_instruments_restriction`, `expiry::several_removals_count_the_universe_down` |
| MI-20 a pinned universe admits nothing | `admission::check_5_a_pinned_universe_admits_nothing_even_with_an_admitting_model`, `admission::mc_n08_a_pinned_validated_mandate_refuses_at_check_four`, `properties::no_pinned_mandate_ever_admits`, `refcases::mc_n08` |
| §8.4, DEC-100, DEC-123 the stagger offset is deterministic, depends on both ids, and is inside the window | `stagger::mc_n23_the_three_fixture_offsets`, `stagger::both_ids_change_the_offset`, `stagger::the_digest_is_the_workspace_a_zero_byte_and_the_thesis`, `stagger::a_zero_window_gives_a_zero_offset`, `stagger::a_one_second_window_gives_a_zero_offset`, `properties::an_offset_is_below_its_window`, `refcases::mc_n23` |
| §8.4 the offset is counted from the admission for crypto and from the **later** of the admission and the next regular open for equities | `stagger::an_equity_release_waits_for_the_later_of_the_two_instants`, `stagger::an_equity_admitted_inside_the_session_counts_from_the_admission`, `stagger::a_crypto_release_is_counted_from_the_admission`, `stagger::a_zero_offset_releases_at_the_anchor`, `stagger::an_equity_without_a_next_regular_open_is_an_error`, `properties::a_release_is_never_before_its_anchor` |
| §8.4 the proposal cadence is a policy minimum, and the crate reads no clock | `stagger::the_next_proposal_is_one_interval_after_the_last`, `stagger::an_agent_that_has_never_proposed_may_propose_now`, `rules::the_proposal_interval_takes_the_higher_of_the_two_bounds` |
| §8.4, DEC-120 the cost cap binds at equality and refuses new theses only | `admission::check_7_the_cost_cap_binds_at_equality`, `admission::check_7_a_cent_below_the_cap_still_admits`, `rules::the_cost_cap_takes_the_lower_of_the_two_amounts`, `properties::the_cost_cap_never_touches_an_existing_entry`, `refcases::mc_n09` |
| DEC-101 every cited source must be on the allowlist, and corroboration is required | `admission::check_14_one_source_off_the_allowlist_refuses_the_whole_thesis`, `admission::check_15_a_thesis_without_corroboration_is_refused`, `admission::check_15_market_data_corroboration_admits_a_thesis_citing_no_sources`, `properties::a_source_off_the_allowlist_never_admits` (stated over theses citing at least one source, because an empty list passes check 14 vacuously; "Decisions needed" item 7), `properties::an_uncorroborated_thesis_never_admits`, `refcases::mc_n06`, `refcases::mc_n07` |
| DEC-101, R-05 no string in a thesis can change a verdict (the prompt-injection property) | `properties::text_never_changes_a_verdict` (the invalidation prose, an allowlisted source's name, and the thesis's own ids), `properties::text_never_changes_a_fold` |
| DEC-103 the thin slice admits only instruments in the pinned data universe, with every admission `ask` | `admission::check_8_the_thin_slices_data_universe_refuses_anything_outside_it`, `properties::a_pinned_data_universe_is_a_superset_of_every_admission`, `rules::the_internal_research_profile_carries_the_thin_slices_own_values`, `refcases::mc_n11` |
| §8.5 checks 10 and 11 read instrument reference data, never the thesis's own claim | `admission::check_10_reads_reference_data_and_a_thesis_has_no_asset_class_to_claim`, `admission::check_11_the_opt_in_alone_does_not_admit_a_leveraged_etp`, `admission::mc_n25_a_leveraged_etp_with_a_different_accepted_disclosure_version_is_refused`, `admission::mc_n26_a_leveraged_etp_with_the_opt_in_and_the_exact_disclosure_is_admitted`, `refcases::mc_n16`, `refcases::mc_n25` |
| trading spec §7.1 the group claim covers an ungrouped instrument as its own group | `admission::check_13_an_instrument_group_claimed_by_another_agent_is_refused`, `admission::check_13_a_group_claimed_through_a_sibling_refuses`, `admission::check_13_an_ungrouped_instrument_is_its_own_group_and_a_group_id_spelling_an_asset_id_claims_nothing`, `rules::a_named_group_wins_and_an_unnamed_instrument_is_its_own_group`, `rules::a_group_id_spelling_an_asset_id_is_not_that_instruments_group`, `rules::an_empty_group_map_leaves_every_instrument_ungrouped`, `refcases::mc_n05` |
| §4.3 each key the overlay resolves tightens in its own direction (interpretation 22) | `rules::a_maximum_takes_the_lower_of_the_two_bounds`, `rules::the_revision_cap_takes_the_lower_of_the_two_bounds`, `rules::a_minimum_takes_the_higher_of_the_two_bounds`, `rules::the_admission_ceiling_only_ever_tightens`, `rules::the_permissive_overlay_constrains_nothing_and_permits_both`, `admission::check_4_a_policy_that_forbids_the_research_agent_refuses`, `admission::check_16_a_policy_ceiling_lowers_the_revision_cap` |
| §2.3, §5.3 an unavailable working universe is an error, never an empty set | `admission::an_unavailable_universe_is_an_error` |
| MI-8, ES-21 the same inputs give identical outputs; no clock, no randomness, order-independent | `properties::identical_inputs_give_identical_admissions_and_events`, `properties::no_output_depends_on_iteration_order`, `expiry::the_removal_order_does_not_depend_on_the_input_order` |
| §8.5, §8.6 every state change emits its journal entry, and nothing changes without one | `properties::the_universe_equals_the_fold_of_the_emitted_events` (oracle 1), `properties::every_step_emits_exactly_one_thesis_entry`, `admission::mc_n01_admission_journals_the_thesis_entry_and_the_universe_change` |
| journal spec §9 the entry type follows the revision number, and `predecessor_thesis_id` is present exactly when `revision > 0` | `admission::the_entry_type_follows_the_revision_number`, `lineage::mc_n19_a_revision_without_a_predecessor_id_is_ignored_and_admits_nothing`, `properties::every_step_emits_exactly_one_thesis_entry`, `refcases::mc_n19` |
| §8.2, DEC-118 `expires_at` equals `as_of + horizon_s` | `admission::the_horizon_pairs_as_of_with_expires_at_exactly`, `admission::check_2_an_expiry_that_disagrees_with_the_horizon_is_ignored`, `refcases::mc_n13` |
| An input that makes a decision impossible is a typed error, never a verdict | `admission::an_unavailable_universe_is_an_error`, `lineage::a_fold_that_names_one_thesis_twice_is_an_error`, `expiry::an_entry_list_holding_one_instrument_twice_is_an_error`, `stagger::an_equity_without_a_next_regular_open_is_an_error` |
| Every family-N case is read from the fixture, and every `expect` key it states is compared (DEC-85) | `refcases::assert_every_key_known` (called by each case), `refcases::every_family_n_case_is_covered`, `refcases::the_three_cases_this_file_defers_are_named` |

**Hand-tested only**, with no property: the three cases that state `first_order_autonomy` (MC-N01,
MC-N14, MC-N26), because the ceiling's *effect* is stream H's `classify`; the digest's byte layout
(`stagger::the_digest_is_the_workspace_a_zero_byte_and_the_thesis`), which is one hash and has nothing
to generalise over; and the four `properties::*_oracle_*` self-checks, which exist to fail on a seeded
bug rather than to hold over generated input.

### Oracles

Four independent oracles, each shown to fail on a seeded bug before it is trusted:

1. **The universe from the events.** `crates/mandate-research/tests/properties.rs` holds a second
   reader that rebuilds the working universe **only** from the `UniverseChanged` entries each step
   emitted, and asserts it equals the universe the step reported. An admission that grows the
   universe without journaling, or a journaled removal that did not happen, fails. It also rebuilds
   `universe_size_after` independently, which is what catches a size counted before the change
   rather than after.
2. **The check set, computed the other way.** The oracle evaluates the 17 predicates as an
   **unordered set** from the raw inputs — written from the §8.5 table, sharing no code with the
   implementation — and then takes the minimum failing check number. The implementation walks in
   order and stops at the first failure. Both must name the same reason, so a check evaluated out of
   order is caught even when the result happens to agree. Every property first asserts that at least
   one check failed in the cases that expect a refusal, so no property can pass on an empty verdict.
3. **The lineage counter.** A separate accumulator walks the thesis sequence and tracks, per lineage
   — keyed by the lineage id the thesis carries, so a sequence of first theses is not collapsed into
   one — the highest admitted revision, the number of admissions, retirement, which instrument each
   lineage holds, and therefore the removal set. `properties::a_lineage_never_admits_past_its_cap`
   compares **all of them** against the fold: every step's `lineage_revisions` and `lineage_retired`,
   then each `Lineage`'s `revisions`, `admitted` and `retired`, then the journaled removals against
   the ones the counter derived. `properties::the_lineage_oracle_catches_an_admission_after_retirement`
   seeds the "retire on any refusal" bug and watches the counter catch it.
4. **The modular reduction, least-significant-first.** The implementation folds the 32 digest bytes
   most-significant-first, accumulating `acc = (acc × 256 + byte) mod window`. The oracle walks the
   bytes from the least significant end with its own table of `256^k mod window`, so the two share no
   arithmetic. Both must give MC-N23's three offsets and agree over generated windows and ids.

### Crates

One new crate:

| Crate | Layer | Holds | Why not elsewhere |
|---|---|---|---|
| `mandate-research` | 5 | The thesis as typed data, the §8.5 checks, the §8.6 lineage fold and expiry, the §8.4 stagger and proposal cadence, and the three journal entries | Not a module of `mandate-spec` (layer 3): that crate is the **document** and must sit below the gate (layer 4) so the gate reads rules it cannot define. Admission is runtime state that reads the document, the policy overlay, and the folded universe, and it produces account-stream events; putting it at layer 3 would let the gate see admission, and would give one crate two jobs with two claims on the same universe. Layer 5, beside `mandate-builder`, is the lowest free layer above `mandate-spec`, and a sibling of the builder rather than a dependant of it (interpretation 3) |

`pure = true`, `safety_critical = true`, `allowed_external = ["thiserror"]`. It depends on
`mandate-num` (`Usd` for the cost cap), `mandate-time` (`UtcNanos` for horizons and the stagger
anchor), `mandate-canon` (`Digest::of_parts` for §8.4's SHA-256, so no `sha2` dependency is added),
`mandate-domain` (`AssetId`, `AssetClass`, `WorkingUniverse`, `AutonomyDecision`), and
`mandate-spec` (`ValidatedMandate`, `PolicyOverlay`, `SchemaDec`, the universe change and reason
types). `xtask/layers.toml` and `CODEOWNERS` are founder-owned, so the two entries the tests PR adds
are the items this brief cannot take alone ("Decisions needed" item 1).

- **Crates out of scope:** `mandate-spec` and `mandate-domain` (stream F owns them; this stream
  consumes their types and changes nothing), `mandate-risk` (stream G owns the gate, the eligibility
  floor, and the re-check of the working universe), `mandate-builder` (stream H owns §6.2 and §8.3),
  `mandate-runtime` (stream I sequences the calls and does the journaling), `mandate-journal`,
  `mandate-accounting`, `mandate-sim`, `mandate-cli`, and `python/research_spike` (a spike; nothing
  here depends on it).
- **Touched later, not in this stream's tests PR:** `mandate-refcases` gains the family-N
  interpretations (`admission`, `lineage`, `thesis_expiry`, `stagger`) in a harness PR after stream
  F's `mandate` module exists, and `status.toml` moves in a status PR.
- **New dependencies allowed:** none. `thiserror` and `proptest` are registered; the
  `docs/dependencies.md` "Used by" cells gain `mandate-research` in the tests PR.
- **Safety-critical:** yes. Admission is the path a bad model output travels, so it is in AGENTS.md's
  safety-critical list under "Autonomy policy" and "Risk gate … eligibility". DEC-77 sequence: brief
  PR (this one, docs only), tests PR (crate, stubs, pending tests), implementation PR (test files
  change only by deleting `#[ignore = "pending <story>"]` lines), harness PR, status PR.
- **Size budget:** 400 non-generated lines of crate code per PR (ES-13). The tests PR may exceed it
  for test code and states its split.

## Data shapes

The caller's view, written before any logic; these are the tests PR's stubs, all in
`mandate-research`.

### The thesis and the model output

```rust
pub struct ThesisId(String);
pub struct LineageId(String);
pub struct SourceId(String);
pub struct WorkspaceId(String);
pub struct AllowlistVersion(u32);

/// §8.2's `direction`. Anything but `Long` is an ignored output in v1 (DEC-32), so the enum keeps a
/// variant for what a model may send rather than refusing to parse it.
pub enum Direction { Long, Other }

/// §8.4, DEC-101: how the platform corroborated the thesis, never what the model asserted
/// (interpretation 7).
pub enum Corroboration { IndependentSource, MarketData }

/// §8.1's pinned identity and §8.2's freshness envelope. Whether the output is *fresh* is stream H's
/// combine step; this crate reads only `as_of` and `expires_at` for check 2.
pub struct OutputEnvelope {
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub content_hash: ContentHash,
    pub as_of: UtcNanos,
    pub expires_at: UtcNanos,
}

/// §8.4's thesis. It carries what the agent said; every fact about the *instrument* is in
/// `InstrumentFacts` and every fact about the evidence is in `ProposedThesis::corroboration` and
/// `AdmissionFacts::allowlist`.
pub struct Thesis {
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub revision: u32,
    pub predecessor_thesis_id: Option<ThesisId>,
    pub instrument_id: AssetId,
    pub output: OutputEnvelope,
    pub direction: Direction,
    pub horizon_s: u32,
    pub conviction: SchemaDec,
    pub confidence: SchemaDec,
    pub evidence: Option<Digest>,
    pub evidence_sources: Vec<SourceId>,
    pub invalidation: Invalidation,
}

/// §8.6's invalidation conditions. v1 stores the text and the evaluated verdict the caller supplies;
/// evaluating a condition against market data is E17-3's runtime work, not this crate's
/// (interpretation 13).
pub struct Invalidation(String);
```

### Admission

```rust
/// Instrument reference data (trading spec §3.2). It holds **no id**: the facts reach a check only
/// through the [`ProposedThesis`] that binds them to their thesis, so there is no pair of ids that
/// could disagree (interpretation 6, and the review's item 2 taken at the higher rung).
pub struct InstrumentFacts {
    pub asset_class: AssetClass,
    pub leveraged_or_inverse_etp: bool,
}

/// An instrument with no named group is its own group, made explicit so a group id that spells an
/// asset id cannot claim it (interpretation 12).
pub enum InstrumentGroup { Named(GroupId), Ungrouped(AssetId) }

/// One thesis with the platform's facts about it: the instrument's reference data, and the
/// corroboration the platform found, **per thesis** rather than per fold, so a sequence with mixed
/// corroboration is representable (the review's item 1).
pub struct ProposedThesis {
    pub thesis: Thesis,
    pub instrument: InstrumentFacts,
    pub corroboration: Option<Corroboration>,
}

/// DEC-101, E17-7: the vetted allowlist and its version, versioned configuration shared by every
/// thesis in a fold.
pub struct SourceAllowlist {
    pub version: AllowlistVersion,
    pub sources: BTreeSet<SourceId>,
}

/// The facts checks 7 to 13 read. Each is produced elsewhere: the eligibility failures by
/// `mandate-risk`'s floor (E6-7), the group map and the claims by the account ledger (trading spec
/// §7.1) **in the shape stream F's `ValidationContext` already supplies** (the review's item 4), the
/// halts by the operator service (E17-6), the disclosures by the consent record, the data universe by
/// the profile (DEC-103), and the spend by the cost accounting (DEC-120).
pub struct AdmissionFacts {
    pub allowlist: SourceAllowlist,
    pub eligibility_failures: BTreeSet<AssetId>,
    pub instrument_groups: BTreeMap<AssetId, GroupId>,
    pub claimed_by_other_agents: BTreeSet<AssetId>,
    pub halted_instruments: BTreeSet<AssetId>,
    pub disclosures_accepted: BTreeSet<Digest>,
    /// DEC-103: `Some` pins a data universe (the research basket); `None` pins none.
    pub data_universe: Option<BTreeSet<AssetId>>,
    /// DEC-120: the risk day's research spend so far, whose day boundary is `mandate_spec::risk_day`.
    pub research_spend_usd_today: Usd,
}

/// The group each check 13 comparison uses, derived inside from F's two inputs.
pub fn group_of(instrument: &AssetId, groups: &BTreeMap<AssetId, GroupId>) -> InstrumentGroup;

pub struct AdmissionInput<'a> {
    pub mandate: &'a ValidatedMandate,
    pub overlay: &'a PolicyOverlay,
    pub universe: &'a WorkingUniverse,
    pub proposal: &'a ProposedThesis,
    pub facts: &'a AdmissionFacts,
    pub lineages: &'a LineageState,
}

/// §8.5's 17 checks, in the spec's order, each with its own verdict. `admit` takes the first
/// failure; `checks` reports all of them, which is what makes "in order" testable
/// (interpretation 4).
pub struct Check { pub number: u8, pub reason: RefusalReason, pub failed: bool }

pub enum RefusalReason {
    DirectionNotAllowed, HorizonMismatch, RevisionWithoutPredecessor,
    ResearchDisabled, UniversePinned, AdmissionDenied, CostCapReached,
    NotInDataUniverse, OperatorHalt, NotAllowedAssetClass, LeveragedEtpNotEnabled,
    EligibilityFloor, InstrumentGroupClaimed, SourceNotAllowlisted, NoCorroboration,
    LineageRetired, UniverseFull,
}

impl RefusalReason {
    /// The journaled reason (ES-09): "direction_not_allowed" .. "universe_full".
    pub fn code(self) -> &'static str;
    /// The §8.5 ordinal, 1 to 17.
    pub fn check_number(self) -> u8;
    /// §8.2: checks 1 to 3 are ignored outputs as well as refusals (interpretation 5).
    pub fn is_ignored_output(self) -> bool;
}

pub enum AdmissionChange { Admitted, Renewed }

pub enum AdmissionDecision {
    Admitted { change: AdmissionChange },
    Refused { reason: RefusalReason },
}

/// What stream H's `classify` needs for the first order in the instrument (§8.5, §6.2). Admission
/// does not decide it (interpretation 3).
pub struct FirstOrderFacts {
    pub new_instrument: bool,
    pub thesis_confidence: SchemaDec,
    pub admission_ceiling: AutonomyDecision,
}

pub struct Admission {
    pub decision: AdmissionDecision,
    pub universe: WorkingUniverse,
    pub journal: Vec<ResearchEvent>,
    /// `Some` exactly when the decision is `Admitted`.
    pub first_order: Option<FirstOrderFacts>,
}

pub fn checks(input: &AdmissionInput<'_>) -> Result<Vec<Check>, ResearchError>;
pub fn admit(input: &AdmissionInput<'_>) -> Result<Admission, ResearchError>;
```

### Lineages, retirement, and expiry

```rust
pub struct Lineage { pub revisions: u32, pub admitted: u32, pub retired: bool }

/// §8.6's folded state: what each lineage has admitted, whether it retired, and which instrument it
/// holds. Retirement is read from here, never asserted per instrument.
pub struct LineageState { /* private: two BTreeMaps */ }

impl LineageState {
    pub fn empty() -> Self;
    pub fn lineage(&self, id: &LineageId) -> Option<&Lineage>;
    pub fn holder_of(&self, id: &LineageId) -> Option<&AssetId>;
    pub fn held_by(&self, instrument: &AssetId) -> Option<&LineageId>;
}

pub struct FoldInput<'a> {
    pub mandate: &'a ValidatedMandate,
    pub overlay: &'a PolicyOverlay,
    pub universe: &'a WorkingUniverse,
    pub proposals: &'a [ProposedThesis],
    pub facts: &'a AdmissionFacts,
    pub lineages: &'a LineageState,
}

pub struct FoldStep {
    pub thesis_id: ThesisId,
    pub decision: AdmissionDecision,
    pub lineage_revisions: u32,
    pub lineage_retired: bool,
    pub journal: Vec<ResearchEvent>,
}

impl FoldStep {
    /// MI-18: always false. No type in this crate accepts a predecessor's score, so carrying one
    /// forward is unrepresentable rather than merely tested (interpretation 16).
    pub fn score_carried_forward(&self) -> bool;
}

pub struct Fold {
    pub steps: Vec<FoldStep>,
    pub lineages: LineageState,
    pub universe: WorkingUniverse,
}

pub fn fold_theses(input: &FoldInput<'_>) -> Result<Fold, ResearchError>;

/// One instrument's current thesis, as the universe fold holds it.
pub struct UniverseEntry {
    pub instrument: AssetId,
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub revision: u32,
    pub expires_at: UtcNanos,
    pub invalidated: bool,
}

pub struct Expiry {
    pub universe: WorkingUniverse,
    pub removed: Vec<AssetId>,
    pub instrument_restrictions: BTreeMap<AssetId, InstrumentRestriction>,
    pub journal: Vec<ResearchEvent>,
}

pub fn expire_theses(
    now: UtcNanos,
    entries: &[UniverseEntry],
    lineages: &LineageState,
) -> Result<Expiry, ResearchError>;
```

### Timing: the stagger and the proposal cadence

```rust
/// §8.4, DEC-123: a policy minimum of 900 s. Zero means no wait.
pub struct StaggerWindow(u32);

/// §8.4: SHA-256(workspace_id ‖ 0x00 ‖ thesis_id), big-endian, modulo the window, in whole seconds.
pub fn stagger_offset(
    workspace: &WorkspaceId,
    thesis: &ThesisId,
    window: StaggerWindow,
) -> Result<u32, ResearchError>;

/// Equities count from the later of the admission and the next regular-session open; crypto counts
/// from the admission. `next_regular_open` is required for an equity and ignored for crypto.
pub fn stagger_release_at(
    asset_class: AssetClass,
    admitted_at: UtcNanos,
    next_regular_open: Option<UtcNanos>,
    offset_s: u32,
) -> Result<UtcNanos, ResearchError>;

/// §8.4's `behavior.research.interval_s`, a policy minimum. The scheduler (stream I) calls it; the
/// crate reads no clock (ES-21).
pub fn next_proposal_at(
    last_proposal: Option<UtcNanos>,
    interval_s: u32,
) -> Result<Option<UtcNanos>, ResearchError>;
```

### Journal entries

```rust
/// journal spec §9. The crate returns values; appending them, their canonical payloads, and the
/// `causation_id` that ties a `UniverseChanged` in the account stream to its thesis entry in the
/// agent stream are the executor's (interpretation 17).
pub enum ResearchEvent {
    ThesisProposed(ThesisEntry),
    ThesisRevised(ThesisEntry),
    UniverseChanged(UniverseChangedEntry),
}

pub struct ThesisEntry {
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub revision: u32,
    pub predecessor_thesis_id: Option<ThesisId>,
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub direction: Direction,
    pub horizon_s: u32,
    pub conviction: SchemaDec,
    pub confidence: SchemaDec,
    pub evidence: Option<Digest>,
    pub evidence_sources: Vec<SourceId>,
    pub corroboration: Option<Corroboration>,
    pub invalidation: Invalidation,
    pub allowlist_version: AllowlistVersion,
    pub admitted: bool,
    pub reason: Option<RefusalReason>,
}

pub struct UniverseChangedEntry {
    pub instrument: AssetId,
    pub change: UniverseChange,
    pub reason: UniverseChangeReason,
    pub thesis_id: ThesisId,
    pub lineage_id: LineageId,
    pub universe_size_after: usize,
}
```

`UniverseChange`, `UniverseChangeReason`, `InstrumentRestriction`, `AutonomyDecision`, `AssetId`,
`AssetClass`, `GroupId`, `SchemaDec`, `ValidatedMandate`, `PolicyOverlay`, and `WorkingUniverse` belong
to `mandate-domain` or `mandate-spec`, and `ModelId` is stream F's too (its `ValidationContext` names
it); `Digest` is `mandate-canon`'s. **`ModelVersion` and `ContentHash` are the exception:** stream H's
brief names both, and stream H is a sibling at layer 5 this crate may not depend on, so they are
declared in `spec_types.rs` and the coordinator settles where they live when F's crate lands
(interpretation 15). Until `mandate-spec` and `mandate-domain` exist, all of these are narrow views in
that temporary module, which the first implementation PR after F's tests PR deletes.

### Errors

`ResearchError` is a `thiserror` enum, each variant with a stable `code()` (ES-09):
`universe_unavailable` (the `UniverseChanged` fold has not been read, so nothing may be admitted —
never treated as an empty universe, interpretation 18), `duplicate_instrument` (an input universe
holding a repeat, which MI-15 forbids), `duplicate_thesis_id` (a fold input naming one thesis twice),
`session_calendar_missing` (an equity stagger anchor with no next regular open),
`window_too_large`, `interval_too_large`, `time_out_of_range` (a horizon or an offset that leaves
`UtcNanos`'s range), `out_of_range` (a decimal outside the arithmetic range, as DEC-128 item 4
requires: a typed error naming the field, never a rounded number), and `unimplemented` (the tests
PR's stubs only; it is gone from the crate by the implementation PR, which the pending gate proves by
requiring every pending test to fail on the stubs).

## Interpretations (recorded as DEC-132)

1. **One new crate, `mandate-research`, layer 5, safety-critical and pure.** The reasoning is the
   crate table above. `allowed_external = ["thiserror"]` only: §8.4's SHA-256 comes from
   `mandate_canon::Digest::of_parts`, so no `sha2` is added, and the modular reduction needs no
   big-integer crate (item 8). The alternative — a module of `mandate-spec` — is rejected because
   the document crate must stay below the gate, and the alternative of layer 6 with a dependency on
   `mandate-builder` is rejected by item 3. `xtask/layers.toml` and `CODEOWNERS` are founder-owned,
   so the placement is "Decisions needed" item 1.
2. **The crate never calls a model.** A thesis arrives as a `Thesis` value. There is no HTTP client,
   no prompt, no artifact store, no provider identity beyond the pinned `OutputEnvelope` the caller
   fills, and no `ModelInvocationRecorded`. `python/research_spike/src/research_spike/propose.py`
   shows the shape today's LLM produces and the strict validation that turns raw JSON into it; the
   platform version of that boundary is E17-2's shell story. This keeps the whole §8.5 decision
   inside a pure function that a fuzzer can drive, which is what E17-3's acceptance clause asks for.
3. **Admission does not call the autonomy classifier.** `admit` returns `FirstOrderFacts`
   (`new_instrument`, `thesis_confidence`, and the overlay's effective `autonomy.admission`) and the
   caller composes stream H's `classify`. §8.5 says the first order "is then decided by the autonomy
   rules … admission does not itself authorize a trade", and §6.2's evaluation is stream H's by
   DEC-130. Stream H's merged brief states that stream J **supplies** `new_instrument` and
   `thesis_confidence`, so a code dependency from J to H would invert the data flow and force this
   crate to layer 6. `new_instrument` is `true` on a renewal as well as a first admission, because
   MC-N14 expects the ceiling there. The family-N harness composes the two crates for the three cases
   that state `first_order_autonomy` ("Decisions needed" item 5).
4. **Every §8.5 check is a total predicate, so `checks` evaluates all 17 and `admit` takes the first
   failure.** The reference's generator is lazy, but nothing depends on laziness: no predicate can
   fail, diverge, or read state another predicate changes. Evaluating all of them makes the *order*
   testable — one hand test per check asserts the whole verdict vector, not only the winning reason —
   and oracle 2 compares the minimum failing number against an unordered set computed its own way.
   `Check { number, reason, failed }` keeps the §8.5 ordinal in the value, and
   `RefusalReason::check_number` is the single place that ordinal is written down.
5. **Checks 1 to 3 are refusals that also report `ignored`.** `direction_not_allowed`,
   `horizon_mismatch`, and `revision_without_predecessor` are §8.2's ignored outputs and §8.5's first
   three checks at once: the crate journals the thesis entry with `admitted: false` and the reason,
   and `RefusalReason::is_ignored_output` is the one predicate that says so, so no caller re-lists
   the three codes.
6. **`asset_class` and the leveraged-ETP flag are instrument reference data, not fields of the
   thesis.** The fixture carries both inside its thesis object because the reference's `admit` takes
   one dictionary, but the classification comes from the instrument reference data (trading spec
   §3.2), and letting the agent's own output declare either would let a thesis talk its way past
   check 10 or check 11 — which MI-16 forbids. So `Thesis` has neither field, `InstrumentFacts`
   carries both, the journal entry records reference data's asset class, and the hole is
   unrepresentable rather than tested (trust ladder rung 1). Every family-N case agrees, because in
   every case the thesis's claim and the instrument's data are the same; the harness interpretation
   fills `InstrumentFacts` from the case's thesis fields and says so.

   **`InstrumentFacts` holds no id either.** The review asked for an `instrument_facts_mismatch`
   error beside the fold's `instrument_facts_missing`, for the case where a thesis's `instrument_id`
   and its facts' id disagree. Both errors are gone instead: `ProposedThesis` binds a thesis to its
   facts, `InstrumentFacts` has no id to disagree with, and a fold takes `&[ProposedThesis]` rather
   than a thesis list beside a facts map, so a thesis without its facts is unrepresentable too. That
   is the same finding taken at rung 1 of the trust ladder rather than rung 2, which AGENTS.md
   prefers where a type can hold the rule.
7. **Corroboration is platform-derived, per thesis, not agent-asserted.** §8.4's table lists
   `corroboration` among the thesis's fields and DEC-101 says it is *recorded* in `ThesisProposed`.
   Read as an agent-asserted field, check 15 is satisfied by a model writing `independent_source`
   into its own output, which is exactly the planted-source attack DEC-101 exists to stop (R-05). So
   `ProposedThesis::corroboration` is what the platform found — an allowlisted source independent of
   the primary one, or market data consistent with the thesis — and check 15 requires it to be
   `Some`. It sits on the **proposal**, not on the fold, so a sequence in which one thesis is
   corroborated by market data and the next by a source is representable, exactly as the reference's
   per-thesis reading requires (the review's item 1); only the allowlist itself is shared, because it
   is one versioned document. Every case agrees: the reference checks only that a kind is present,
   and the harness fills the fact from the case's `corroboration.kind`. What the platform must do to
   *earn* that fact is E17-7's data-plane work, named under "Not done", and "Decisions needed" item 2
   asks for the one-word spec clarification.
8. **The stagger reduction is exact integer arithmetic in this crate, with no new dependency.**
   `Digest::of_parts(&[workspace.as_bytes(), &[0x00], thesis.as_bytes()])` is §8.4's
   `SHA-256(workspace_id ‖ 0x00 ‖ thesis_id)`. Reading it big-endian modulo the window is a byte fold
   in `u64`: with `acc < window ≤ u32::MAX`, `acc × 256 + 255 < 2^40`, so every step is a
   `checked_mul` and a `checked_add` that cannot overflow, and the result is the same integer a
   256-bit type would give. ES-04 keeps *decimal* arithmetic in `mandate-num`; this is neither money
   nor a quantity, so it stays beside the rule it serves, with oracle 4 and a hand-computed check
   against MC-N23's three offsets. A window of 0 returns 0 (§8.4: "a window of 0 means no wait").
9. **The fold is the only place a lineage retires.** Retirement fires exactly when a step's journaled
   refusal reason is `lineage_retired` and the lineage was not already retired (§8.6 item 4). A thesis
   an earlier check refused retires nothing, whatever its revision number (MC-N27), and a thesis over
   the cap that check 16 refuses while the lineage is *already* retired adds no second removal.
10. **A retired lineage keeps its holder entry.** MC-N17 and MC-N24 both keep `lineage_instruments`
    pointing at the removed instrument after the retirement. That is right for replay: the map records
    which lineage held what, not what the universe holds now. A second removal is impossible because
    `retired` is already true, so a stale holder can never remove twice, and a lineage loses its
    holder entry the moment another lineage's thesis for that instrument is admitted (MC-N28).
11. **A renewal journals no `UniverseChanged`.** An admitted thesis for an instrument already in the
    universe reports `change: Renewed`, journals only its thesis entry, and leaves
    `universe_size_after` at the current size. `universe_full` is the only check a renewal skips, so
    a renewal and a first admission differ in exactly one predicate — which a property asserts, so no
    second difference can creep in.
12. **An ungrouped instrument is its own group, said explicitly.** The reference defaults the group
    to the instrument id, which works only because ids and group names never collide in the fixture.
    `InstrumentGroup::{Named, Ungrouped}` makes the default a distinct case, so a group id that spells
    an asset id cannot claim that instrument (trust ladder rung 1). Check 13 compares groups, never
    strings.
13. **Invalidation is evaluated by the caller; the crate reads the verdict.** §8.6 says an instrument
    becomes removed "when an invalidation condition holds". Evaluating a condition needs market data
    and news, which a pure crate has no business assembling, so `UniverseEntry::invalidated` is the
    verdict and `Invalidation` keeps the text for the journal and the approval screen (DEC-126). The
    crate therefore decides *what removal follows*, never *whether the world changed*.
14. **A lowered ceiling refuses and never removes.** The overlay's effective `max_instruments` enters
    check 17 alone. A universe already above a lowered ceiling drains by expiry; no code path removes
    an instrument to fit a new ceiling, because a refusal admits nothing and changes the universe only
    in the one `lineage_retired` case (§8.6 item 4, MI-19). This matches the policy-overlay paragraph
    of stream F's brief, and is planted bug 6.
15. **Shared types have one home, and the tests PR borrows it rather than forking it.**
    `UniverseChange`, the universe change reason, `InstrumentRestriction`, `AutonomyDecision`,
    `AssetId`, `AssetClass`, `GroupId`, `SchemaDec`, `ValidatedMandate`, `PolicyOverlay`, and
    `WorkingUniverse` belong to `mandate-domain` or `mandate-spec` (DEC-128 item 21's mechanism).
    Neither crate exists on `main` yet, so the tests PR carries them as narrow views in a temporary
    `crates/mandate-research/src/spec_types.rs`, field for field the same as stream G's module of the
    same name, and the first implementation PR after stream F's tests PR merges **deletes it**.

    On the reason enum, this brief was **wrong when it was written**: it said stream F's
    `RemovalReason` could not carry `thesis_admitted`, reading F's brief rather than F's code. F has
    since shipped `mandate_spec::risk::RemovalReason` with **all seven** reasons of journal spec §9's
    `UniverseChanged` row, `ThesisAdmitted` included, so there is no variant to add and this crate's
    `UniverseChangeReason` is that enum under another name. The implementation PR takes F's as it
    stands; whether it is worth renaming is the coordinator's call ("Decisions needed" item 4).

    **`ModelVersion` and `ContentHash` are the exception.** Stream H's brief names both, and stream H
    is a sibling at layer 5 that this crate may not depend on, so they are declared here. `ModelId` is
    stream F's (its `ValidationContext` names it). `SchemaDec` is F's; where stream H's
    `ActionContext` wants `thesis_confidence` as its own unit-interval type, the conversion happens at
    the composition point, which parses `FirstOrderFacts::thesis_confidence.as_str()` through that
    type — this crate carries the text and rounds nothing (the review's item 5).
16. **No score is ever carried forward, by construction.** No type in this crate's API accepts a
    predecessor's score or scorecard, so MI-18's second half is unrepresentable rather than only
    tested; `FoldStep::score_carried_forward` returns `false`, and a property asserts it over every
    generated fold so a later scorecard reader cannot regress it. Scoring itself is E17-8's.
17. **The crate returns events; it appends nothing.** `ResearchEvent` values carry the fields journal
    spec §9 lists. Appending them, their canonical payloads, the stream each goes to (the thesis
    entries to the agent stream, `UniverseChanged` to the account stream), and the `causation_id` that
    ties the two together are the executor's and `mandate-journal`'s. No event schema is exported
    here.
18. **An unavailable working universe is an error, never an empty one.** §2.3 rebuilds the universe by
    folding `UniverseChanged`; until that fold is read, nothing may be admitted.
    `WorkingUniverse::Unavailable` returns `Err(universe_unavailable)`, exactly as stream G's gate
    treats it (DEC-129 interpretation 3), so "unknown" can never be reported as "empty".
19. **Pending markers name stories, not this stream.** `#[ignore = "pending E17-3"]`,
    `"pending E17-7"`, `"pending E17-2"`, and `"pending E17-9"`, which `cargo xtask markers` accepts
    and `cargo xtask ci pending` proves fail on the stubs (DEC-110).
20. **Check 5 is unreachable for a validated mandate, and stays.** V-036 requires
    `behavior.research` to be non-null exactly when a model admits, and V-037 forbids an admitting
    model when `universe.pinned` is true, so a pinned validated mandate always fails check 4
    (`research_disabled`) first — which is what MC-N08 expects, although its title names the pinned
    universe. Check 5 stays as the structural statement of MI-20, and
    `properties::no_pinned_mandate_ever_admits` asserts the invariant whichever check fires, so the
    ordering is defence in depth rather than dead code. Recorded so MC-N08 is not read as coverage of
    check 5 ("Decisions needed" item 3).
21. **Landing the fold is not the revision loop DEC-111 item 5 gates.** Item 5 says no revision loop
    ships before the forward-paper evaluator exists (E17-8) and one DEC-99 evaluation has run. This
    crate never *proposes* a revision: it folds revisions it is given, enforces the cap, and retires
    a lineage past it — and retirement is a removal, which is exits-only and adds no risk (MI-19). The
    cap and the retirement are what make an ungated loop impossible, so building them first is the
    conservative order, and every part of E17-9 that could originate a revision is under "Not done".
22. **Checks 4, 6, 7, 16, and 17 read the overlay's effective value, not the mandate's alone.** §4.3
    says the policy applies to a running agent at the next evaluation as an overlay, and stream F's
    brief names `max_instruments`, `research_cost_cap_usd_per_day`, `max_revisions_per_lineage`,
    `research_agent_allowed`, `admission_auto_allowed`, `research_interval_s`, and `stagger_window_s`
    as the keys this stream asks `PolicyOverlay::effective` for. So each check compares against the
    stricter of the mandate's value and the policy's: the lower bound for a maximum, the higher for a
    minimum, and `auto` evaluating as `ask` for `autonomy.admission`. Two consequences are additions
    to what the reference models, and both only ever **refuse more**, so no committed case moves: a
    policy with `research_agent_allowed: false` fails check 4 beside V-036's own condition, and a
    policy ceiling below the mandate's binds at checks 7, 16, and 17. Check 5 is not among them: it
    reads the envelope field **`universe.pinned`**, not `WorkingUniverse::Known { pinned }`, and it is
    MI-20 rather than a policy key. The two are kept in step by validation, and the crate reads the
    envelope, because a runtime flag is not what the owner confirmed.

23. **Three rules stay live, and are mutation-tested by hand.** `RefusalReason`'s ordinals, codes and
    `is_ignored_output`; `PolicyOverlay`'s six stricter-of rules and its two profile constructors; and
    `group_of` are real code in this tests PR, not stubs, because each states a rule exactly once
    (interpretations 4, 12 and 22) and a stub cannot hold a rule. DEC-83 asks in exchange that nothing
    live goes unverified, so `crates/mandate-research/tests/rules.rs` pins every branch of all three
    **unignored**, alongside every `as_str` and `code` accessor, every id constructor's rejection of the
    empty string, `AdmissionDecision`'s three readers over both variants and all seventeen reasons,
    `LineageState`'s five readers, and `FoldStep::score_carried_forward`. `cargo mutants -p
    mandate-research`, run by hand: **`79 mutants tested: 4 missed, 55 caught, 20 unviable`**, and the
    four are the four stub bodies (`checks`, `stagger_offset` twice, `next_proposal_at`), which only a
    pending test can reach. **Every line of live code is at 0 missed.**

    Three shapes changed to get there, and all three are improvements rather than concessions:
    `effective_cost_cap` uses `min` like the other maximums, because its `<` and `<=` return the same
    value at equality; neither profile constructor uses `..Self::default()`, because a field whose
    explicit value equals the default can be deleted invisibly; and `LineageState::empty` is gone in
    favour of `default()`, because a function whose body *is* `Self::default()` cannot be distinguished
    from the mutant that replaces it. Each was an equivalent mutant, which no test could ever have
    caught — the fix is to remove the equivalence, not to approve it.
24. **The swap PR asks for one DEC-77 exception, scoped to constructor and builder lines.** Verified
    first-hand against what stream F shipped in
    [#140](https://github.com/kunwarshivam/mandate/pull/140), not predicted:

    | This crate's narrow view | What F shipped | Consequence for the swap |
    |---|---|---|
    | `AssetId::new(&str) -> Result<_, SpecTypeError>` | `AssetId::parse(&str) -> Result<_, DomainError>` (`mandate-domain`) | a rename and a different error type at every fixture builder |
    | `GroupId::new(&str) -> Result<_, SpecTypeError>` | `GroupId::new(&str) -> Self` (`mandate-spec::validate`) | the builders drop an `.expect` |
    | `SchemaDec::from_checked_text(&str) -> Self` | `SchemaDec::parse(&str, DecGrammar) -> Result<_, GrammarMismatch>` | each fixture decimal names its grammar |
    | `ResearchEnvelope::cost_cap_usd_per_day: Usd` | `cost_cap_usd_per_day: SchemaDec` (`document.rs`) | check 7 converts through `to_usd` before comparing, so the comparison stays exact |
    | `ValidatedMandate::from_validated_envelope(MandateEnvelope)` | `ValidatedMandate::new(Mandate, &ValidationContext, &[PolicyLevel])` | every scenario builds a real document instead of an envelope struct |
    | `PolicyOverlay` with public fields and six infallible typed accessors | `PolicyOverlay { tightest: BTreeMap<PolicyKey, PolicyValue> }` — private — with a **fallible** `effective`, plus `auto_allowed` and `narrow(decision)` | the tests stop using struct literals, and the six accessors become fallible. `checks` already returns `Result`, so no signature in this brief changes; F's doc gives the reason the fallibility matters — an overlay that answered "no ceiling" from a stub would enforce nothing |
    | `AssetClass { UsEquity, Crypto }` | `AssetClass { Crypto, UsEquity }` | a different `Ord`, which this crate only membership-tests, so no output moves |

    So the PR that deletes `spec_types.rs` also edits `tests/common/mod.rs` and the construction lines
    of `admission.rs`, `properties.rs`, `refcases.rs` and `rules.rs`. DEC-77 otherwise lets an
    implementation PR change test files only by deleting pending markers, so that PR states the
    exception, keeps it to those lines, and changes no assertion — which a reviewer can check, because
    every assertion is a separate line from the builder that feeds it.

## The case-loading design

Family N's harness is a later PR, after stream F's `crates/mandate-refcases/src/mandate.rs` exists.
It follows the same pattern:

- four new interpretations in that module — `admission`, `lineage`, `thesis_expiry`, and `stagger` —
  each reading every key its cases state and failing loudly on a key it does not read (DEC-85), so a
  case cannot pass on a key nobody compared.
- One `Case` per id, named `mandate::MC-N01` to `mandate::MC-N28`, dispatched by `kind`.
- `admission` fills `InstrumentFacts` from the case's thesis fields (interpretation 6) and states
  that it does, and composes `mandate-builder`'s `classify` for the three cases that state
  `first_order_autonomy` (interpretation 3).
- `lineage` compares each step's `admitted`, `reason`, `score_carried_forward`,
  `lineage_revisions`, `lineage_retired`, `universe_size_after`, and `journal`, then the final
  `lineages`, `lineage_instruments`, and `working_universe`.
- `stagger` compares the offset list and the window.
- The status PR adds the family-N rows to the founder-owned `status.toml`.

Until that PR, family N fails with "not interpreted until E17-3", as stream F's harness arranges.

## Not done

- **E17-5, the input-drift detector** (`unusual_input`, V-018). Stream F leaves the condition field
  rejected at validation, so no autonomy rule can name it yet, and §8.4's "escalates unusual inputs
  before the agent acts on them" has no implementation in this crate. E17-5 ships it.
- **E17-6, the operator flow monitor.** This crate reads a halt set for check 9 and nothing more.
  Summing research-agent exposure per instrument across a deployment's workspaces, the DEC-123
  thresholds, the operator alert, and the `PlatformOperatorAction` that issues a halt are an operator
  service outside the trade path (DEC-100), and the `research_thesis_halt` action kind is a spec
  change of E17-6's own.
- **E17-8, the forward-paper evaluation harness**, and **E15-3's scorecards**. DEC-99 and DEC-122 fix
  the window, metric, and threshold; this crate computes no score, and interpretation 16 makes
  carrying one forward unrepresentable. What it does provide is the lineage's revision count, which
  MI-18 requires every report to show.
- **E17-9 beyond the fold.** Proposing a revision, writing the autopsy, and the `OwnerAlertSent` a
  retirement owes the owner (§8.6 item 4) are the loop DEC-111 item 5 gates on E17-8.
- **E17-2's model invocation.** The prompt, the retrieved context, the artifact writes,
  `ModelInvocationRecorded`, `ModelOutputRecorded`, the allowlist fetch, and the cost accounting that
  fills `research_spend_usd_today` are the shell's. The crate takes the typed output
  (interpretation 2).
- **Output freshness and the combine step.** §8.2's `fresh` (`as_of ≤ now < expires_at` and
  `now − as_of ≤ max_output_age_s`, latest per model) and §8.3's `conviction_linear` are stream H's.
  A thesis's *lifetime* (§8.6: admission to expiry) and an output's *freshness* are different clocks,
  as §8.6's last bullet says; this crate owns the first only.
- **The gate's re-check.** §5.3 check 2 denies any opening or increasing order outside the working
  universe, and §5.9 turns a removal into an instrument restriction. Both are stream G's, so the
  universe binds the order path independently of anything here.
- **The eligibility floor itself** (trading spec §3.2, E6-7, RC-16), **instrument-group claims**
  (§7.1), and the **conduct controls** the stagger sits inside (§9.6): stream G's. This crate reads
  their verdicts as facts and must never hold a second, laxer copy — planted bug 12.
- **Applying a mandate version** (§2.2's `version_applied` removals) and the runtime sequencing:
  stream I's and E7's.
- **`status.toml`.** Founder-owned; family N moves in a status PR per DEC-77.
- **No spec, schema, reference, or fixture edit.** They are the contract (ES-22). The two gaps found
  are under "Decisions needed".

## Decisions needed

Seven for the founder (items 1, 2, 3, 7's two halves, and 8's two halves) and three for the
coordinator (items 4, 5, 6).

1. **Two founder-owned entries** the tests PR adds and cannot take itself: `xtask/layers.toml` gains
   `[crates.mandate-research]` (layer 5, `safety_critical = true`, `pure = true`,
   `allowed_external = ["thiserror"]`) and CODEOWNERS gains a line for the crate directory.
   `cargo xtask layers` fails without them. The alternative placement is **layer 6 with a dependency
   on `mandate-builder`**, which would let `admit` return the first order's autonomy decision itself
   and reproduce the reference's output shape exactly, at the cost of inverting the data flow stream
   H's brief states and making admission depend on the order builder; interpretation 3 explains why
   this brief does not recommend it. A veto of layer 5 reopens interpretations 1 and 3.
   `Proposed (founder)`.
2. **§8.4's thesis table lists `corroboration` among the thesis's own fields.** Read as
   agent-asserted, check 15 is satisfied by a model writing `independent_source` into its own output,
   which is the planted-source attack DEC-101 exists to stop; read as *recorded* (DEC-101's own
   word), the corroboration is platform-derived and the model cannot manufacture it.
   **Recommendation:** clarify §8.4's row to say the corroboration is what the platform found and is
   recorded in the thesis record, and keep §8.5 check 15 as it stands. No reference case changes
   either way, and interpretation 7 takes the conservative reading meanwhile. `Proposed (founder)`.
3. **Check 5 (`universe_pinned`) is unreachable for a validated mandate** (V-036 with V-037), so
   MC-N08 exercises check 4 despite its title. Nothing needs to change: the check is defence in
   depth against an unvalidated input, and interpretation 20 tests MI-20 whichever check fires. This
   is recorded so the founder knows family N covers 16 of the 17 checks by a journaled reason, and
   the seventeenth by an invariant. **Recommendation:** leave the spec as it is; if the founder
   prefers coverage by reason, a new case with an unvalidated pinned mandate carrying an admitting
   model would do it, which is a fixture change only the founder can make. `Proposed (founder)`.
4. **One universe change reason across streams F and J — the variant gap is closed; only the name
   differs.** The brief raised this when both sides were briefs. Stream F has now shipped
   `mandate_spec::risk::RemovalReason`, and it carries **all seven** reasons of journal spec §9's
   `UniverseChanged` row, `ThesisAdmitted` included, so nothing is missing and this crate's
   `UniverseChangeReason` is that enum under another name. **Recommendation:** J's implementation PR
   takes F's `RemovalReason` as it stands and deletes its own copy with the rest of `spec_types.rs`; a
   rename to `UniverseChangeReason` would be tidier, since six of the seven are removals and one is
   not, but it is F's type and touches F's risk state, so it is the coordinator's call and not a
   blocker either way. A coordinator item.
5. **The family-N harness needs two crates.** The three cases that state `first_order_autonomy`
   (MC-N01, MC-N14, MC-N26) need `mandate-research` for the admission and `mandate-builder` for
   `classify`, so the harness PR lands after both streams' tests PRs and touches
   `mandate-refcases`, which claims #124, #125, and #132 all name. **Recommendation:** the
   coordinator sequences the harness PR after stream F's `mandate` module and stream H's tests PR,
   and says which claim carries it. A coordinator item.
6. **E17-3's acceptance clause spans four streams.** "Simulation fuzzing over random theses never
   admits an ineligible instrument or exceeds the envelope" is this stream's property tests;
   "prompt-injection fixtures never reach an order" needs the gate (G), the builder (H), and the
   runtime (I) as well, because "an order" exists only there. This brief delivers the admission half
   and `properties::text_never_changes_a_verdict`. **Recommendation:** the coordinator names the
   stream whose PR closes E17-3, and the end-to-end injection fixture becomes its own story once
   streams G, H, and I have landed. A coordinator item.
7. **Two things §8.5 does not cross-check, both raised by the independent review of the brief.**
   Neither is a defect in the seventeen checks; both are gaps in what the checks are given.
   - **A thesis citing no source reaches check 15 without check 14 ever binding.** `any([])` is false,
     so a source list that is empty passes check 14 vacuously, and check 15 carries the thesis alone.
     That is right for market-data corroboration, which cites no source, and the brief's property is
     therefore stated over theses with at least one source. What §8.5 does not do is cross-check the
     corroboration *kind* against the sources: a thesis with no sources whose kind is
     `independent_source` would be admitted. Interpretation 7 closes the model-driven path — the kind
     is what the platform found, so a model cannot assert it — but nothing stops a buggy corroboration
     service. **Recommendation:** E17-7 asserts the consistency where it derives the fact (an
     `independent_source` kind names the allowlisted source it came from), rather than adding an
     eighteenth check to §8.5. No case changes. `Proposed (founder)`.
   - **§8.5 never checks that a revision's predecessor failed.** A revision is admitted if the
     seventeen checks pass, and none of them reads a scorecard or asks whether
     `predecessor_thesis_id` names a thesis in the same lineage, so the research agent could revise a
     thesis that is doing fine, or name any predecessor at all. DEC-111 describes a revision as the
     answer to a failure, but §8.5 does not make that a condition, and the cap plus retirement is what
     bounds the loop. **Recommendation:** leave §8.5 as it is for the thin slice, where every
     admission is `ask` and a person sees the lineage's revision count (DEC-126), and let E17-8's
     evaluator supply the "did it fail" input that a later check could read. The lineage-membership
     half is cheaper: the fold could reject a `predecessor_thesis_id` that is not a thesis of that
     lineage as an input error, which would be an addition to the reference's behaviour, so the
     founder decides. `Proposed (founder)`.
8. **Two interpretations the review asked to raise here as well, because each is a judgement and not
   only a shape.**
   - **Interpretation 6 moves `asset_class` and the leveraged-ETP flag out of the thesis.** The
     reference's `admit` reads both from the thesis dictionary; this brief reads them from instrument
     reference data. No family-N case changes, because in every one the thesis's claim and the
     instrument's data agree, but the *source* of a check-10 and check-11 input differs from the
     reference. **Recommendation:** keep it — a thesis that classifies its own instrument decides two
     checks for itself, which MI-16 forbids — and let the founder note it as the one place where this
     crate reads an input the reference reads from elsewhere. `Proposed (founder)`.
   - **Interpretation 21 lands the lineage fold ahead of E17-8.** DEC-111 item 5 says no revision loop
     ships before the forward-paper evaluator exists and one DEC-99 evaluation has run. The reading
     here is that the *fold* is not the loop: nothing in this crate proposes a revision, and the cap
     and its retirement are what make an ungated loop impossible. **Recommendation:** keep it, since
     family N cannot pass without the fold and retirement only reduces risk; if the founder reads
     item 5 as covering the fold too, family N's six lineage cases wait for E17-8 and this stream
     ships admission, expiry, and the stagger alone. `Proposed (founder)`.

## Commands

```bash
cargo xtask check
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo nextest run -p mandate-research
cargo test -p mandate-refcases -- --include-ignored mandate::MC-N
cargo xtask ci pending
cargo mutants -p mandate-research
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong (the two found so far are under "Decisions needed");
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- a reference case seems wrong (propose the fix to the founder; never edit the fixture);
- anything would deviate from an accepted decision;
- stream F's or stream H's merged brief changes a type this brief reads.

## Definition of done

- [ ] The 28 cases of family N pass, and no case that passed before now fails.
- [ ] Tests came first; each invariant above has a property test whose oracle is independent and was
      shown to fail on a planted bug, and the planted-bug table below is reproduced in the tests PR
      body with the test that caught each one.
- [ ] New state changes emit journal events: `ThesisProposed`, `ThesisRevised`, and
      `UniverseChanged` cover every change this crate decides; appending them is the executor's.
- [ ] Docs updated: the feature map gains a "Research-agent contract, admission, and lineages"
      entry; the tracker's Stories, Claims, and work-graph rows.
- [ ] `cargo xtask check` is green, including `ci pending` (every pending test fails on the stubs)
      and zero missed mutants on the implementation PR's diff.
- [ ] Each PR description is complete (see the PR template).

## Planted bugs

Each was broken in a throwaway implementation of the stubs, one at a time, kept out of the PR
(DEC-83). **All 22 were caught.** Writing that implementation also found one defect in the tests' own
first reading: the check-set oracle capped revisions at the mandate's `max_revisions_per_lineage` even
when `behavior.research` was null, where `ref.py` caps at 0 (`cap = res[...] if res is not None else
0`), so a revision-1 thesis on a mandate with no research envelope is already over the cap. The
oracle disagreed with the crate, the reference settled it, and the oracle was wrong — which is the
whole reason an independent oracle exists.

| # | Bug | Caught by (every test that failed, from the run reported in the tests PR) |
|---|---|---|
| 1 | Check 15 never fails, so a thesis with no corroboration is admitted | 3: `check_15_a_thesis_without_corroboration_is_refused`, `the_lower_numbered_check_decides_when_several_fail`, `the_reason_is_the_lowest_numbered_failing_check` |
| 2 | Check 16 compares `revision < cap` instead of `revision > cap`, so a revision past the cap is admitted | 24: `a_lineage_never_admits_past_its_cap`, `a_lineage_retires_and_removes_only_once`, `a_lowered_ceiling_refuses_and_never_removes`, `a_renewal_skips_only_the_full_check`, `check_13_an_ungrouped_instrument_is_its_own_group_and_a_group_id_spelling_an_asset_id_claims_nothing`, `check_15_market_data_corroboration_admits_a_thesis_citing_no_sources`, `check_16_a_policy_ceiling_lowers_the_revision_cap`, `check_16_a_revision_past_the_cap_is_refused`, `check_7_a_cent_below_the_cap_still_admits`, `mc_n01_a_corroborated_thesis_in_an_allowed_asset_class_is_admitted`, `mc_n01_admission_journals_the_thesis_entry_and_the_universe_change`, `mc_n02_a_full_universe_refuses_rather_than_displacing`, `mc_n14_renewing_an_active_instrument_adds_no_second_entry`, `mc_n17_revisions_one_to_three_are_admitted_and_the_fourth_retires_the_lineage`, `mc_n17_the_retiring_step_journals_the_removal_with_the_size_after`, `mc_n18_a_revision_never_carries_its_predecessor_score_forward`, `mc_n24_retirement_removes_the_instrument_it_holds`, `mc_n26_a_leveraged_etp_with_the_opt_in_and_the_exact_disclosure_is_admitted`, `mc_n27_an_over_cap_revision_an_earlier_check_refuses_retires_nothing`, `mc_n28_retirement_never_removes_an_instrument_another_lineage_now_holds`, `the_horizon_pairs_as_of_with_expires_at_exactly`, `the_internal_research_profile_makes_every_admission_ask_without_refusing_any`, `the_lower_numbered_check_decides_when_several_fail`, `the_reason_is_the_lowest_numbered_failing_check` |
| 3 | `expire_theses` keeps an entry it also removed, so a thesis that ended stays in the universe | 5: `mc_n20_a_thesis_at_its_horizon_removes_its_instrument`, `mc_n21_an_invalidated_thesis_removes_at_once_before_its_horizon`, `mc_n22_a_retired_lineage_removes_its_instrument_and_an_unexpired_thesis_stays`, `several_removals_count_the_universe_down`, `the_horizon_removes_at_exactly_the_horizon_and_not_before` |
| 4 | The equity stagger is counted from the admission rather than the later of the admission and the next regular open | 1: `an_equity_release_waits_for_the_later_of_the_two_instants` |
| 5 | Retirement sets `retired` but does not emit the removal, so the instrument stays with no path back | 4: `a_lineage_retires_and_removes_only_once`, `mc_n17_revisions_one_to_three_are_admitted_and_the_fourth_retires_the_lineage`, `mc_n17_the_retiring_step_journals_the_removal_with_the_size_after`, `mc_n24_retirement_removes_the_instrument_it_holds` |
| 6 | Check 17 never fails, so a full universe admits and a lowered ceiling is ignored | 3: `a_lowered_ceiling_refuses_and_never_removes`, `mc_n02_a_full_universe_refuses_rather_than_displacing`, `the_reason_is_the_lowest_numbered_failing_check` |
| 7 | Checks 10 and 12 are evaluated out of order, so a refusal journals the wrong reason | 2: `checks_reports_all_seventeen_in_the_spec_order`, `the_lower_numbered_check_decides_when_several_fail` |
| 8 | `cost_cap_reached` compares `spend > cap` instead of `>=` | 2: `check_7_the_cost_cap_binds_at_equality`, `the_reason_is_the_lowest_numbered_failing_check` |
| 9 | A renewal is treated as a first admission, so it writes a second `UniverseChanged` and a duplicate entry | 4: `mc_n14_renewing_an_active_instrument_adds_no_second_entry`, `mc_n17_revisions_one_to_three_are_admitted_and_the_fourth_retires_the_lineage`, `mc_n28_retirement_never_removes_an_instrument_another_lineage_now_holds`, `the_universe_equals_the_fold_of_the_emitted_events` |
| 10 | `universe_full` is applied to a renewal too, so renewing at the ceiling is refused | 2: `a_renewal_skips_only_the_full_check`, `the_reason_is_the_lowest_numbered_failing_check` |
| 11 | Check 11 accepts any accepted disclosure rather than exactly `leveraged_etp_disclosure_version` | 2: `mc_n25_a_leveraged_etp_with_a_different_accepted_disclosure_version_is_refused`, `the_reason_is_the_lowest_numbered_failing_check` |
| 12 | Check 12 never fails, so the crate ignores the eligibility floor's verdict | 3: `check_12_a_thesis_failing_the_eligibility_floor_is_refused`, `the_lower_numbered_check_decides_when_several_fail`, `the_reason_is_the_lowest_numbered_failing_check` |
| 13 | Check 10 never fails, so an asset class outside the envelope is admitted | 4: `check_10_an_asset_class_outside_the_envelope_is_refused`, `check_10_reads_reference_data_and_a_thesis_has_no_asset_class_to_claim`, `the_lower_numbered_check_decides_when_several_fail`, `the_reason_is_the_lowest_numbered_failing_check` |
| 14 | Any refusal of a revision retires the lineage, whatever the journaled reason | 3: `mc_n19_a_revision_without_a_predecessor_id_is_ignored_and_admits_nothing`, `mc_n27_an_over_cap_revision_an_earlier_check_refuses_retires_nothing`, `retirement_happens_exactly_on_a_lineage_retired_refusal` |
| 15 | Retirement removes the instrument the refused thesis **named** rather than the one the lineage **holds** | 1: `mc_n28_retirement_never_removes_an_instrument_another_lineage_now_holds` |
| 16 | Expiry compares `now > expires_at`, so a thesis survives its own horizon by an instant | 2: `mc_n20_a_thesis_at_its_horizon_removes_its_instrument`, `several_removals_count_the_universe_down` |
| 17 | The reduction folds the digest least-significant-first | 2: `mc_n23_the_three_fixture_offsets`, `the_digest_is_the_workspace_a_zero_byte_and_the_thesis` |
| 18 | Check 3 tests only that a revision has a predecessor, not that revision 0 has none | 2: `check_3_a_revision_zero_carrying_a_predecessor_is_ignored`, `the_reason_is_the_lowest_numbered_failing_check` |
| 19 | `is_ignored_output` covers `direction_not_allowed` alone | 3: `check_2_an_expiry_that_disagrees_with_the_horizon_is_ignored`, `ignored_is_exactly_the_first_three_checks`, `mc_n19_a_revision_without_a_predecessor_id_is_ignored_and_admits_nothing` |
| 20 | `universe_size_after` is counted before the change rather than after | 2: `mc_n01_admission_journals_the_thesis_entry_and_the_universe_change`, `the_universe_equals_the_fold_of_the_emitted_events` |
| 21 | `WorkingUniverse::Unavailable` is treated as an empty universe | 1: `an_unavailable_universe_is_an_error` |
| 22 | Groups are compared as strings, so a group id that spells an asset id claims that instrument | 1: `check_13_an_ungrouped_instrument_is_its_own_group_and_a_group_id_spelling_an_asset_id_claims_nothing` |
