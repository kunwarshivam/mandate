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
  lineage cap), DEC-117 to DEC-126 (`Proposed (founder)`; the items this brief depends on are listed
  below), DEC-128 (stream F's types), DEC-129 (stream G's gate), DEC-130 (stream H's builder and
  autonomy), DEC-131 (stream I's runtime), and DEC-132 (this brief's interpretations).
- **Proposed decisions this brief depends on.** A founder veto of any of these reopens this brief:
  **DEC-117** (`max_instruments`, the platform ceiling of 20 and the default of 5; check 17),
  **DEC-118** (thesis lifetime: `expires_at = as_of + horizon_s`, no automatic renewal, expiry makes
  the instrument removed; checks 2 and `thesis_expired`), **DEC-120** (the cost cap as an envelope
  field enforced by deterministic code; check 7), **DEC-121** (the pinning switch, which is stream
  F's classification and reaches this crate only as `universe.pinned`; check 5),
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
oracles"; ES-11). Test paths are `crates/mandate-research/tests/`.

| Clause or invariant | Test |
|---|---|
| §8.5 the checks are evaluated **in order** and the first failure is the journaled reason | `admission::one_test_per_check` (17 named cases, each failing exactly one check with every later check also failing where the spec allows it), `properties::the_reason_is_the_lowest_numbered_failing_check` |
| §8.2, §8.5 checks 1 to 3 are **ignored outputs** as well as refusals | `admission::the_three_ignored_outputs_report_ignored`, `properties::ignored_is_exactly_the_first_three_checks`, MC-N12, MC-N13, MC-N19 |
| §8.5 a refusal admits nothing and leaves the universe as it was, with the one `lineage_retired` exception | `properties::a_refusal_never_grows_the_universe`, `properties::only_a_retirement_lets_a_refusal_change_the_universe`, MC-N02 to MC-N13, MC-N15, MC-N16, MC-N25 |
| MI-15 the universe never exceeds the effective `max_instruments`, holds no duplicate, and every member passed every check when admitted | `properties::the_universe_never_exceeds_its_ceiling_or_repeats`, `properties::every_member_was_admitted_by_a_passing_check_set`, MC-N02 |
| MI-15, §8.5 a **full universe never displaces** an active instrument; `universe_full` is the only check a renewal skips | `admission::a_full_universe_refuses_rather_than_displacing`, `admission::a_renewal_skips_only_the_full_check`, `properties::a_renewal_and_a_first_admission_differ_only_in_check_17`, MC-N02, MC-N14 |
| MI-16 admission never changes an envelope field | `properties::admission_changes_no_envelope_field` (the mandate's canonical bytes, its version, and every effective overlay value are identical before and after every admission and every fold) |
| MI-17 the first order in a newly admitted instrument is at least as strict as `autonomy.admission` | `admission::an_admitted_thesis_reports_the_effective_admission_ceiling`, `properties::first_order_facts_always_carry_new_instrument_and_the_ceiling` (the strictness comparison itself is stream H's, interpretation 3) |
| MI-18 a lineage never admits a revision past `max_revisions_per_lineage`, and no revision carries a predecessor's score | `lineage::revisions_up_to_the_cap_are_admitted_and_the_next_retires`, `properties::a_lineage_never_admits_past_its_cap`, `properties::no_fold_step_ever_carries_a_score_forward`, MC-N17, MC-N18, MC-N24 |
| §8.6 item 4 retirement follows the **journaled refusal reason**, not the revision number | `lineage::an_earlier_check_refusing_an_over_cap_revision_retires_nothing`, `properties::retirement_happens_exactly_on_a_lineage_retired_refusal`, MC-N27 |
| §8.6 item 4 retirement removes the instrument the lineage holds, in the same fold step, and never one another lineage now holds | `lineage::retirement_removes_the_instrument_it_holds`, `lineage::retirement_leaves_what_another_lineage_took_over`, `properties::retirement_removes_at_most_its_own_holder`, MC-N24, MC-N28 |
| MI-19 exactly the theses that expired, were invalidated, or whose lineage retired remove their instrument | `expiry::one_test_per_removal_reason`, `properties::exactly_the_ended_theses_remove_their_instrument`, MC-N20, MC-N21, MC-N22 |
| §8.6 expiry is inclusive at the horizon, and the reason order is invalidated, retired, expired | `expiry::the_horizon_removes_at_exactly_the_horizon`, `expiry::invalidation_outranks_an_unreached_horizon`, `properties::the_removal_reason_is_the_first_that_holds`, MC-N20, MC-N22 |
| MI-19 removal restricts that instrument only | `expiry::a_removal_restricts_only_its_own_instrument`, `properties::a_removal_touches_no_other_instruments_restriction` |
| MI-20 a pinned universe admits nothing | `admission::a_pinned_universe_admits_nothing`, `properties::no_pinned_mandate_ever_admits` (whichever of checks 4 and 5 fires), MC-N08 |
| §8.4, DEC-100, DEC-123 the stagger offset is deterministic, depends on both ids, and is inside the window | `stagger::the_three_fixture_offsets`, `properties::an_offset_is_below_its_window`, `properties::both_ids_change_the_offset`, `num::the_reduction_matches_a_least_significant_first_oracle`, MC-N23 |
| §8.4 the offset is counted from the admission for crypto and from the **later** of the admission and the next regular open for equities; a window of 0 means no wait | `stagger::an_equity_release_waits_for_the_later_of_the_two_instants`, `stagger::a_crypto_release_is_counted_from_the_admission`, `stagger::a_zero_window_releases_at_once`, `properties::a_release_is_never_before_its_anchor` |
| §8.4, DEC-120 the cost cap binds at equality and refuses new theses only | `admission::the_cost_cap_binds_at_equality`, `properties::the_cost_cap_never_touches_an_existing_entry`, MC-N09 |
| DEC-101 every cited source must be on the allowlist, and corroboration is required | `admission::one_source_off_the_allowlist_refuses_the_whole_thesis`, `admission::a_thesis_without_corroboration_is_refused`, `properties::an_empty_allowlist_admits_nothing`, MC-N06, MC-N07 |
| DEC-101, R-05 no string in a thesis can change a verdict (the prompt-injection property) | `properties::text_never_changes_a_verdict` (every string field replaced by adversarial text, including text naming other checks, over generated inputs) |
| DEC-103 the thin slice admits only instruments in the pinned data universe | `admission::the_data_universe_refuses_anything_outside_it`, `properties::a_pinned_data_universe_is_a_superset_of_every_admission`, MC-N11 |
| §8.5 checks 10 and 11 read instrument reference data, never the thesis's own claim | `admission::a_self_declared_asset_class_cannot_pass_check_10`, `admission::a_self_declared_etp_flag_cannot_pass_check_11` (interpretation 6) |
| trading spec §7.1 the group claim covers an ungrouped instrument as its own group | `admission::an_ungrouped_instrument_is_its_own_group`, `admission::a_group_claimed_through_a_sibling_refuses`, MC-N05 |
| §2.3, §5.3 an unavailable working universe is an error, never an empty set | `admission::an_unavailable_universe_is_an_error` |
| MI-8, ES-21 the same inputs give identical outputs; no clock, no randomness, `BTreeMap` order only | `properties::identical_inputs_give_identical_admissions_and_events`, `properties::no_output_depends_on_iteration_order` |
| §8.5, §8.6 every state change emits its journal entry, and nothing changes without one | `properties::the_universe_equals_the_fold_of_the_emitted_events` (oracle 1), `properties::every_step_emits_exactly_one_thesis_entry` |
| journal spec §9 the entry type follows the revision number, and `predecessor_thesis_id` is present exactly when `revision > 0` | `admission::the_entry_type_follows_the_revision_number`, `properties::a_predecessor_appears_exactly_on_a_revision`, MC-N19 |
| The harness reads every key every family-N case states (DEC-85) | `harness::every_family_n_case_key_is_read`, `harness::a_wrong_expected_value_fails_the_case` (in the later harness PR) |

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
3. **The lineage counter.** A separate accumulator walks the thesis sequence and tracks, per
   lineage, the highest admitted revision, the number of admissions, and whether an otherwise-passing
   thesis exceeded the cap; from those it derives retirement and the removal set without reading the
   fold's own state. A test seeds "retire on any refusal of an over-cap revision" and watches the
   oracle catch it on MC-N27's shape.
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
/// `InstrumentFacts` and every fact about the evidence is in `CorroborationFacts`.
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
/// Instrument reference data (trading spec §3.2, §7.1). Never the thesis's own claim
/// (interpretation 6).
pub struct InstrumentFacts {
    pub instrument: AssetId,
    pub asset_class: AssetClass,
    pub leveraged_or_inverse_etp: bool,
    pub group: InstrumentGroup,
}

/// An instrument with no named group is its own group, made explicit so a group id that spells an
/// asset id cannot claim it (interpretation 12).
pub enum InstrumentGroup { Named(GroupId), Ungrouped(AssetId) }

/// DEC-101, E17-7. The allowlist and its version are versioned configuration the caller supplies;
/// `kind` is the corroboration the platform found, or `None` when it found none.
pub struct CorroborationFacts {
    pub allowlist_version: AllowlistVersion,
    pub allowlisted_sources: BTreeSet<SourceId>,
    pub kind: Option<Corroboration>,
}

/// Facts about the account and the operator that checks 9, 12, 13, and 11 read. Each is produced
/// elsewhere: the eligibility failures by `mandate-risk`'s floor (E6-7), the claims by the account
/// ledger (trading spec §7.1), the halts by the operator service (E17-6), the disclosures by the
/// consent record.
pub struct AdmissionFacts {
    pub eligibility_failures: BTreeSet<AssetId>,
    pub claimed_by_other_agents: BTreeSet<InstrumentGroup>,
    pub halted_instruments: BTreeSet<AssetId>,
    pub disclosures_accepted: BTreeSet<Digest>,
    /// DEC-103: `Some` pins a data universe (the research basket); `None` pins none.
    pub data_universe: Option<BTreeSet<AssetId>>,
    /// DEC-120: the risk day's research spend so far, whose day boundary is `mandate_spec::risk_day`.
    pub research_spend_usd_today: Usd,
}

pub struct AdmissionInput<'a> {
    pub mandate: &'a ValidatedMandate,
    pub overlay: &'a PolicyOverlay,
    pub universe: &'a WorkingUniverse,
    pub thesis: &'a Thesis,
    pub instrument: &'a InstrumentFacts,
    pub corroboration: &'a CorroborationFacts,
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
    pub theses: &'a [Thesis],
    pub instruments: &'a BTreeMap<AssetId, InstrumentFacts>,
    pub corroboration: &'a CorroborationFacts,
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
`AssetClass`, and `WorkingUniverse` come from `mandate-domain` or `mandate-spec` (interpretation 15);
`ModelId`, `ModelVersion`, `ContentHash`, `GroupId`, `SchemaDec`, and `Digest` come from
`mandate-spec` and `mandate-canon`. This crate defines no second copy.

### Errors

`ResearchError` is a `thiserror` enum, each variant with a stable `code()` (ES-09):
`universe_unavailable` (the `UniverseChanged` fold has not been read, so nothing may be admitted —
never treated as an empty universe, interpretation 18), `duplicate_instrument` (an input universe
holding a repeat, which MI-15 forbids), `duplicate_thesis_id` (a fold input naming one thesis twice),
`instrument_facts_missing` (a fold step whose instrument has no reference data),
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
7. **Corroboration is platform-derived, not agent-asserted.** §8.4's table lists `corroboration`
   among the thesis's fields and DEC-101 says it is *recorded* in `ThesisProposed`. Read as an
   agent-asserted field, check 15 is satisfied by a model writing `independent_source` into its own
   output, which is exactly the planted-source attack DEC-101 exists to stop (R-05). So
   `CorroborationFacts::kind` is what the platform found — an allowlisted source independent of the
   primary one, or market data consistent with the thesis — and check 15 requires it to be `Some`.
   Every case agrees: the reference checks only that a kind is present, and the harness fills the
   fact from the case's `corroboration.kind`. What the platform must do to *earn* that fact is E17-7's
   data-plane work, named under "Not done", and "Decisions needed" item 2 asks for the one-word spec
   clarification.
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
15. **Shared types have one home.** `UniverseChange`, the universe change reason,
    `InstrumentRestriction`, `AutonomyDecision`, `AssetId`, `AssetClass`, and `WorkingUniverse` are
    taken from `mandate-domain` or `mandate-spec`, never redefined here (DEC-128 item 21's
    mechanism). Stream F's brief names the reason enum `RemovalReason`, which cannot carry
    `thesis_admitted`; one `UniverseChangeReason` covering the seven reasons of journal spec §9's
    `UniverseChanged` row is what both streams need, and the tests PR takes whichever name is on
    `main` ("Decisions needed" item 4).
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
4. **One universe change reason across streams F and J.** Stream F's merged brief names the risk
   state's reason enum `RemovalReason`, which cannot carry `thesis_admitted`; journal spec §9's
   `UniverseChanged` row lists seven reasons including it. **Recommendation:** the coordinator relays
   a one-line note to stream F's tests PR to name the shared enum `UniverseChangeReason` with all
   seven variants in `mandate-domain`; this stream's tests PR then takes whichever name is on `main`
   and its implementation PR keeps one definition (interpretation 15). A coordinator item, not a
   founder one.
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
- the founder vetoes DEC-117, DEC-118, DEC-120, DEC-121, DEC-123, or DEC-126, which reopens this
  brief;
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

Each is broken in a throwaway implementation of the stubs, one at a time, kept out of the PR
(DEC-83), and each must be caught. The tests PR body reports the result for every row; a row that
nothing catches means the test set is incomplete, not that the bug is harmless.

| # | Bug | Expected to fail |
|---|---|---|
| 1 | Check 15 never fails, so a thesis with no corroboration is admitted | `admission::a_thesis_without_corroboration_is_refused`, `properties::the_reason_is_the_lowest_numbered_failing_check`, MC-N06 |
| 2 | Check 16 compares `revision < cap` instead of `revision > cap`, so a revision past the cap is admitted | `lineage::revisions_up_to_the_cap_are_admitted_and_the_next_retires`, `properties::a_lineage_never_admits_past_its_cap` (MI-18), MC-N17, MC-N24 |
| 3 | `expire_theses` emits the removal for the entry at the position of the *kept* instrument, so a live instrument is removed and the expired one stays | `expiry::one_test_per_removal_reason`, `properties::exactly_the_ended_theses_remove_their_instrument` (MI-19), MC-N22 |
| 4 | The equity stagger is counted from the admission rather than the later of the admission and the next regular open | `stagger::an_equity_release_waits_for_the_later_of_the_two_instants`, `properties::a_release_is_never_before_its_anchor` |
| 5 | Retirement sets `retired` but does not emit the `UniverseChanged` removal, so the instrument stays in the universe with no path back | `lineage::retirement_removes_the_instrument_it_holds`, `properties::the_universe_equals_the_fold_of_the_emitted_events` (oracle 1), MC-N17, MC-N24 |
| 6 | A `max_instruments` below the universe's size removes instruments to fit instead of only refusing further admissions | `properties::a_refusal_never_grows_the_universe`, `properties::only_a_retirement_lets_a_refusal_change_the_universe`, `properties::exactly_the_ended_theses_remove_their_instrument` (MI-19) |
| 7 | The checks run out of order (eligibility before the asset class), so a refusal journals the wrong reason | `admission::one_test_per_check`, `properties::the_reason_is_the_lowest_numbered_failing_check` (oracle 2), MC-N03, MC-N04 |
| 8 | `cost_cap_reached` compares `spend > cap` instead of `>=` | `admission::the_cost_cap_binds_at_equality`, MC-N09 |
| 9 | A renewal writes a second `UniverseChanged` and a duplicate universe entry | `admission::a_renewal_skips_only_the_full_check`, `properties::the_universe_never_exceeds_its_ceiling_or_repeats` (MI-15), MC-N14, MC-N17 |
| 10 | `universe_full` is applied to a renewal too, so renewing at the ceiling is refused | `admission::a_renewal_skips_only_the_full_check`, `properties::a_renewal_and_a_first_admission_differ_only_in_check_17`, MC-N14 |
| 11 | Check 11 accepts any accepted disclosure rather than exactly `leveraged_etp_disclosure_version` | `admission::one_test_per_check`, MC-N25, MC-N26 |
| 12 | The crate holds its own eligibility test (a price floor only) instead of reading `eligibility_failures` | `admission::one_test_per_check`, `properties::every_member_was_admitted_by_a_passing_check_set`, MC-N04 |
| 13 | Checks 10 and 11 read the thesis's claimed asset class and ETP flag instead of instrument reference data | `admission::a_self_declared_asset_class_cannot_pass_check_10`, `admission::a_self_declared_etp_flag_cannot_pass_check_11`, `properties::text_never_changes_a_verdict` |
| 14 | Any refusal of an over-cap revision retires the lineage, whatever the journaled reason | `lineage::an_earlier_check_refusing_an_over_cap_revision_retires_nothing`, `properties::retirement_happens_exactly_on_a_lineage_retired_refusal` (oracle 3), MC-N27 |
| 15 | Retirement removes the instrument the lineage *named* rather than the one it *holds*, so it takes what another lineage took over | `lineage::retirement_leaves_what_another_lineage_took_over`, `properties::retirement_removes_at_most_its_own_holder`, MC-N28 |
| 16 | Expiry compares `now > expires_at`, so a thesis survives its own horizon by an instant | `expiry::the_horizon_removes_at_exactly_the_horizon`, MC-N20 |
| 17 | The reduction folds the digest least-significant-first, or reads only its first eight bytes | `stagger::the_three_fixture_offsets`, `num::the_reduction_matches_a_least_significant_first_oracle`, MC-N23 |
| 18 | Check 3 tests only that a revision has a predecessor, not that revision 0 has none | `admission::one_test_per_check`, `properties::a_predecessor_appears_exactly_on_a_revision`, MC-N19 |
| 19 | `ignored` is reported for `direction_not_allowed` alone | `admission::the_three_ignored_outputs_report_ignored`, `properties::ignored_is_exactly_the_first_three_checks`, MC-N13, MC-N19 |
| 20 | `universe_size_after` is counted before the change rather than after, and the expiry countdown runs upward | `properties::the_universe_equals_the_fold_of_the_emitted_events` (oracle 1), MC-N17, MC-N22 |
| 21 | `WorkingUniverse::Unavailable` is treated as an empty universe, so the first admission succeeds before the fold has been read | `admission::an_unavailable_universe_is_an_error` |
| 22 | A group id that spells an asset id claims that instrument, because groups are compared as strings | `admission::an_ungrouped_instrument_is_its_own_group` |
