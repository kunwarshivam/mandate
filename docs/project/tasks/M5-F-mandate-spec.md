# Task: stream F, `mandate-spec` and `mandate-domain` (the mandate document as code)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). This stream opens
milestone M5. It turns the mandate document, its validation, the policy hierarchy, change
classification, the risk state, risk days, and goals into Rust, and gives
[the 298 mandate reference cases](../../specs/reference-cases/mandate.yaml) their first Rust
harness. Streams G (`mandate-risk`), H (`mandate-builder`), and J (E17 thin slice) consume its
types, so the tests PR lands the whole public API as stubs before any of them starts.

The file is named after the stream rather than one story because one crate carries the families of
four stories (below) and the spec sections they share; one PR still implements one story's worth of
families at a time.

## Story

- **Stories:** [E10-1](../06-backlog-v1.md#e10-mandate-authoring), [E10-3](../06-backlog-v1.md#e10-mandate-authoring),
  [E6-4](../06-backlog-v1.md#e6-agent-runtime-and-risk), [E17-1](../06-backlog-v1.md#e17-research-agent-and-dynamic-universe).
- **Acceptance criteria (verbatim):**
  - E10-1: "As an operator, I want to describe an agent in plain language and get a compiled mandate
    with inferred fields highlighted. *Accepted when:* compiled mandates validate against the
    mandate spec (schema, V-rules, policy hierarchy; reference cases MC-S, MC-V, and MC-P pass);
    proposed envelope values are marked as proposed and no envelope field activates unconfirmed
    (DEC-97)."
  - E10-3: "As an operator, I want mandates versioned with viewable diffs, and changes that increase
    risk to require step-up. *Accepted when:* the version vector and MC-C01 to MC-C35 pass."
  - E6-4: "As an owner, I want a daily-loss limit and a drawdown ladder so that losses trigger
    automatic de-risking. *Accepted when:* MC-R01 to MC-R15, MC-T01 to MC-T05, and MC-L01 to MC-L09
    pass."
  - E17-1: "As an owner, I want the mandate split into envelope fields I confirm and a working
    universe the platform produces at runtime, so that I set the risk and the agent brings the
    ideas. *Accepted when:* the rewritten mandate spec's cases pass; the compiler may propose
    envelope values, each marked as proposed; no envelope field activates unconfirmed."

  This stream owns the *rule engine* behind those clauses, not the authoring surfaces: E10-1's
  compiler and E10-2's form, E10-3's diff view, and the step-up flow are later stories that call
  these functions. The three stale case ranges in the clauses (MC-C01 to MC-C35, MC-R01 to MC-R15,
  MC-L01 to MC-L09) predate the v0.6 rewrite; see "Decisions needed".
- **PRD / HLD / spec anchors:** PRD 6.3 FR-3.1 to FR-3.8, 6.5 FR-5.2 to FR-5.5, 6.6 FR-6.1 to
  FR-6.6; [mandate spec](../../specs/mandate.md) §1 and §1.1 (principles, MI-1 to MI-20), §2
  (lifecycle, provenance, applying a version, the working universe), §3 and §3.1 and §3.2
  (structure, goals, the field split), §4 (validation: §4.1 V-rules, §4.2 warnings and the
  confirmation screen, §4.3 the policy hierarchy), §5 (risk state: §5.1 capital and allocation
  changes, §5.2 inputs and the risk clock, §5.3 the limits the gate reads, §5.4 daily loss and the
  risk day, §5.5 drawdown and the ladder, §5.6 breach confirmation, §5.7 the lifetime floor, §5.8
  acknowledgment and the high-water-mark reset, §5.9 restrictions and the effective mode, §5.10 the
  journal events), §6.3 (the condition language), §7 (platform defaults and proposals), §9
  (versioning and change classification), §10 (records), §11 (the reference cases);
  [trading domain spec](../../specs/trading-domain.md) §8.1 (cost-basis reduction), §8.2 (risk
  marks); [journal spec](../../specs/journal.md) §4 (canonical form and the version hash), §9 (the
  event catalogue); [HLD](../../HLD.md) "Agent lifecycle"; ADR-0001 ES-02 (crates and layers), ES-04
  (decimals), ES-09 (error codes), ES-11 (tests), ES-21 (determinism), ES-22 (spec anti-drift).
- **Decisions that apply:** DEC-03, DEC-04, DEC-05 (the mandate binds; models produce opinions;
  reducing risk needs no approval), DEC-39 to DEC-70 (the risk state, autonomy, and classification
  rules the spec records), DEC-43 (classification), DEC-51 and DEC-98 (the policy hierarchy and the
  retail profile), DEC-63 (the two-quote hard trigger), DEC-72 (ADR-0001), DEC-77 (tests PR,
  implementation PR, status PR), DEC-79, DEC-80 (no plain comments), DEC-83 (tests PRs hold stubs
  only), DEC-85 (harness interpretations fail loudly until owned), DEC-89 and DEC-116 item 3 (exact
  arithmetic, no hidden quotients), DEC-97 to DEC-103 (the direction change: the envelope and
  strategy split, the thin slice), DEC-110 (every pending test must fail on the stubs), DEC-111
  (revision lineages), DEC-117 to DEC-126 (the rewrite answers, `Proposed (founder)`: a veto reopens
  this brief), DEC-127 item 14 (`mandate-num`'s `Ratio`, which this stream builds on), and DEC-128
  (this brief's interpretations).

## Scope

### Reference cases

`fixtures/refcases/mandate.json` (the machine form of `docs/specs/reference-cases/mandate.yaml`,
`version: 4`, 298 cases). The suite has no Rust harness today, so `crates/mandate-refcases/status.toml`
has no `[mandate]` table and every case is pending by default. This stream owns 202 of them:

| Family | Kind | Cases | Count | Story |
|---|---|---|---|---|
| S | `schema` | MC-S01 to MC-S31 | 31 | E10-1 |
| V | `semantic` | MC-V01 to MC-V67 | 67 | E10-1, E17-1 |
| P | `policy` | MC-P01 to MC-P22 | 22 | E10-1 |
| C | `change` | MC-C01 to MC-C48 | 48 | E10-3 |
| R | `risk_state` | MC-R01 to MC-R24 | 24 | E6-4 |
| T | `risk_day` | MC-T01 to MC-T05 | 5 | E6-4 |
| L | `goal` | MC-L01 to MC-L05 | 5 | E6-4 |

Plus `mandate::version` (the fixture's `version: 4`) and `mandate::version_vector` (the canonical
bytes and the document digest of the `btc_accumulator` base, spec §9.1), which belong to E10-3.

**Spec §11 names the families the other way round from the stream-F row of the work graph:** family
**F** is *agent flatten* (MC-F01 to MC-F04, kind `agent_flatten`) and family **L** is *the goal
family* (MC-L01 to MC-L05, kind `goal`). The goal family is in this stream. Family F is not: stream
G's claim [#123](https://github.com/kunwarshivam/mandate/issues/123) already holds MC-F01 to MC-F04
under E6-3 and the `agent_flatten` harness interpretation, and the plan is an order-path artifact
that reads no mandate field (see "Not done").

No case moves to `passing` in a tests PR or an implementation PR: `status.toml` is founder-owned and
protected, so the `[mandate]` table arrives in a status PR per DEC-77.

### Fixture check before any code

Every figure below was recomputed from the spec text, by hand, before any type was written
(AGENTS.md, "Validate fixtures against the rules"). All agree with the fixture.

| Case | Claim | Recomputed |
|---|---|---|
| MC-V01 | The base accumulator's worst-case block | A = 10000; cap = min(`max_position_usd` 10000, `max_position_fraction` 1 × A) = 10000; `crypto` is in `asset_classes`, so the stop distance is 0.08 + 0.005 = 0.085; one position at its stop = 850; daily budget = 0.02 × 10000 = 200; flatten loss = `max_drawdown` 0.08 × 10000 = 800; lifetime floor loss = 0.1 × 10000 = 1000. 850 > 200, so W-002 fires |
| MC-R01 step 2 | The first rung binds exactly at its `at` | E = 10000 + 100 × 101.85 − 10000 = 10185; H = 10500; H − E = 315 = 0.03 × 10500, and the rule is `>=`, so `drawdown_ladder[0]` triggers on the boundary |
| MC-R01 step 3 | Reported drawdown rounds half-even at 12 places | (10500 − 10100) ÷ 10500 = 0.0380952380952380…; the 13th place is 2, so 0.038095238095 |
| MC-R11 | A withdrawal never lifts a limit | E = 9500, Δ = −5000, E′ = 4500, H = 10000; ceil(10000 × 4500 ÷ 9500, 12) = ceil(4736.842105263157894…, 12) = 4736.842105263158, so DD = (4736.842105263158 − 4500) ÷ 4736.842105263158 rounds to 0.05, exactly the pre-change value and never below it |
| MC-R11 | And the reverse deposit does not lower it either | E = 4500, Δ = +5000, E′ = 9500, H = 4736.842105263158; ceil(H × 9500 ÷ 4500, 12) = ceil(10000.0000000000003…, 12) = 10000.000000000001 |
| MC-R19 step 2 | One flash print sets `hard_breach` and latches nothing | E = 9440, H = 10500, H − E = 1060; rung 1 (`at` 0.06) hard level 1.25 × 0.06 × 10500 = 787.5 and rung 2 (`at` 0.08) hard level 1050 are both reached, so both rungs journal `hard_breach_pending` while one `hard_breach` restriction is set; the mode becomes `exits_only`; nothing is in `latched`, and both rungs are `pending` while `drawdown_ladder[0]` (a `scale_sizes` rung, which does not confirm) is not |
| MC-T01 to MC-T05 | The three risk-day lengths | 2026-03-07 is 86400 s (the change is at 07:00Z on 03-08, after the day boundary at 05:00Z); 2026-03-08 runs 05:00Z to 04:00Z = 82800 s; 2026-11-01 runs 04:00Z to 05:00Z = 90000 s |
| MC-L01 | `accumulate` completes on quantity | `target_qty` 0.15 − position 0.15 = 0, below the 0.0001 increment, so `done` with reason `target_qty` and `then` the mandate's `on_complete` (`hold_protected`) |
| MC-C01 | Raising the last rung is one increasing change | `/risk/max_drawdown` is a maximum and grew; the ladder's actions are unchanged but an `at` grew, so both paths are increasing and the version is `risk_increasing` with step-up required. The changed paths are `/risk/drawdown_ladder` and `/risk/max_drawdown`: an array is compared whole, never element by element |

### Invariants touched

Every MI-n the mandate spec §1.1 states that this stream can decide, each with a named test whose
oracle computes the answer its own way. MI-9, MI-10, MI-15 to MI-20 belong to streams G, H, and J;
MI-12 is asserted by the semantic cases rather than fuzzed, as §1.1 says.

| Clause or invariant | Test |
|---|---|
| §4.1 each V-rule fires exactly on the mandates that break it, and `validate` returns every violated code, sorted | `validate::one_test_per_rule` (39 named cases, one per V-code, each with a mandate that breaks only that rule), `properties::a_valid_mandate_reports_no_code_and_an_invalid_one_reports_every_code_it_breaks` |
| §4.2 warnings never block, and each is computed from the document alone | `validate::w001_to_w006`, `properties::warnings_never_appear_in_violations` |
| §4.2 the confirmation screen's four dollar figures | `validate::worst_case_figures_are_the_spec_products`, `properties::worst_case_is_monotone_in_the_allocation` |
| MI-12 every envelope field is user-sourced and confirmed; a `platform_default` is valid only on the closed §7 list with its listed value; every `auto` is `user_entered` | `validate::v020_closed_platform_default_list`, `validate::v022_every_auto_is_user_entered`, `validate::v038_never_proposed_paths`, `properties::a_proposed_or_unconfirmed_envelope_field_always_reports_v020` |
| §4.3 a child may only tighten, and a violation reports the **nearest** ancestor it breaks | `policy::nearest_ancestor_is_reported`, `properties::policy_violations_name_the_nearest_broken_ancestor` (oracle scans the chain in the opposite direction) |
| §4.3 each key kind: maximum, minimum, permission, requirement, set | `policy::one_test_per_key_kind`, `properties::tightening_any_key_never_creates_a_violation` |
| §4.3 the retail and internal research profiles, and the platform base | `policy::retail_profile_bounds`, `policy::internal_research_profile_bounds`, `policy::platform_base_bounds` |
| §5.2 evaluation order per input: settle time, apply, E then H, rungs in ascending `at`, daily loss, floor, effective mode; journal events follow that order | `risk::journal_order_follows_the_evaluation_order`, `properties::emitted_events_are_in_the_spec_order` |
| MI-5 H >= E and 0 <= DD < 1 | `properties::the_high_water_mark_never_falls_below_equity` |
| MI-6 the effective mode is the strictest active restriction, and `AgentModeApplied` is journaled exactly when it changes | `risk::the_strictest_restriction_wins`, `properties::mode_events_appear_exactly_on_a_change` |
| §5.6 breach time accumulates over intervals that start with the condition true, and resets only after `breach_confirm_s` of continuous falsity | `risk::a_short_recovery_does_not_restart_confirmation`, `risk::a_recovery_longer_than_the_window_restarts_it`, `properties::confirmation_matches_an_independent_interval_accumulator` |
| MI-4, DEC-63 a hard breach applies `exits_only` at once and latches only on a second sane quote at least min(`breach_confirm_s`, 10) s later; a quote below the level clears it | `risk::a_single_flash_print_latches_nothing`, `risk::a_flatten_hard_trigger_needs_a_second_quote`, `properties::one_bad_print_never_latches_a_limit` |
| §5.4 a breach still confirming at the rollover keeps confirming against the previous day's E0, and is discarded if the condition stays false | `risk::a_pending_breach_resolves_at_the_rollover`, `risk::a_flash_breach_before_midnight_is_discarded`, `properties::a_rollover_breach_is_measured_against_the_day_it_started_in` |
| §5.4 the lift needs a new risk day, `daily_breach_min_s`, and (for `flatten_and_pause`) the owner; a confirmed new-day breach renews the latch | `risk::the_daily_lift_waits_for_the_minimum`, `risk::a_new_day_breach_renews_the_latch`, `risk::a_daily_flatten_acknowledged_after_flat_leaves_exits_only` |
| MI-3 latched limits lift only by their defined path | `properties::no_latch_lifts_except_by_its_defined_path` |
| §5.5 the size factor is the product of the active rungs' factors | `risk::the_size_factor_is_the_product_of_active_rungs`, `properties::the_size_factor_never_exceeds_one` |
| §5.8 acknowledgment is rejected while a flatten is unfinished; otherwise H := E, the latched rungs lift, every `scale_sizes` rung is set active, and the rungs then lift one at a time, highest `at` first, each after `scale_lift_after_s` of regular-session time | `risk::acknowledgment_is_rejected_while_flattening`, `risk::rungs_lift_one_at_a_time_highest_first`, `properties::a_reset_never_lowers_the_size_factor_immediately` |
| §5.7 the floor cannot be acknowledged and lifts only on a loosening version, with the waiting period, the `still_below_new_floor` test, and the `not_loosening` test | `risk::the_floor_cannot_be_acknowledged`, `risk::loosening_the_floor_waits_a_full_risk_day`, `risk::a_version_that_leaves_equity_below_the_new_floor_is_rejected` |
| MI-14 the loss carried to the connection is max(0, net contributed − E), whatever withdrawals came first | `risk::a_withdrawal_cannot_shrink_the_loss_carry`, `properties::the_loss_carry_is_invariant_under_a_withdraw_then_deposit_pair` |
| MI-2, MI-7 an applied allocation change never triggers or lifts a limit, never lowers drawdown or the daily loss fraction, and never raises floor headroom or agent return; an increase is rejected while any limit is latched | `risk::an_increase_is_rejected_while_latched`, `risk::a_decrease_below_exposure_is_rejected`, `properties::an_applied_allocation_change_preserves_every_ratio_and_arms_nothing` |
| §5.2 for equities only regular-session sane marks update E; staleness is per instrument and measured in regular-session time; a stale mark never triggers a flatten | `risk::extended_hours_marks_are_ignored_for_equities`, `risk::a_missing_mark_sets_stale_mark_and_a_sane_one_clears_it`, `properties::a_stale_or_insane_mark_never_latches_a_limit` |
| MI-13 dropping clock ticks that emitted no events changes no later result | `properties::silent_ticks_can_be_dropped` |
| MI-8 the same mandate and inputs give identical outputs | `properties::identical_inputs_give_identical_states_and_events` |
| §5.9 an instrument restriction blocks that instrument only, and one event is emitted per restriction that changed | `risk::a_removed_instrument_is_exits_only_in_that_instrument`, `properties::one_instrument_event_per_changed_restriction` |
| §3.1 goal completion for each type and each `on_complete`, and `profit_stop` confirmed inside the risk state with no hard trigger | `goal::one_test_per_row_of_the_goal_table`, `risk::profit_stop_confirms_by_time_in_breach`, `properties::a_completed_goal_never_adds_risk` |
| §5.4 the risk day is 00:00 to 00:00 America/New_York, 23 or 25 hours on a change day | `risk_day::three_lengths`, `properties::risk_days_tile_the_timeline_without_gap_or_overlap` |
| §9.1 the version is `sha256:` plus the SHA-256 of the canonical JSON, and provenance is not in it | `change::the_version_vector`, `properties::equal_documents_hash_equally_and_a_one_bit_change_does_not` |
| §9.2 every classification row, and the pinning switch in both directions | `change::one_test_per_row` (about 30 named cases), `change::pinning_from_a_version_with_an_admitting_model_is_reducing`, `change::pinning_from_a_version_without_one_is_increasing`, `change::unpinning_is_increasing` |
| MI-11 a version classified reducing or neutral never makes any autonomy decision less strict | `properties::a_reducing_or_neutral_version_never_loosens_an_autonomy_decision` (the oracle evaluates every generated action under both rule sets) |
| §9.2 the version is increasing if any path is, otherwise reducing if any path is, otherwise neutral; an unlisted path is increasing | `change::an_unknown_path_is_increasing`, `properties::classification_is_the_join_over_changed_paths` |
| ES-22 the Rust parse accepts exactly what the JSON Schema accepts, each decimal against its field's **whole** `$def` — the pattern and, for `decimal`, the `not: {const: "-0"}` beside it | `schema::one_test_per_s_case`, `properties::the_parse_agrees_with_each_fields_whole_schema_def`, `properties::a_schema_dec_is_its_own_dec_str_normal_form` |
| ES-21 no clock, no randomness, `BTreeMap`, exact arithmetic or a typed error | `properties::no_output_depends_on_iteration_order`, `num::every_new_operation_matches_its_integer_oracle` |
| The harness reads every key every owned case states (DEC-85) | `harness::every_owned_case_key_is_read`, `harness::a_family_another_stream_owns_fails_with_its_story`, `harness::a_wrong_expected_value_fails_the_case` |

### Oracles

Four independent oracles, each shown to fail on a seeded bug before it is trusted (AGENTS.md,
"Independent oracles"):

1. **Risk state, from the journal.** `crates/mandate-spec/tests/properties.rs` holds a second
   reader that rebuilds the latched set, the restrictions, the mode, and the high-water mark **only
   from the events each step emitted**, and asserts it equals the snapshot the step reported. A
   limit that latches without journaling, or journals without latching, fails. Equity, H, E0, C, and
   L are recomputed in a separate `i128` accumulator at 10^-12 with its own ceiling arithmetic.
2. **Breach time, from the input list.** A second accumulator walks the step list and credits each
   interval by the condition at the interval's **start**, computed from its own equity series, so it
   shares no code with `Confirm`. A test seeds the "resets on the first false interval" bug and
   watches the oracle catch it.
3. **Policy, scanned the other way.** The implementation walks ancestors from nearest to furthest
   and stops at the first violation; the oracle walks from the platform level outward and keeps the
   last violation it saw. Both must name the same level.
4. **Classification, by behaviour.** For MI-11 the oracle never looks at
   `change::classify`'s reasoning: it generates an action set, evaluates the old and new autonomy
   blocks over it (through the condition tree `mandate-spec` exposes), and asserts that a version
   classified reducing or neutral never returns a less strict decision for any action. A generator
   that only produced rules nothing matched would make this vacuous, so the property first asserts
   that at least one generated action matches at least one rule.

## Crate split and layers

Two new crates, both `pure = true` and `safety_critical = true` (ES-02 already plans both, and lists
`mandate-spec` at layer 3 and `mandate-domain` at layer 1):

| Crate | Layer | Holds | Why not elsewhere |
|---|---|---|---|
| `mandate-domain` | 1 | The vocabulary three M5 streams share: `AssetClass`, `AssetId`, `WorkingUniverse`, `Environment`, `Side`, `Purpose`, `AutonomyDecision`, `AgentMode`, `Session`, `DomainError` | Putting them in `mandate-spec` would make `mandate-risk` (4), `mandate-builder` (5), and any later connector depend on the mandate rules to name an asset class. ES-02's chain is `num, time, canon -> domain -> journal, accounting -> spec`, and the known-issues row ("`AssetClass` exists in both `mandate-accounting` and `mandate-marketdata`; move it to `mandate-domain`") names the story that creates the crate as its owner |
| `mandate-spec` | 3 | The mandate document, validation, the policy hierarchy, the risk state, risk days, goals, the condition language, and change classification | These are the rules the gate enforces, so they must sit **below** the gate: `mandate-risk` at layer 4 reads them and cannot be where they are defined (AGENTS.md rule 1, "limits are enforced by the risk gate, independent of agent logic") |

`mandate-spec` depends on `mandate-num`, `mandate-time`, `mandate-canon`, and `mandate-domain`, and
**not** on `mandate-journal` or `mandate-accounting`, although layer 3 would allow it. The risk
state is a transition function over values (equity components, marks, fills, acknowledgments); the
account sub-ledger fold stays in `mandate-accounting` and the executor wires the two together
(§5: "The executor computes the risk state from the account ledger"). Depending on the fold would
pull the whole account model into a rule crate and give two crates a claim on the same numbers.

`allowed_external` is `["thiserror"]` for both. `xtask/layers.toml` and `CODEOWNERS` are
founder-owned, so the four entries the tests PR adds are the items this brief cannot take alone.

### Shared crates the tests PR touches

- **`mandate-num`** (shared-crate additions only; ES-04 keeps exact arithmetic there, so
  `mandate-spec` divides nothing itself). `Ratio` (signed, 24 places) and
  `Usd::ratio_to(other, scale, mode)` — which is what DD, the daily P&L fraction, and
  `position_pnl_fraction` need — arrived on `main` with E4-2's tests PR (#118, DEC-127 item 14), and
  `CostBasis::portion` already carries §8.1's reduction. This stream adds only what §4.2 and §5 need
  on top: `Usd::at_least_ratio_of(ratio, of)` and the three other exact comparison predicates §5.2
  asks for ("comparisons are exact"), which cross-multiply on 256-bit intermediates and never
  materialise a rounded product; `Usd::times_ratio(ratio, scale, mode)` for the reported figures;
  `Usd::scaled_by(numerator, denominator, scale, mode)`, one rounding, for §5.1's
  ceil(X × (E + Δ) ÷ E, 12); and `Ratio::{times, complement}`.
  Each gets an integer oracle in `crates/mandate-num/tests/num.rs`.
- **`mandate-accounting`**: one line, `pub use mandate_domain::AssetClass;`, replacing the local
  enum. Its variants are identical and it carries no methods, so no call site changes.
- **`mandate-refcases`**: the new `mandate` suite (below).

## Data shapes

The caller's view, written before any logic; these are the tests PR's stubs.

### The hashed envelope document

```rust
pub struct Mandate { /* private fields; every accessor below */ }

pub enum DecGrammar { Decimal, PositiveDecimal, Fraction, OpenFraction, UnitPositive }
pub struct SchemaDec { /* the field's text and the grammar it satisfied */ }

impl SchemaDec {
    pub fn parse(text: &str, grammar: DecGrammar) -> Result<Self, ParseError>;
    pub fn as_str(&self) -> &str;
    pub fn to_dec_str(&self) -> DecStr;      // the identity on a SchemaDec, by construction
    pub fn to_usd(&self) -> Result<Usd, SpecError>;
    pub fn to_ratio(&self) -> Result<Ratio, SpecError>;
}

impl Mandate {
    pub fn parse(value: &Value) -> Result<Self, ParseError>;
    pub fn canonical_bytes(&self) -> Vec<u8>;
    pub fn version(&self) -> MandateVersion;

    pub fn schema_version(&self) -> u32;
    pub fn name(&self) -> &Name;
    pub fn source_text_ref(&self) -> Option<&Digest>;
    pub fn environment(&self) -> Environment;
    pub fn connection_id(&self) -> &ConnectionId;
    pub fn capital(&self) -> &Capital;
    pub fn goal(&self) -> &Goal;
    pub fn universe(&self) -> &Universe;
    pub fn behavior(&self) -> &Behavior;
    pub fn protection(&self) -> &Protection;
    pub fn risk(&self) -> &Risk;
    pub fn autonomy(&self) -> &Autonomy;
    pub fn notifications(&self) -> &Notifications;
    pub fn at(&self, path: &Pointer) -> Option<Field<'_>>;
}

pub struct MandateVersion(Digest);
pub struct Pointer(String);
```

- **`parse` takes `mandate_canon::Value`,** the canonical value tree, not `serde_json::Value`: the
  document reaches the platform as canonical JSON (journal spec §4) and `mandate-spec` must not
  carry a JSON parser of its own. It is strict in exactly the schema's way: an unknown member, a
  missing required member, a decimal sent as a JSON number, a value outside an enum, or a string off
  its pattern is a `ParseError` naming the JSON Pointer and the reason. That is what makes the 31
  MC-S cases a test of the Rust parse and satisfies ES-22 ("the Rust mandate parser must agree with
  `jsonschema` validation on every MC-S case").
- **Every decimal field is a `SchemaDec`,** a `mandate-spec` newtype holding the field's text
  together with the `$def` grammar it satisfied — not a `mandate-num` type, whose scales are
  narrower, and **not a bare `mandate_canon::DecStr`**, which is a *normalising* wrapper over the
  wider journal grammar (§4.6): `DecStr::parse` returns `Ok("0.02")` for MC-S04's `0.020`, also
  accepts `007.50`, `1e3`, and `.5`, allows 29 integer digits where the schema allows 28, and knows
  nothing of the narrower `fraction`, `open_fraction`, `unit_positive`, and `positive_decimal`
  grammars. So `SchemaDec::parse` checks the raw text against the field's whole `$def` **first** and
  only then wraps it — the pattern *and*, for `decimal`, the `not: {const: "-0"}` beside it, which is
  load-bearing: the `decimal` pattern alone matches `-0`, and `DecStr::parse("-0")` returns `"0"`
  because all-zero digits normalise before the sign is applied. Checking the whole `$def` makes every
  schema grammar canonical — no leading zeros beyond a bare `0`, a fractional part ending in a
  non-zero digit, no exponent, and no `-0` — so text that passes it is already `DecStr`'s normal form,
  `DecStr::parse` on it is the identity, and `canonical_bytes` round-trips. A property test asserts
  that identity over generated values rather than assuming it, and planted bug 21 is an
  implementation that checks the pattern and drops the `-0` exclusion. Each rule converts the fields
  it computes with, and a value outside the target type is a typed error naming the path, never a
  rounded number (interpretation 4).
- **`decimal` is the only signed grammar, and the only one with that exclusion.** It is referenced
  exactly once, by `behavior.signal_models[].params[].value`, whose schema is an `anyOf` of
  `decimal`, a boolean, and a string of at most 200 characters, so a parameter value is a `SchemaDec`
  only on the decimal branch. Every other decimal in the document uses `positive_decimal`,
  `fraction`, `open_fraction`, or `unit_positive`, all unsigned.
- **`SchemaDec` orders by decimal value** (`Ord`, total and exact): compare signs first, then, for
  two values of the same sign, the integer-part length and then the text, which the canonical grammar
  makes sufficient — **reversed for two negatives**, where longer and lexically greater text means
  smaller. Only the `decimal` grammar can be negative, so only `params[].value` reaches that branch;
  V-012, V-013, and V-014 compare unsigned fields. That is all they need, so no arithmetic type and
  no change to `mandate-canon` is involved.
- **`canonical_bytes` round-trips.** `Mandate::parse(v)` then `canonical_bytes` reproduces the bytes
  `mandate-canon`'s writer produces for `v`, so the version hash cannot drift from the document the
  owner confirmed. `mandate::version_vector` checks the bytes and the digest against the fixture.
- **Provenance is not in the document** (§2.1, §9.1): it is a separate map, so the same fields hash
  the same however they were sourced.

```rust
pub struct ProvenanceMap(BTreeMap<Pointer, Provenance>);
pub struct Provenance { pub source: Source, pub confirmed: bool }
pub enum Source { UserStated, UserEntered, TemplateStructure, PlatformProposed, PlatformDefault }
```

A path the map does not mention is `{ UserEntered, confirmed: true }`, as the fixture's harness
rules state.

### Strategy fields: what the document does not hold

§3.2's runtime state is **not** in `Mandate`. The working universe, the current thesis per
instrument, the lineage state, and the day's research spend are folds of `UniverseChanged`,
`ThesisProposed`, `ThesisRevised`, and `ModelInvocationRecorded`, produced by streams G and J.
Only the universe's **type** is shared, so it lives in `mandate-domain` — one type, not one per
stream — in the shape stream G's gate actually needs:

```rust
pub enum WorkingUniverse {
    Known { instruments: BTreeSet<AssetId>, pinned: bool },
    Unavailable,
}
```

`Unavailable` is a real state and not an empty set: §5.3 check 2 must deny every opening while the
fold has not been read, which an empty `Known` would also do but would report as "universe empty"
rather than "universe unknown". Stream G's merged brief
([#127](https://github.com/kunwarshivam/mandate/pull/127)) defines this enum over `InstrumentId`
inside a `#[doc(hidden)]` `crates/mandate-risk/src/spec_types.rs`, together with a placeholder
`RiskState` whose field is `day_open_equity`; its own Dependencies section has the first
implementation PR after F's tests PR merges delete that module and take F's types, so no second
design survives and the two name differences resolve there (DEC-128 item 21).

**One id type.** `mandate_domain::AssetId` is the schema's `$defs/uuid` form (`asset_id` in
`instrument_ref`, and the `instrument_id` of a model output), which is what every mandate rule and
every reference case names. `mandate_accounting::InstrumentId` stays the broker-facing id, "any
non-empty string", and is the wider type: `AssetId -> InstrumentId` is total and infallible,
`InstrumentId -> AssetId` is fallible and is only ever needed at a connector boundary. Streams G and
H use `AssetId` for anything that comes from a mandate, a thesis, or a reference case.

The risk state takes `UniverseChanged` as one of its inputs (§2.3: "a risk input carrying
`risk_clock`") and turns it into an instrument restriction; it never decides admission.

### Validation

```rust
pub struct ValidationContext {
    pub account_equity_usd: Usd,
    pub other_allocations_usd: Usd,
    pub validation_date: Date,
    pub registry: Option<BTreeMap<ModelId, RegisteredModel>>,
    pub provenance: ProvenanceMap,
    pub workspace_users: u32,
    pub approver_users: u32,
    pub disclosures_accepted: BTreeSet<Digest>,
    pub instrument_groups: BTreeMap<AssetId, GroupId>,
    pub claimed_by_other_agents: BTreeSet<AssetId>,
    pub connection_environment: Option<Environment>,
    pub connection_loss_carry_usd: Usd,
    pub eligibility_failures: BTreeSet<AssetId>,
    pub previous_version: Option<PreviousVersion>,
}

pub struct ValidationReport {
    pub violations: BTreeSet<Violation>,
    pub warnings: BTreeSet<Warning>,
    pub worst_case: WorstCase,
}

pub struct WorstCase {
    pub one_position_at_stop_usd: Option<Usd>,
    pub daily_loss_budget_usd: Usd,
    pub flatten_trigger_loss_usd: Usd,
    pub lifetime_floor_loss_usd: Usd,
}

pub fn validate(m: &Mandate, ctx: &ValidationContext)
    -> Result<ValidationReport, SpecError>;

pub struct ValidatedMandate(Mandate);

impl ValidatedMandate {
    pub fn new(m: Mandate, ctx: &ValidationContext, policies: &[PolicyLevel])
        -> Result<Self, Rejected>;
    pub fn mandate(&self) -> &Mandate;
}
```

`ValidatedMandate` is the type streams G, H, and J take. It has no other constructor, so a gate
cannot be handed a mandate nobody validated, and `Rejected` carries the report and the policy
violations rather than a bare error. `validate` itself returns the full report — every violated code
at once, as §4 requires — and reserves `Err` for an input it cannot evaluate at all (a decimal
outside the arithmetic range, interpretation 4).

### Rule numbering and error codes

The V- and W-codes are the spec's own stable identifiers, so they are the code (ES-09):

```rust
pub enum Violation { V001, V002, V003, V005, V006, V007, V008, V009, V010, V011, V012, V013, V014,
                     V015, V016, V017, V018, V020, V022, V023, V024, V030, V031, V032, V033, V034,
                     V035, V036, V037, V038, V039 }

impl Violation {
    pub fn code(self) -> &'static str;   // "V-001" .. "V-039"
    pub fn spec_section(self) -> &'static str;
}

pub enum Warning { W001, W002, W003, W005, W006 }
```

There is no V-004, V-019, V-021, V-025 to V-029, or W-004: §4.1 and §4.2 do not define them (v0.1's
V-021 was withdrawn in the rewrite), and the enum has no variant for a code the spec does not state,
so a reader cannot mistake a gap for an unimplemented rule. `BTreeSet<Violation>` gives the sorted
order the cases expect, ordered by code. The type is `Violation`, not `Rule`, because
`autonomy.rules[]` is the crate's `Rule` (§6.3) and stream H reads it by that name.

`ParseError`, `SpecError`, `PolicyError`, and `DomainError` are `thiserror` enums, each variant with
a stable `code()` (ES-09): `unknown_member`, `missing_member`, `not_a_decimal`,
`decimal_as_number`, `off_pattern`, `not_in_enum`, `out_of_bounds`, `too_deep`, `out_of_range`,
`clock_went_backwards`, `unknown_restriction`, `not_acknowledgeable`, `nothing_to_acknowledge`,
`flatten_in_progress`, `increase_blocked_while_latched`, `equity_below_exposure`,
`would_trigger_limit`, `not_loosening`, `waiting_period`, `still_below_new_floor`, and
`unimplemented` (the tests PR's stubs only; it is gone from the crate by the last implementation PR,
which the pending gate proves by requiring every pending test to fail on the stubs).

### Policy resolution as a pure function

```rust
pub struct PolicyLevel { pub name: LevelName, pub values: PolicyValues }
pub enum LevelName { Platform, Organization, Workspace, Mandate }
pub struct PolicyValues(BTreeMap<PolicyKey, PolicyValue>);
pub enum PolicyKey { /* the §4.3 keys, one variant each */ }
pub enum PolicyValue { Decimal(SchemaDec), Integer(u64), Flag(bool), Set(BTreeSet<String>), Absent }

pub struct PolicyViolation {
    pub key: PolicyKey,
    pub level: LevelName,
    pub value: PolicyValue,
    pub limit_level: LevelName,
    pub limit: PolicyValue,
}

pub fn policy_values_of(m: &Mandate) -> PolicyValues;
pub fn check(m: &Mandate, levels: &[PolicyLevel]) -> Result<PolicyResult, PolicyError>;

pub struct PolicyResult {
    pub violations: Vec<PolicyViolation>,
    pub overlay: PolicyOverlay,
}

pub struct PolicyOverlay { /* private */ }

impl PolicyOverlay {
    pub fn effective(&self, key: PolicyKey, mandate_value: &PolicyValue) -> PolicyValue;
    pub fn auto_allowed(&self) -> bool;
}
```

`check` takes the levels outermost first and nothing else: no registry lookup, no I/O, no clock. It
does two things — report violations, and fold the chain into the **overlay** §4.3 describes ("they
apply to running agents at the next evaluation as an overlay: the stricter value governs, and `auto`
evaluates as `ask` when `auto_allowed` becomes false"). `PolicyOverlay` is the second type streams G,
H, and J consume: H asks `auto_allowed` before returning `auto`, G asks `effective` for the ceiling
of each limit it enforces, and J asks `effective` for `max_instruments`,
`research_cost_cap_usd_per_day`, `research_interval_s`, `max_revisions_per_lineage`,
`research_agent_allowed`, `admission_auto_allowed`, and `stagger_window_s`. Those bear on §8.5
checks 4 (`research_disabled`), 6 (`admission_denied`), 7 (`cost_cap_reached`), 16
(`lineage_retired`, through `max_revisions_per_lineage`), and 17 (`universe_full`, through
`max_instruments`), plus §8.4's proposal interval and stagger window, which are timing rather than
admission checks. Check 5 (`universe_pinned`) is not among them: it reads `universe.pinned`, an
envelope field, and is MI-20 rather than a policy key. A lowered `max_instruments` therefore
**refuses further admissions and removes nothing**: §8.5 check 17 refuses a thesis when the universe
is already at the ceiling, and a refusal
admits nothing and changes the universe only in the one `lineage_retired` case (§8.6 item 4), so
MI-19 still holds and an over-ceiling universe drains by expiry, never by a forced removal. Keeping
the overlay next to the check means one definition of "stricter", tested once.

### Change classification as a pure function

```rust
pub fn classify(old: &Mandate, new: &Mandate) -> Classification;

pub struct Classification {
    pub class: ChangeClass,
    pub changed_paths: Vec<Pointer>,
    pub step_up_required: bool,
}

pub enum ChangeClass { RiskIncreasing, RiskReducing, Neutral, Invalid }
```

`classify` reads two documents and returns a verdict: no context, no policy, no state. The changed
paths come from one walk of the two canonical value trees, which recurses into objects and compares
arrays whole (so a ladder change is `/risk/drawdown_ladder`, never
`/risk/drawdown_ladder/2/at`; MC-C01 depends on this). `Invalid` is the §9.2 row for `environment`
and `connection_id`, reported instead of a class rather than as a V-code, because V-031 already
names it at validation and §9.2 calls the change itself invalid. `step_up_required` is
`class == RiskIncreasing` (§9.2's last paragraph); independent approval is a policy question and
belongs to `PolicyOverlay`, not here.

The §9.2 pinning row is one function, `pinning_switch(old, new, paths)`, because DEC-121 classifies
the whole mode change as one thing rather than field by field. It is the only place where the
verdict is not the join over changed paths, so it is named, tested on both directions, and tested on
each of its five conditions failing.

### The risk state

```rust
pub struct RiskState { /* private */ }

pub struct Opening {
    pub position_qty: Qty,
    pub avg_cost: Price,
    pub asset_class: AssetClass,
    pub at: UtcNanos,
    pub inherited_loss_usd: Usd,
    pub mark_max_age_s: u32,
}

pub enum Input {
    Mark { bid: Price, sane: bool },
    Fill { side: Side, qty: Qty, price: Price },
    RiskDayStarted,
    OwnerAcknowledged { restriction: Latch },
    AllocationChange { delta_usd: Usd },
    Clock,
    UniverseChanged { instrument: AssetId, change: UniverseChange, reason: RemovalReason },
    FloorLoosened { new_max_loss_from_allocation: SchemaDec, confirmed_at: UtcNanos,
                    independent_approval: bool },
    AgentStopped { reason: StopReason },
    GoalComplete,
}

pub struct Step { pub at: UtcNanos, pub session: MarketSession, pub input: Input }

pub struct Outcome {
    pub snapshot: Snapshot,
    pub journal: Vec<RiskEvent>,
    pub pending: BTreeSet<LimitKey>,
    pub rejection: Option<Rejection>,
}

pub struct Snapshot {
    pub agent_equity: Usd,
    pub high_water_mark: Usd,
    pub drawdown: Ratio,
    pub day_start_equity: Usd,
    pub daily_pnl: Usd,
    pub daily_pnl_fraction: Ratio,
    pub capital_base: Usd,
    pub inherited_loss: Usd,
    pub size_factor: Ratio,
    pub latched: BTreeSet<LimitKey>,
    pub active_rungs: BTreeSet<u8>,
    pub restrictions: BTreeSet<Restriction>,
    pub agent_mode: AgentMode,
    pub instrument_restrictions: BTreeSet<InstrumentRestriction>,
    pub net_contributed: Usd,
}

pub enum LimitKey { MaxDailyLoss, DrawdownRung(u8), LifetimeFloor, ProfitStop }
pub enum Latch { DailyLoss, DrawdownLadder, LifetimeFloor }
pub enum Restriction { DailyLoss, DrawdownExitsOnly, DrawdownFlatten, LifetimeFloor, HardBreach,
                       GoalComplete, Retired }
pub enum InstrumentRestriction { StaleMark, RemovedInstrument }

pub enum RiskEvent {
    MandateVersionApplied { result: ApplyResult },
    RiskDayStarted { day_start_equity: Usd },
    RiskLimitTriggered { limit: LimitKey, action: LimitAction, reason: Option<TriggerReason> },
    RiskLimitLifted { limit: LimitKey, action: Option<LimitAction>, reason: Option<LiftReason> },
    HighWaterMarkReset { from: Usd, to: Usd },
    AgentModeApplied { from: AgentMode, to: AgentMode },
    KillSwitchActivated { scope: KillScope, initiator: LimitKey },
    UniverseChanged { instrument: AssetId, change: UniverseChange, reason: RemovalReason },
    InstrumentRestrictionChanged { restriction: InstrumentRestriction,
                                   reason: RestrictionReason, active: bool },
    GoalCompleted { reason: Option<GoalReason>, then: Option<ThenAction>,
                    on_complete: Option<OnComplete> },
    PositionReleased { qty: Qty },
    AgentStopped { reason: StopReason, loss_carry_usd: Usd },
}

impl RiskState {
    pub fn open(m: &ValidatedMandate, opening: &Opening) -> Result<Self, SpecError>;
    pub fn step(&mut self, step: &Step) -> Result<Outcome, SpecError>;
    pub fn snapshot(&self) -> Snapshot;
}

pub fn risk_day(at: UtcNanos) -> Result<RiskDay, SpecError>;
pub struct RiskDay { pub day: Date, pub starts_at: UtcNanos, pub ends_at: UtcNanos,
                     pub length_s: u32 }
```

`Snapshot` is the third type the other streams consume: G reads `size_factor`, `restrictions`,
`agent_mode`, `instrument_restrictions`, and `agent_equity` for the §5.3 limits, `active_rungs` for
the §5.5 `trim_to_target` guard (a rung must have been active for `breach_confirm_s`), and `latched`
and `inherited_loss` for the floor and the acknowledgment paths; H reads `size_factor` and
`drawdown` for the order builder's targets and its condition fields. `latched`, `active_rungs`, and
`inherited_loss` are in the snapshot rather than derived by each caller because oracle 1 rebuilds
exactly those three from the journal and compares them, so a limit that latches without journaling
is caught once instead of per consumer. The field is `day_start_equity`, the name every
`risk_state` case uses in its `expect` block; stream G's brief calls it `day_open_equity` and takes
this name (DEC-128 item 21). Reading a snapshot rather than the state itself means no other crate
can advance the risk clock.

`step` takes the clock with the input, never from a clock read (ES-21), and rejects a time before
the last step (`clock_went_backwards`). An `AllocationChange` whose `at` is later than the last step
settles time first as its own evaluation and then applies, with no time passing, exactly as §5.1
requires ("applied when its version applies, after time has been settled at that instant, with no
time passing"); the two evaluations' events concatenate in order.

### Goals

```rust
pub struct GoalInputs {
    pub now: UtcNanos,
    pub position_qty: Qty,
    pub goal_spent_usd: Usd,
    pub min_order_usd: Usd,
    pub qty_increment: ShareIncrement,
    pub ask: Price,
}

pub enum OnComplete { HoldProtected, DisarmLadder, Release }
pub enum ThenAction { Applied(OnComplete), DiscretionaryExitAllThenRetire }

pub enum GoalStatus {
    Running,
    ConfirmedInRiskState,
    Done { reason: GoalReason, then: ThenAction, stop_reason: StopReason },
}

pub fn goal_status(m: &ValidatedMandate, inputs: &GoalInputs) -> Result<GoalStatus, SpecError>;
```

`profit_stop` returns `ConfirmedInRiskState` rather than a completion: §3.1 confirms it by breach
time in the risk state, and returning `Running` here would invite a caller to decide it twice.

### Autonomy and the condition language

```rust
pub struct Autonomy {
    pub rules: Vec<Rule>,
    pub default: AutonomyDecision,
    pub admission: AutonomyDecision,
    pub approval: Approval,
}

pub struct Rule { pub id: RuleId, pub when: Condition, pub then: AutonomyDecision }

pub struct Approval {
    pub timeout_s: u32,
    pub on_timeout: OnTimeout,
    pub approvers: Vec<ApproverRef>,
    pub two_approver_above_usd: Option<SchemaDec>,
}

pub enum ConditionField {
    Purpose, OrderUsd, CombinedScore, Instrument, AssetClass, Session,
    FirstTradeInInstrument, NewInstrument, ThesisConfidence, Drawdown,
    DailyPnlFraction, PositionUsdAfter, GrossUsdAfter, BoughtTodayUsd,
    PositionPnlFraction, UnusualInput,
}

pub enum Operator { Eq, Ne, Gt, Gte, Lt, Lte, In, NotIn }

pub enum ConditionValue {
    Enum(String),
    Decimal(SchemaDec),
    Bool(bool),
    List(Vec<String>),
}

pub enum Condition {
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    Compare { field: ConditionField, op: Operator, value: ConditionValue },
}

impl Condition {
    pub fn comparisons(&self) -> Vec<(&Condition, u8)>;      // V-017 depth, V-018, V-023
    pub fn matches(&self, facts: &dyn Facts) -> Result<bool, SpecError>;
    pub fn is_catch_all(&self) -> bool;                      // W-005
}

pub trait Facts {
    fn enum_field(&self, field: ConditionField) -> Option<&str>;
    fn decimal_field(&self, field: ConditionField) -> Option<Ratio>;
    fn bool_field(&self, field: ConditionField) -> Option<bool>;
}
```

`Rule` is `autonomy.rules[]`, which is why the V-code enum is `Violation` (above) and not `Rule`:
stream H reads `Autonomy`, `Rule`, `AutonomyDecision`, and the condition types by these names, so they
are declared here rather than left implicit.

§6.3 is the document's own language, so the tree, its type rules, and `matches` live here, in
`mandate-spec`, and **nowhere else**. §6.2's evaluation order — the gate dry run, the built-in AUTO
purposes, first match wins, the default, and the `autonomy.admission` ceiling — is stream H's, and so
is every implementation of `Facts`. This split is why the A-family cases are H's while V-017, V-018,
and V-023 are this stream's.

Stream H's brief ([#128](https://github.com/kunwarshivam/mandate/pull/128), merged) defines its own
`Condition`, `Field`, `Op`, and `Value`, its own `matches` oracle, and the `ConditionTooDeep`,
`ConditionTypeMismatch`, and `ReservedField` errors. Those are the same §6.3 rule in a second
safety-critical crate, which is the very argument this brief uses to leave family F with stream G, so
they go: `mandate-spec` owns the tree, `Condition::matches`, and those three error variants (they are
V-017, V-023, and V-018, which only `validate` can report). H's own brief already sets the mechanism
— its item 5 and its Dependencies section say that types this brief places in `mandate-domain` or
`mandate-spec` are taken from there **in H's implementation PR**, "which changes no signature in this
brief" — so H's tests PR keeps its narrow views and does not block on F, and its implementation PR
imports these and deletes the copy. H keeps its own `Facts` implementation and its own oracle *for
§6.2*, which is a different rule (DEC-128 item 21).

## The case-loading design

A new module, `crates/mandate-refcases/src/mandate.rs`, following `trading_domain.rs` exactly:

- `pub fn cases(fixture: &Arc<Json>) -> Vec<Case>` returns **one `Case` per case id**, named
  `mandate::MC-S01`, plus `mandate::version` and `mandate::version_vector`. No case is skipped, and
  no two cases share an id (the runner already fails on a duplicate).
- `tests/refcases.rs` gains one `read_fixture(&fixtures, "mandate.json")` arm beside the two it has.
  Every case is pending until `status.toml` lists it, so the whole suite runs only with
  `--include-ignored` and the required checks stay green from the first PR. The suite therefore needs
  no `#[ignore]` marker of its own; the crate's own hand and property tests carry
  `#[ignore = "pending <story>"]`.
- **Base and patch.** `base` names a member of `bases`; `patch` is the RFC 6902 subset the fixture
  uses (`replace`, `add`, `remove`, 274 operations across the 298 cases), applied by
  `patch::apply(base, patch)` in the harness. An unrecognised `op`, a `replace` on an absent member,
  or a pointer that does not resolve fails the case rather than being ignored. Case ids and expected
  values are read from the fixture, never restated in Rust.
- **Dispatch by `kind`**, family by family:

  | `kind` | Runs | Owner |
  |---|---|---|
  | `schema` | `Mandate::parse` on the patched document; `schema_valid` is `Ok(_)` | this stream |
  | `semantic` | `validate` with `context` over `validation_context_defaults`, and `provenance` as the §2.1 map; checks the sorted codes, the warnings, and all four worst-case figures | this stream |
  | `policy` | `check` with `policies` outermost first; checks `valid` and each violation's five members | this stream |
  | `risk_state` | `RiskState::open` from `initial`, then one `step` per entry; checks every member of `expect`, the journal list in order, and `pending` | this stream |
  | `risk_day` | `risk_day(at)`; checks all four members | this stream |
  | `goal` | `goal_status`; checks `done`, `reason`, `then`, `stop_reason` | this stream |
  | `change` | `classify(base, patched)`; checks the class, the changed paths, both version hashes, and `step_up_required` | this stream |
  | `gate` | fails: `not interpreted until E6-3` | stream G |
  | `agent_flatten` | fails: `not interpreted until E6-3` | stream G |
  | `builder` | fails: `not interpreted until E6-2` | stream H |
  | `autonomy` | fails: `not interpreted until E6-2` | stream H |
  | `admission`, `lineage`, `thesis_expiry`, `stagger` | fails: `not interpreted until E17-3` | stream J |
  | anything else | fails as an unknown kind |

  The DEC-85 rule holds inside an owned kind too: an `expect` member, a step field, or an `initial`
  field the harness does not read fails the case naming the story that will read it, so a case
  cannot pass while part of it is ignored. `harness::every_owned_case_key_is_read` walks the whole
  fixture and asserts that every key of every owned case appears in the harness's vocabulary.
- **Pending markers.** Each of the crates' own tests carries `#[ignore = "pending <story>"]` with
  the story that owns its family: `pending E10-1` for schema, semantic, and policy; `pending E10-3`
  for the version vector and classification; `pending E6-4` for the risk state, risk days, and
  goals; `pending E17-1` for the field-split tests (V-034 to V-039 and `platform_proposed`
  provenance). `cargo xtask markers` accepts only `pending E<n>-<n>`, so `pending F` — a stream, not
  a story — is not a marker the workspace can carry (interpretation 2).

## Interpretations (recorded as DEC-128)

1. **Two crates, both safety-critical.** `mandate-spec` (layer 3) and `mandate-domain` (layer 1),
   both `pure = true`, `safety_critical = true`, `allowed_external = ["thiserror"]`. `mandate-spec`
   holds the rules the gate enforces, so it sits below the gate; `mandate-domain` holds the
   vocabulary streams G, H, and J share so none of them depends on the mandate rules to name an
   asset class. `mandate-spec` does not depend on `mandate-journal` or `mandate-accounting`: the
   risk state is a transition function over values and the account fold stays where it is.
   `mandate-domain` takes `AssetClass` from `mandate-accounting` (a one-line re-export closes half
   the known-issues row); it does **not** take `mandate-accounting::InstrumentId` (a looser
   broker-id type whose constructor returns `AccountingError`) or
   `mandate-marketdata::AssetClass` (whose `as_str` is the vendor path spelling `us-equity`, not the
   spec's `us_equity`, and is baked into dataset paths). Those two migrations change public APIs and
   path code, so they stay a separate change and the known-issues row stays open, narrower.
2. **Pending markers name stories, not streams.** `cargo xtask markers` accepts only
   `#[ignore = "pending E<n>-<n>"]` (DEC-77, DEC-110), so each test names the story whose acceptance
   clause covers its family: E10-1 (S, V, P), E10-3 (the version vector and C), E6-4 (R, T, L),
   E17-1 (the field split). Reference cases need no marker: a case absent from `status.toml` is
   pending by construction.
3. **The document keeps its decimals as text, checked against the field's own grammar.** Every
   decimal field is a `SchemaDec`: the raw text plus the `$def` grammar it satisfied, checked as
   text before anything wraps it. It is **not** a bare `mandate_canon::DecStr`, which normalises
   rather than rejects and implements the wider journal grammar of §4.6: `DecStr::parse("0.020")`
   returns `Ok("0.02")`, so MC-S04 would be accepted; it also takes `007.50`, `1e3`, and `.5`,
   allows 29 integer digits where the schema allows 28, and does not know the narrower `fraction`,
   `open_fraction`, `unit_positive`, and `positive_decimal` grammars at all. The check is against the
   whole `$def`, not the pattern alone: `decimal` carries `not: {const: "-0"}` beside its pattern,
   which the pattern itself matches and `DecStr` would normalise to `0`. Text is still the
   storage, because the schema's grammars allow up to 28 integer and 28 fractional digits, which
   `Price` (9 places), `Usd` (28 places of scale but a 96-bit significand), and `Ratio` (24 places)
   cannot hold; keeping it as text is what puts the Rust parse in agreement with `jsonschema` on
   every MC-S case and on fuzzed mandates (ES-22) and keeps the canonical bytes, and therefore the
   version hash, byte-identical to what the owner confirmed. Because the whole `$def` makes every
   grammar canonical, `DecStr::parse` on a `SchemaDec` is the identity, which a property test asserts
   and planted bug 21 attacks. V-012, V-013, and V-014 need only ordering, so `SchemaDec` implements
   `Ord` by decimal value — reversed between two negatives, which only `params[].value` can be — and
   `mandate-canon` is not touched at all.
4. **Out of arithmetic range is a typed error, never a rounded number.** A rule that must compute
   with a field converts it, and a value the target type cannot hold exactly returns
   `SpecError::OutOfRange { path }` with code `out_of_range`. It is not a V-code: the mandate is
   schema-valid and the spec states no such rule, so inventing a violation would put a code in a
   report that no specification defines. This is reachable only through the schema's 28-place
   grammars; see "Decisions needed" for the recommended schema bound.
5. **Comparisons are exact, and never through a rounded product.** §5.2's five comparisons
   (`H − E >= at × H`, the hysteresis form, `E − E0 <= −max_daily_loss × E0`,
   `E <= C × (1 − f) + L`, and each 1.25× hard form) are evaluated by cross-multiplication on
   256-bit intermediates in `mandate-num`, which returns a boolean. No product is materialised, so a
   scale limit can never decide a limit, and a rung cannot fire or fail to fire because a product
   rounded.
6. **Reported ratios round half-even at 12 places; the allocation scaling rounds up at 12.** DD, the
   daily P&L fraction, `position_pnl_fraction`, and the `drawdown` and `daily_pnl_fraction`
   condition fields are `round(x, 12, half_even)` (§5.2). §5.1's H′, E0′, C′, and L′ are
   `ceil(X × (E + Δ) ÷ E, 12)` computed with **one** rounding, which is exactly the conservative
   direction MI-2 needs: every scaled quantity is at or above the exact value, so drawdown and the
   loss fractions never fall and floor headroom never rises. The reference implementation computes
   the same value through a 60-significant-digit intermediate; the two agree on every reference case
   and on every input whose quotient the intermediate does not itself round, and a differential
   property test asserts the agreement rather than assuming it.
7. **The worst-case figures round half-even at 28 places.** §4.2 does not state a scale for the four
   dollar figures. They are products of a 28-place fraction and a 28-place dollar amount, so an
   exact result can need 56 places; one rounding at 28 places, half-even, reproduces every reference
   case exactly and is the widest scale `Usd` carries. The confirmation screen's *display* rounding
   is a separate, user-facing question left to the founder ("Decisions needed").
8. **`ValidatedMandate` is the only way in.** Streams G, H, and J take `&ValidatedMandate`, which
   only `ValidatedMandate::new` constructs and only when the schema parse, every V-rule, and the
   policy hierarchy pass. A gate cannot be handed an unvalidated document, which is what makes
   AGENTS.md rule 1 structural rather than a convention (trust ladder rung 1).
9. **The policy check returns the overlay with the violations.** §4.3's runtime overlay ("the
   stricter value governs, and `auto` evaluates as `ask` when `auto_allowed` becomes false") is the
   same fold over the same chain as the validation check, so one function produces both and
   "stricter" has one definition, tested once. `PolicyOverlay::auto_allowed` is what stream H calls
   before it returns `auto`; `PolicyOverlay::effective` is what stream G calls for a limit's
   ceiling.
10. **Missing is not violating.** A policy level that states no value for a key constrains nothing,
    and a mandate value of `null` violates nothing — except `two_approver_above_usd` and
    `stop_distance_max`, where §4.3 makes an unset mandate value itself the violation ("the mandate
    must set one at or below it"), because "no limit" is looser than any limit. That asymmetry is
    named in the code and has its own test.
11. **Change classification is the join over changed paths, with one exception.** The verdict is
    increasing if any path is, else reducing if any path is, else neutral, and an unlisted path is
    increasing (fail safe, §9.2's last row). The DEC-121 pinning switch is the one rule that looks
    at the whole change at once, so it is a named function tested on both directions and on each of
    its five conditions failing. Arrays are compared whole when the changed paths are computed, so a
    ladder or rule-list change reports the array's path; MC-C01 fixes this.
12. **`Invalid` is a classification, not a violation.** A changed `environment` or `connection_id`
    makes `classify` return `ChangeClass::Invalid` (§9.2) while `validate` returns V-031. Both
    surfaces report it, neither depends on the other.
13. **Warnings and violations are disjoint.** A W-code never appears among the violations and never
    blocks; §4.2's "each must be acknowledged" is the confirmation flow's job (E10-1's surface),
    which reads the report. A property test asserts the two sets never intersect.
14. **The risk state's inputs carry their own time and session.** `Step { at, session, input }`, and
    a step at or before the previous step's time is `clock_went_backwards`. Durations are integer
    seconds; equity staleness and the `scale_lift_after_s` timer count regular-session seconds for
    equities (from `mandate-time`'s NYSE calendar) and all seconds for crypto (§5.2, §5.5). Nothing
    reads a clock (ES-21).
15. **One journal event per thing that changed, in the fixture's exact shape.** `AgentModeApplied`
    only on a change (MI-6); `InstrumentRestrictionChanged` once per restriction that changed, never
    one standing for another (§5.10). A `scale_sizes` rung's trigger and lift carry an `action` and
    no reason; a latch carries a reason only when it is not plain confirmation
    (`hard_trigger`, `resolved_at_rollover`, `new_day_breach`, `after_reset`, `owner_acknowledged`,
    `version_loosened`), and the hard-breach pair adds `hard_breach_pending` to the trigger reasons
    and `hard_breach_cleared` to the lift reasons, which a lift carries *instead of* an `action`
    (MC-R19 step 3 expects `RiskLimitLifted{limit, reason: hard_breach_cleared}` beside
    `RiskLimitLifted{limit, action: scale_sizes}`). Two rungs can both journal `hard_breach_pending`
    while a single `hard_breach` restriction is set (MC-R19 step 2). `GoalCompleted` has two shapes
    in the fixture and the type carries both: `{on_complete}` for the `goal_complete` input
    (MC-R16, MC-R17) and `{reason, then}` for a confirmed `profit_stop` (MC-R22), so every member is
    optional and the harness compares only the members the case states.
16. **A rejected input still produces an outcome.** An allocation change or floor loosening that is
    refused returns `Outcome` with `rejection: Some(..)` and a journaled
    `MandateVersionApplied { result: rejected }`, and the state is unchanged apart from the time that
    was settled. The caller cannot lose the refusal by matching on `Ok`.
17. **The tests land in three PRs against `main`.** ES-13 splits changes over 400 non-generated
    lines in safety-critical crates. Tests PR 1 creates both crates with the **whole** public API as
    stubs (so streams G, H, and J can compile against it at once), adds the `layers.toml` and
    CODEOWNERS entries, wires the `mandate` suite with every family dispatched, and carries the S,
    V, and P tests. Tests PR 2 adds the R, T, and L tests, PR 3 the C tests; neither adds a stub.
    Implementation PRs follow in the same order, each deleting only its own pending markers. Each PR
    targets `main` (never another PR's branch) and states its line split. The coordinator's go
    comment may re-slice this.
18. **The condition language is here; its evaluation order is not.** §6.3's tree, the V-017 depth
    limit, the V-018 reserved field, the V-023 type rules, and `Condition::matches` over a `Facts`
    trait are `mandate-spec`'s. §6.2's order, the built-in AUTO purposes, the admission ceiling, and
    the facts themselves are stream H's, which is why the A-family cases are H's.
19. **Family F (agent flatten) is stream G's.** The work-graph row for stream F lists it, but spec
    §11 assigns MC-F01 to MC-F04 to *agent flatten*, and stream G's **merged** brief
    ([#127](https://github.com/kunwarshivam/mandate/pull/127), claim
    [#123](https://github.com/kunwarshivam/mandate/issues/123)) owns those four cases under E6-3,
    with `agent_flatten` in its public API and its own hand and property tests for them;
    together with the `agent_flatten` harness interpretation, and the plan reads no mandate field at
    all (it takes open orders, sub-ledger positions, the session, and the initiator). Implementing it
    in both crates would put one safety-critical rule in two places. The harness this stream writes
    dispatches `agent_flatten` to `not interpreted until E6-3`, and the coordinator can move it back
    with one comment.
20. **`mandate-num` gets one set of exact operations.** ES-04 keeps exact arithmetic in that crate.
    E4-2's tests PR (#118) landed `Ratio` (signed, 24 places) and `Usd::ratio_to` on `main`, so this
    stream reuses both rather than adding a second ratio type, and adds only the exact comparison
    predicates, `Usd::times_ratio`, `Usd::scaled_by` (one rounding), and `Ratio::{times, complement}`.
    Stream G's claim also names `Usd × Fraction`; the coordinator sequences so that one stream lands
    each operation and the other consumes it.

21. **Shared types have one home, named here, because three streams read them.** The review of this
    brief found the same type in two briefs three times, so each is settled, and each of the two
    merged sibling briefs already carries the mechanism that adopts it: (a)
    `WorkingUniverse` lives in `mandate-domain` in stream G's richer shape
    (`Known { instruments, pinned } | Unavailable`) over `AssetId`, and G's copy — in its
    `#[doc(hidden)]` `spec_types.rs`, over `InstrumentId` and beside a `RiskState` whose field is
    `day_open_equity` — is deleted by G's first implementation PR after F's tests PR merges, which is
    G's own stated plan; (b) the
    id type is `mandate_domain::AssetId`, the schema's UUID form, for everything that comes from a
    mandate, a thesis, or a reference case, while `mandate_accounting::InstrumentId` stays the wider
    broker-facing id (`AssetId -> InstrumentId` total, the reverse fallible and needed only at a
    connector); (c) the §6.3 condition tree, `Condition::matches`, and the `ConditionTooDeep`,
    `ConditionTypeMismatch`, and `ReservedField` errors are `mandate-spec`'s alone, and stream H
    imports them in its **implementation** PR, the sequencing H's own merged brief sets (its item 5
    and Dependencies), keeping only its own `Facts` and its §6.2 oracle;
    (d) the risk snapshot's field is `day_start_equity`, the name every `risk_state` case uses, not
    `day_open_equity`; (e) the V-code enum is `Violation`, leaving `Rule` for `autonomy.rules[]`,
    which H reads by that name. F's tests PR lands all of these first so the other streams have
    something to import.

## Not done

- **Family F** (agent flatten, MC-F01 to MC-F04): stream G, per interpretation 19.
- **Families G, A, B, N** (gate, autonomy, order builder, admission and lineage): streams G, H, and
  J. The harness dispatches each to its owning story from tests PR 1, so none can silently pass.
- **`status.toml`.** No case moves to `passing` in a tests or implementation PR. The `[mandate]`
  table arrives in a status PR per DEC-77, one per implemented family group.
- **The authoring surfaces.** E10-1's compiler (the model invocation that extracts `user_stated`
  values and proposes the rest, §7), E10-2's form and YAML editor, E10-3's diff view, E10-4's
  go-live gate, and the step-up flow itself. This stream provides the functions they call and the
  provenance types they fill; it renders nothing and calls no model.
- **Applying a version at runtime** (§2.2: cancelling pending approvals, cancelling working opening
  orders, the safe point for an increasing version). `classify` decides *what* a version is; the
  executor decides *when* it applies. Stream I's runtime and E7 own that.
- **The journal side.** `mandate-spec` returns `RiskEvent` values; appending them, their canonical
  payloads, and the `causation_id` that ties `UniverseChanged` to a thesis are the executor's and
  `mandate-journal`'s (journal spec §9). No event schema is exported here.
- **The differential oracle** (`mandate-cli oracle` driven by `reference/mandate/fuzz.py`, ES-11).
  It needs a CLI surface over these functions, and it is the natural next story once the
  implementation PRs land; the property tests carry the invariants meanwhile.
- **V-018's `unusual_input`** stays rejected: E17-5 ships the input-drift detector (DEC-60,
  DEC-101).
- **No spec, schema, reference, or fixture edit.** They are the contract (ES-22). The two gaps found
  are below.

## Decisions needed

1. **The schema does not bound a decimal's scale** (`schemas/mandate.schema.json` `$defs`:
   `decimal`, `positive_decimal`, `fraction`, `open_fraction`, `unit_positive` all allow up to 28
   integer and up to 28 fractional digits, so an exact product needs up to 56). No workspace
   arithmetic type holds that, and §4.2 does not state a scale for the worst-case figures.
   **Recommendation:** bound `fraction`, `open_fraction`, and `unit_positive` to 12 fractional
   digits and the money grammars to 2, in a founder-owned schema change with its reference and
   fixture regeneration. Until then this stream takes the conservative option: the document parse
   accepts everything the schema accepts, and a rule that cannot compute exactly returns
   `out_of_range` naming the path rather than rounding a number an owner would read
   (interpretation 4). `Proposed (founder)`.
2. **The confirmation screen's display rounding** (§4.2). The four dollar figures are computed
   exactly and reported at 28 places (interpretation 7), but what the owner *sees* is compliance
   wording and presentation, which DEC-79 reserves. **Recommendation:** two decimal places, rounded
   **away from zero** for a loss figure so a displayed loss is never smaller than the computed one,
   with the exact figure kept in `MandateVersionCreated`. `Proposed (founder)`.
3. **Three backlog acceptance clauses name stale case ranges** after the v0.6 rewrite (#109):
   E10-3 says MC-C01 to MC-C35 where the family now runs to MC-C48; E6-4 says MC-R01 to MC-R15 (now
   MC-R24) and MC-L01 to MC-L09 (the goal family is MC-L01 to MC-L05); E6-2 says MC-A01 to MC-A11
   (now MC-A16) and MC-B01 to MC-B29 (now MC-B32); E6-3 says MC-G01 to MC-G13 (now MC-G16). This
   brief takes each family whole rather than the stale range. The ranges span stream H's and stream
   G's stories as well as this one's, so the correction belongs in one coordinator docs PR rather
   than in five briefs. **Recommendation:** the coordinator updates all five clauses to whole
   families in the PR that merges the last M5 brief.
4. **The stream-F and stream-G work-graph rows both claim "forced flatten (F)"**, and the stream-F
   row's parenthetical swaps the L and F family names against spec §11. Interpretation 19 gives the
   family to stream G. **Recommendation:** the coordinator corrects the two rows when it merges this
   brief; one comment reassigning family F is enough to change the tests PR.
5. **Four founder-owned entries** the tests PR adds and cannot take itself: `xtask/layers.toml` gains
   `[crates.mandate-spec]` (layer 3, `safety_critical = true`, `pure = true`,
   `allowed_external = ["thiserror"]`) and `[crates.mandate-domain]` (the same at layer 1), and
   CODEOWNERS gains a line for each crate directory. `cargo xtask layers` fails without them, so the
   tests PR cannot be green until they are in; the founder may veto either crate's placement, which
   reopens interpretation 1. `Proposed (founder)`.
6. **Shared-crate sequencing across three claims.** `mandate-num` is touched by this claim (the exact
   comparison predicates, `Usd::times_ratio`, `Usd::scaled_by`, `Ratio::{times, complement}`), by
   E4-2's #114, and by stream G's #123 (`Usd × Fraction`, which `Usd::times_ratio` subsumes). The
   coordinator decides which stream lands each operation; this brief's default is that F lands the
   set above and G consumes it.

## Commands

```bash
cargo xtask check
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo nextest run -p mandate-spec
cargo nextest run -p mandate-domain
cargo nextest run -p mandate-num
cargo test -p mandate-refcases -- --include-ignored mandate::
cargo xtask ci pending
cargo mutants -p mandate-spec
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong (the two found so far are under "Decisions needed");
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- a reference case seems wrong (propose the fix to the founder; never edit the fixture);
- anything would deviate from an accepted decision;
- the founder vetoes any of DEC-117 to DEC-126, which reopens this brief.

## Definition of done

- [ ] The 202 cases of families S, V, P, C, R, T, and L pass, plus `mandate::version` and
      `mandate::version_vector`, and no case that passed before now fails.
- [ ] Tests came first; each invariant above has a property test whose oracle is independent and was
      shown to fail on a planted bug, and the planted-bug table below is reproduced in the tests PR
      body with the test that caught each one.
- [ ] `Mandate::parse` accepts exactly what `jsonschema` accepts on all 31 MC-S cases and on the
      fuzzer's mandates (ES-22).
- [ ] New state changes emit journal events: `RiskEvent` covers every event of §5.10 this stream
      decides; appending them is the executor's.
- [ ] Docs updated: the feature map gains a "Mandate document, validation, and risk state" entry and
      its reference-case suite line; the tracker's Stories, Claims, and work-graph rows.
- [ ] `cargo xtask check` is green, including `ci pending` (every pending test fails on the stubs)
      and zero missed mutants on the implementation PRs' diffs.
- [ ] Each PR description is complete (see the PR template).

## Planted bugs

Each is broken in a throwaway implementation of the stubs, one at a time, kept out of the PR
(DEC-83), and each must be caught. The tests PR body reports the result for every row; a row that
nothing catches means the test set is incomplete, not that the bug is harmless.

| # | Bug | Expected to fail |
|---|---|---|
| 1 | `Confirm` resets breach time on the first interval the condition is false, instead of after `breach_confirm_s` of continuous falsity | `risk::a_short_recovery_does_not_restart_confirmation`, `properties::confirmation_matches_an_independent_interval_accumulator`, MC-R02 |
| 2 | A hard breach latches on the first sane quote rather than waiting min(`breach_confirm_s`, 10) s for a second | `risk::a_single_flash_print_latches_nothing`, `risk::a_flatten_hard_trigger_needs_a_second_quote`, `properties::one_bad_print_never_latches_a_limit`, MC-R04, MC-R18, MC-R19 |
| 3 | The rollover breach is measured against the new day's E0 instead of the day it started in | `risk::a_pending_breach_resolves_at_the_rollover`, `risk::a_flash_breach_before_midnight_is_discarded`, MC-R06, MC-R20 |
| 4 | Allocation scaling rounds half-even at 12 places instead of up | `properties::an_applied_allocation_change_preserves_every_ratio_and_arms_nothing` (MI-2), MC-R11, MC-R23 |
| 5 | `would_trigger_limit` tests only the soft conditions, not the 1.25× hard ones | `properties::an_applied_allocation_change_preserves_every_ratio_and_arms_nothing`, `risk::an_increase_is_rejected_while_latched` |
| 6 | The loss carry uses `C − E` instead of max(0, net contributed − E) | `risk::a_withdrawal_cannot_shrink_the_loss_carry`, `properties::the_loss_carry_is_invariant_under_a_withdraw_then_deposit_pair` (MI-14), MC-R23 |
| 7 | The effective mode takes the last restriction added rather than the strictest | `risk::the_strictest_restriction_wins`, `properties::mode_events_appear_exactly_on_a_change` (MI-6), MC-R15 |
| 8 | Equity staleness and the scale-lift timer count wall-clock seconds for equities | `risk::a_missing_mark_sets_stale_mark_and_a_sane_one_clears_it`, `risk::rungs_lift_one_at_a_time_highest_first`, MC-R09, MC-R13 |
| 9 | Reported drawdown rounds toward zero instead of half-even at 12 places | `risk::journal_order_follows_the_evaluation_order`, MC-R01 step 3 |
| 10 | After a high-water-mark reset the scale rungs all lift together instead of highest `at` first, one `scale_lift_after_s` apart | `risk::rungs_lift_one_at_a_time_highest_first`, `properties::a_reset_never_lowers_the_size_factor_immediately`, MC-R09 |
| 11 | V-020 accepts a `platform_default` on any path prefix instead of the closed §7 list with its listed value | `validate::v020_closed_platform_default_list`, `properties::a_proposed_or_unconfirmed_envelope_field_always_reports_v020` (MI-12) |
| 12 | V-022 accepts a `platform_proposed` `auto` | `validate::v022_every_auto_is_user_entered`, the V cases on `auto` provenance |
| 13 | The policy check reports the furthest violated ancestor instead of the nearest | `policy::nearest_ancestor_is_reported`, `properties::policy_violations_name_the_nearest_broken_ancestor` |
| 14 | An unset mandate `two_approver_above_usd` is treated as satisfying a policy ceiling | `policy::one_test_per_key_kind`, the P case on that key |
| 15 | Pinning is classified reducing even when the old version had no admitting model | `change::pinning_from_a_version_without_one_is_increasing`, the C case on it |
| 16 | `classify_autonomy` ignores a removed rule whose `then` was stricter than a later rule or the default | `properties::a_reducing_or_neutral_version_never_loosens_an_autonomy_decision` (MI-11), the C cases on rule removal |
| 17 | An unlisted changed path is treated as neutral instead of increasing | `change::an_unknown_path_is_increasing`, `properties::classification_is_the_join_over_changed_paths` |
| 18 | Changed paths are computed element by element inside an array, so a ladder change reports `/risk/drawdown_ladder/2/at` | `change::one_test_per_row`, MC-C01 |
| 19 | The parse accepts a decimal sent as a JSON number | `schema::one_test_per_s_case`, MC-S06, MC-S16 |
| 20 | The parse wraps a decimal in `DecStr` **without** first checking the field's `$def` pattern, so `DecStr`'s normalisation silently accepts `0.020` (as `0.02`), `007.50`, `1e3`, and `.5` | `schema::one_test_per_s_case`, `properties::a_schema_dec_is_its_own_dec_str_normal_form`, MC-S04 |
| 21 | `SchemaDec::parse` checks the `decimal` pattern but drops the `not: {const: "-0"}` beside it, so `-0` is accepted, `DecStr` normalises it to `0`, and the round trip breaks | `properties::a_schema_dec_is_its_own_dec_str_normal_form`, `schema::a_minus_zero_parameter_value_is_rejected` |
| 21b | The parse uses the `decimal` grammar for every field instead of the narrower one the schema declares, so `max_drawdown` of `8` or a negative fraction passes | `schema::one_test_per_s_case`, MC-S03, MC-S13, MC-S14 |
| 22 | The parse bounds the integer part at `DecStr`'s 29 digits rather than the schema's 28 | `properties::the_parse_agrees_with_each_fields_whole_schema_def` |
| 23 | 29 fractional digits are accepted | `schema::one_test_per_s_case`, MC-S17 |
| 24 | The parse ignores unknown members instead of rejecting them | `schema::one_test_per_s_case`, MC-S05 |
| 25 | `canonical_bytes` re-serialises from the typed fields rather than reproducing the canonical form, so a 28-place decimal loses a digit and the version hash moves | `change::the_version_vector`, `properties::equal_documents_hash_equally_and_a_one_bit_change_does_not` |
| 26 | The harness treats an unknown `expect` member as satisfied | `harness::every_owned_case_key_is_read`, `harness::a_wrong_expected_value_fails_the_case` |
| 27 | The harness dispatches a `gate` case to the risk state instead of failing with its owning story | `harness::a_family_another_stream_owns_fails_with_its_story` |
| 28 | A hard-breach lift journals an `action` instead of `reason: hard_breach_cleared`, or the two rungs share one event | `risk::a_single_flash_print_latches_nothing`, MC-R19 step 3 |
| 29 | `GoalCompleted` always carries `reason` and `then`, so the `goal_complete` input's `on_complete`-only entry does not match | `goal::one_test_per_row_of_the_goal_table`, MC-R16, MC-R17 |
