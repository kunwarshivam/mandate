# Task: E6-2 Autonomy classification and the order builder

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. This story is the agent's decision layer: model outputs and the owner's fixed weights in, a
clipped proposal and an AUTO / ASK / DENY classification out. It proposes; the risk gate decides.

## Story

- **Story:** E6-2 ([backlog](../06-backlog-v1.md#e6-agent-runtime-and-risk)), together with the
  order builder of [mandate spec §8](../../specs/mandate.md#8-signal-models-and-the-order-builder),
  which E6-2's acceptance criteria depend on (a combined score is what an autonomy rule compares).
- **Acceptance criteria (verbatim):** "As an operator, I want every proposed action classified AUTO,
  ASK, or DENY per the mandate so that autonomy matches my rules." *Accepted when:* "mandate
  reference cases MC-A01 to MC-A11 and MC-B01 to MC-B29 pass."
- **PRD / HLD / spec anchors:** mandate spec §3 (`behavior.signal_models` and their fixed weights,
  `behavior.sizing`, `protection`, `behavior.cadence` as envelope fields), §5.2 (comparisons are
  exact; reported ratios round half-even at 12 places), §5.3 (the position, order-size, and gross
  limits the proposal is clipped to), §6.1 (purposes and who assigns them), §6.2 (the evaluation
  order: risk engine, builder, gate dry run, autonomy), §6.3 (the condition language), §6.4
  (approvals: the approver count and the skip-on-timeout the classification carries), §8.1 and §8.2
  (the signal-model contract and the output fields freshness is judged on), §8.3 (`conviction_linear`:
  combine, decide, size, the accumulate clips, the minimum order), §3.1 (`accumulate` disables
  discretionary exits and clips buys; `profit_stop`); trading domain spec §5.1 and §5.3 (the v1
  order policy the proposal must be expressible in: limit orders, one side, no crossing zero), §8.2
  (the risk mark is the bid for a long), §9.1 (the gate assigns purpose and reports the first failing
  check; verdicts `allow`, `deny`, `defer`), §9.6 (the collar and the close-window rule that pace a
  discretionary exit); HLD "Agent lifecycle" and the decision path; glossary ("order builder",
  "combined score", "autonomy policy", "safe default").
- **Decisions that apply:** DEC-03 (the mandate is the contract), DEC-04 (LLMs produce opinions,
  deterministic code sizes and places orders), DEC-05 (reducing risk never needs approval), DEC-06
  (timeouts and ambiguity resolve to a safe default), DEC-32 (no short sales in v1), DEC-42, DEC-48,
  DEC-58 (autonomy), DEC-47 and DEC-60 (`conviction_linear`, fixed weights, no calibration), DEC-52
  and DEC-67 (the signal-model contract and no substitution), DEC-65 (`trim_to_target`), DEC-70 (the
  close window), DEC-72 (ADR-0001: ES-02, ES-04, ES-09, ES-13, ES-15, ES-21, ES-22), DEC-77 (brief,
  tests PR, implementation PR), DEC-79, DEC-80 (no plain comments), DEC-83 (tests PRs hold stubs
  only), DEC-85 (an uninterpreted input fails loudly), DEC-89 (exact decimals only), DEC-97 (the
  owner sets the envelope; signal models, weights, thresholds and cadence are envelope fields),
  DEC-110 (every pending test fails on the stubs), DEC-112 (this brief's PR is documentation only),
  DEC-117 to DEC-126 (the spec v0.6 rewrite this brief reads, `Proposed (founder)`; a veto reopens
  this brief), and DEC-130 (this story's interpretations, below).

## Scope

- **Reference cases that must move from pending to passing:** `mandate::MC-A01` to `MC-A16`
  (autonomy, 16 cases) and the 28 `mandate::MC-B*` builder cases other than `MC-B17`, `MC-B30`,
  `MC-B31`, and `MC-B32`. They move in a **harness-and-status PR after stream F's and stream G's
  tests PRs**, not in this story's tests PR. Three things are missing today:
  `crates/mandate-refcases` has no `mandate` module (stream F builds it); building one needs stream
  F's mandate document type; and every `B` case states a `gate_state` and expects a `gate_dry_run`
  verdict, so the harness composes propose → **gate** → `decide` and needs stream G's gate, which
  `MC-B21`'s deny and `MC-B22`'s defer make unavoidable (item 15). The `A` family needs neither the
  gate nor a quote, so if the coordinator wants the 16 autonomy cases earlier they can move on F's
  harness alone. Nothing in `crates/mandate-refcases/status.toml` changes before that PR.
- **Cases this story does not own.** `MC-B17`, `MC-B30`, `MC-B31`, and `MC-B32` assert the §5.5
  `trim_to_target` risk exit and its four guards (`rung_not_confirmed`, `holding`,
  `regular_session_only`, `below_minimum_order`). Every one of them turns on ladder state the
  builder does not hold, so they are **stream G's** (`mandate-risk`, DEC-129); see Dependencies.
- **`RC-08` and `RC-18` are not this story's either.** The tracker's M2 row reads that they wait for
  an interpretation of `propose_order`. Their `propose_order` steps supply an explicit instrument,
  side, quantity, limit price, and purpose and expect a **gate** verdict: `RC-08` and `RC-18`'s
  `generic_cash_account` variant a deny with reason code `insufficient_settled_buying_power`, and
  `RC-18`'s main path an `allow` with the reservation's effect on buying power (`449.97`). Either way
  the verdict is trading spec §9.5 buying power reached through §9.1's ordered checks, and §12 maps
  `propose_order` to `IntentProposed` and `GateDecided`. Nothing in either case combines model
  outputs or sizes a position. Both are **stream G's** (E6-3 and E6-6); this brief records the
  correction so no stream waits on the other.
- **Fixture check before any code.** Every number below is recomputed from §8.3 against the
  `two_stock_swing` base (weights `llm.news_research` 0.4 and `quant.momentum` 0.6, so W = 1;
  `entry_threshold` and `exit_threshold` 0.3; `rebalance_band` 0.05; `max_position_usd` 1500,
  `max_position_fraction` 0.2, `max_gross_exposure_usd` 2000, `max_order_usd` 1000) with
  E = 10000, bid 99.9, ask 100, increment 1, `min_order_usd` 1, and it matches the case:

  | Case | Recomputation | Expected |
  |---|---|---|
  | MC-B01 | cap = min(1500, 0.2 × 10000) = 1500; F = 0.6 × 0.8 × 0.9 + 0.4 × 0.2 × 0.5 = 0.472; M = 0; c = b = 0.472; s = 0.6 × 0.9 + 0.4 × 0.5 = 0.74; T = 0.472 × 1500 = 708; delta = 708; band = 0.05 × 1500 = 75; budget = min(708, 1000, 1500, 2000) = 708; n = trunc(708 ÷ 100, 1) = 7; 7 × 100 = 700 ≥ 75 | buy 7 at 100, order 700, AUTO by `rule:routine` |
  | MC-B02 | the same with size factor 0.5: T = 0.472 × 1500 × 0.5 = 354; n = trunc(354 ÷ 100, 1) = 3; 300 ≥ 75 | buy 3 at 100, order 300 |
  | MC-B13 | F = 0.6 × 1 × 0.65 + 0.4 × 1 × 0.6499999999999 = 0.64999999999996; c = b = s = round₁₂ = 0.65; T = 975; budget = min(975, 1000, 1500, 2000) = 975; n = 9; 900 ≥ 75 | score 0.65, buy 9, AUTO by `rule:routine`, clipped by nothing |
  | MC-B18 | position 7, MV = 7 × 99.9 = 699.3 at the bid; delta = 708 − 699.3 = 8.7 < 75 | hold, `within_rebalance_band` |
  | MC-B19 | gross 1950; budget = min(1500, 1000, 1500, min(2000, 10000) − 1950 = 50) = 50; n = trunc(50 ÷ 100, 1) = 0; 0 < 75 | hold, `below_band_after_clipping`, clipped by `limits` |
  | MC-B20 | no output is fresh: F = 0, M = W = 1, so c = 0 and b = round₁₂((0 − 1) ÷ 1) = −1, s = 0, and the hold is reported with all three | hold, `no_fresh_outputs`, b = −1 |

  MC-B20 is the shape of the whole §8.3 step 1: the same F and W give an exit conviction of 0 and a
  buy conviction of −1, so one outage can never force a sell and can never enlarge a buy (MI-10).
  MC-B13 is why the three combined figures round **before** a rule compares them: at 13 places the
  score is below `low_score`'s 0.65 and the order would ASK; at 12 it is 0.65 and the order is AUTO.

- **Invariants touched** (each gets a named test in the tests PR, with an oracle that computes the
  answer its own way):

  | Spec clause or invariant | Test |
  |---|---|
  | §6.1, §6.2 step 3 every purpose other than `open` and `increase` is AUTO by a built-in rule, whatever the rules say (MI-1, DEC-05) | `hand::every_reducing_purpose_is_auto_by_the_builtin`, `properties::no_rule_set_ever_denies_or_asks_a_reducing_purpose` |
  | §6.2 step 4 the **first** matching rule decides, in order | `hand::the_first_matching_rule_decides_and_a_later_one_is_not_read`, `properties::the_decision_matches_the_first_match_oracle` |
  | §6.2 step 4 with no match the decision is `autonomy.default` | `hand::no_rule_matches_so_the_default_decides` |
  | §6.2 step 5 the admission ceiling only **tightens**: it never turns `deny` into `ask` or `ask` into `auto` (MI-17, DEC-05) | `hand::the_admission_ceiling_turns_auto_into_ask_for_a_new_instrument`, `hand::the_admission_ceiling_never_loosens_a_deny_rule`, `properties::the_admission_ceiling_is_monotone_in_strictness` |
  | §6.2 step 5 the ceiling applies only when `new_instrument` is true | `hand::an_admission_ceiling_does_not_touch_an_order_in_a_held_instrument` |
  | §6.4 an ASK above `two_approver_above_usd` needs two approvers, `null` means one, and the comparison is strict | `hand::two_approvers_above_the_threshold_and_one_at_it`, `properties::the_approver_count_is_two_exactly_above_the_threshold` |
  | §6.4 `on_timeout` is always `skip`, so an unanswered ASK adds no risk (DEC-06) | `hand::an_ask_always_carries_skip_on_timeout` |
  | §6.3 `all`, `any`, `not`, and comparisons evaluate as written, nesting to V-017's four levels | `hand::nested_conditions_evaluate_as_written`, `properties::conditions_match_the_recursive_oracle` |
  | §6.3 decimal fields compare numerically, never as text: `0.65` and `0.650` compare equal and `9` is below `10` | `hand::a_decimal_condition_compares_numerically_not_lexically` |
  | §6.3 `in` and `not_in` on enum and string fields; V-023's type rules reject anything else at load | `hand::a_condition_whose_value_does_not_match_its_field_type_is_refused`, `properties::every_loaded_rule_is_type_correct` |
  | §6.3, V-018 `unusual_input` is refused at load until the drift detector ships | `hand::a_rule_using_unusual_input_is_refused` |
  | §6.3 the exposure fields are the order's **after** values, so order splitting cannot evade a bound | `hand::bought_today_catches_order_splitting`, `properties::position_and_gross_after_include_this_order` |
  | §8.2 fresh means `as_of ≤ now < expires_at` **and** `now − as_of ≤ max_output_age_s`; each bound is exclusive or inclusive exactly as written | `hand::a_future_as_of_is_not_fresh`, `hand::an_output_whose_as_of_is_exactly_now_is_fresh`, `hand::an_output_at_exactly_max_output_age_is_fresh_and_one_second_later_is_not`, `hand::an_output_expiring_exactly_now_is_not_fresh`, `properties::freshness_matches_the_interval_oracle` |
  | §8.1, §8.2 an output whose id, version, or content hash is not the pinned triple is ignored and counts as missing (DEC-67) | `hand::a_wrong_model_version_counts_as_missing`, `hand::an_unpinned_model_id_is_ignored` |
  | §8.2 only the latest fresh output per model counts: latest `as_of`, ties by journal order | `hand::duplicate_outputs_take_the_latest_as_of`, `hand::two_outputs_with_one_as_of_take_the_later_journal_position`, `properties::one_output_per_model_is_used_and_it_is_the_latest` |
  | §8.3 step 1 c = round₁₂(F ÷ W) with a missing model at 0, so an outage never forces a sell | `hand::a_missing_model_counts_as_zero_for_the_exit_conviction`, `properties::removing_a_fresh_output_never_lowers_the_exit_conviction_below_the_rest` |
  | §8.3 step 1 b = round₁₂((F − M) ÷ W) with a missing model fully bearish, so an outage never enlarges a buy (MI-10) | `hand::a_missing_model_counts_as_fully_bearish_for_buys`, `properties::a_missing_model_never_raises_the_buy_conviction` |
  | §8.3 step 1 W is the sum over **all** configured models, not the fresh ones | `hand::the_denominator_counts_configured_models_not_fresh_ones` |
  | §8.3 step 1 s = round₁₂(Σ fresh wᵢ · confidenceᵢ ÷ W), and all three round once, before any comparison | `hand::the_score_rounds_to_twelve_places_before_the_rule_compares_it`, `num::a_weighted_ratio_is_one_rounding_of_the_exact_quotient`, `properties::the_three_combined_figures_match_the_integer_oracle` |
  | §8.3 step 1 no fresh output holds, and still reports c, b, and s | `hand::no_fresh_output_holds_and_reports_zero_minus_one_and_zero` |
  | §8.3 step 2 an exit fires at c ≤ −`exit_threshold`, sells the whole position at the bid, and is disabled for `accumulate` (§3.1) | `hand::an_exit_at_the_threshold_sells_the_whole_position`, `hand::accumulate_never_sells_on_negative_conviction`, `hand::a_flat_position_below_the_exit_threshold_holds` |
  | §8.3 step 2, item 21 the `accumulate` check precedes the flat-position check, so a flat `accumulate` agent holds `discretionary_exits_disabled` and never `no_position` | `hand::a_flat_accumulate_agent_below_the_exit_threshold_holds_exits_disabled` |
  | Item 21 the step-5 guard is `n ≤ 0` **or** below the minimum order, so a zero-quantity buy is impossible even with a zero minimum and a zero band | `hand::a_zero_quantity_never_becomes_a_buy_at_a_zero_minimum_and_zero_band`, `properties::every_proposed_quantity_is_strictly_positive` |
  | §8.3 step 2 a buy fires at b ≥ `entry_threshold`, and between the thresholds nothing is proposed | `hand::between_the_thresholds_nothing_is_proposed`, `properties::the_three_bands_partition_the_conviction_line` |
  | §8.3 step 2 cap = min(`max_position_usd`, `max_position_fraction` × E), and T = b × cap × size factor | `hand::the_cap_is_the_lesser_of_the_dollar_and_fraction_limits`, `hand::the_ladder_size_factor_scales_the_target` |
  | §8.3 step 3 Delta = T − MV at the **risk mark** − max cost of working opening orders; a working order counts toward the target | `hand::a_working_opening_order_counts_toward_the_target`, `properties::delta_matches_the_independent_target_oracle` |
  | §8.3 step 3 hold when Delta ≤ 0 (no signal trims in v1) and when Delta < `rebalance_band` × cap | `hand::at_or_above_target_holds_and_never_sells`, `hand::a_delta_inside_the_band_holds`, `properties::a_positive_conviction_never_produces_a_sell` |
  | §8.3 step 3 the buy value is the least of Delta, `max_order_usd`, the cap headroom, and the gross headroom, at the ask, truncated to the increment | `hand::each_bound_that_can_bind_binds_in_turn`, `properties::a_proposal_never_exceeds_any_of_the_four_bounds` |
  | The cap headroom is **never** the binding bound, because `T ≤ cap` makes `delta ≤ cap − MV − working` always (a finding of the tests PR, below) | `hand::the_cap_headroom_is_never_the_binding_bound` |
  | §8.3 step 3 hold when the clipped value is below `rebalance_band` × cap (no tiny top-ups) | `hand::a_value_below_the_band_after_clipping_holds` |
  | §8.3 step 4 the three accumulate clips, each truncated to the increment, and the projected-average guard | `hand::accumulate_clipped_to_the_remaining_target_quantity`, `hand::accumulate_clipped_by_max_spend`, `hand::accumulate_clipped_by_max_avg_price`, `hand::a_projected_average_above_max_avg_price_holds`, `properties::an_accumulate_buy_never_breaks_a_goal_bound` |
  | §8.3 step 4 fees are in the clips: per-unit cost a = ask × (1 + cash fee rate) and quantity received β = 1 − asset fee rate | `hand::accumulate_with_fees_counts_the_spend_and_the_quantity_received`, `hand::a_max_avg_price_denominator_that_is_not_positive_leaves_the_clip_off` |
  | §8.3 step 5 hold below the minimum order | `hand::a_value_below_the_minimum_order_holds` |
  | §8.3, §5.3 the proposal is already clipped to the limits, so the gate's §5.3 checks cannot deny an allowed proposal for a limit the builder owns | `properties::a_proposal_never_fails_the_position_order_or_gross_limit` |
  | §6.2 step 2 a `deny` dry run skips the action and **no approval is requested** (DEC-05) | `hand::a_gate_deny_skips_and_asks_nobody`, `properties::no_denied_proposal_ever_reaches_an_approval` |
  | §6.2 step 2, §9.6 a `defer` verdict leaves nothing stored and never becomes a deny (DEC-48), and the builder never derives one: `decide` sees no session (item 15) | `hand::a_defer_verdict_stores_nothing_and_never_becomes_a_deny`, `properties::decide_returns_deferred_exactly_when_the_verdict_defers` |
  | §9.6, DEC-70 an equity discretionary exit inside the close window is proposed as a marketable limit order, and a crypto exit never is | `hand::a_discretionary_exit_in_the_close_window_is_a_marketable_limit`, `hand::a_crypto_discretionary_exit_is_a_plain_limit_in_any_session` |
  | §6.1 the builder labels a purpose from side and position but never binds it: the gate assigns it | `hand::a_buy_with_no_position_is_open_and_with_one_is_increase`, `properties::the_builder_never_proposes_a_sell_above_the_position` |
  | DEC-32 long only: no proposal is ever a short sale or crosses zero | `properties::no_proposal_crosses_zero` |
  | ES-21, DEC-89 exact arithmetic: no float, no clock, no randomness, ordered containers, identical inputs give identical proposals | `properties::identical_inputs_give_identical_proposals`, `num::the_builder_arithmetic_is_exact_or_an_error`, and the crate's lint header |
  | DEC-130 item 8 an input too wide for an exact chain refuses the proposal instead of approximating | `hand::a_weight_beyond_twelve_places_is_refused`, `hand::a_confidence_beyond_eighteen_places_is_refused`, `properties::no_input_within_the_stated_bounds_overflows` |
  | DEC-85 every input is stated: nothing defaults silently | `hand::an_absent_prior_fill_flag_is_not_inferred_from_the_position` |

  **Oracles.** `crates/mandate-builder/tests/properties.rs` holds three implementations that share no
  code with the crate: a sign-and-magnitude decimal on `i128` with its own alignment, multiplication,
  comparison, half-even rounding, and truncation, which recomputes every combined and every sizing
  figure; a naive autonomy walk that re-reads the rule list from the start for every action and
  compares condition values by parsing their canonical text; and the §8.3 chain as a straight line of
  those operations, with the four bounds compared rather than minimised through the crate's own type.
  Every sizing property runs through one helper that first asserts the oracle and the crate agree on
  the **action kind**, so no property can pass on a hold the crate returned for the wrong reason.

  **Generated scales are coarse on purpose:** weights, convictions, and confidences at up to 6
  fractional places, equities and prices at up to 2. An `i128` oracle cannot carry the 48-place
  products an 18-place confidence would make, and an arbitrary-precision oracle would end up reusing
  `mandate-num`'s own 256-bit arithmetic and prove nothing. The fine-scale edges are pinned by hand
  instead: `MC-B13`'s 13-place confidence, the 12-place and 18-place refusals, and the `mandate-num`
  digit tests. E4-1 made the same trade for its 18-place root.

  **Planted bugs.** Thirty-two, each broken alone in a throwaway implementation of the stubs (kept
  out of the PR, DEC-83), run, and reverted. Every one is caught. Writing that implementation is also
  what found the two defects the tests PR records below, and the pass itself found a third: no test
  covered §8.2's **inclusive** lower bound, so an implementation reading `as_of ≤ now` as strict
  passed every other freshness case. `hand::an_output_whose_as_of_is_exactly_now_is_fresh` closes it,
  and the freshness generator now draws the boundary offsets explicitly rather than hoping a uniform
  range lands on zero.

  | Planted bug | Caught by |
  |---|---|
  | The buy conviction uses F ÷ W instead of (F − M) ÷ W, so a model outage enlarges a buy | `hand::a_missing_model_counts_as_fully_bearish_for_buys`, `hand::no_fresh_output_holds_and_reports_zero_minus_one_and_zero`, `properties::a_missing_model_never_raises_the_buy_conviction` |
  | The exit conviction subtracts M too, so an outage forces a sell | `hand::a_missing_model_counts_as_zero_for_the_exit_conviction`, `properties::removing_a_fresh_output_never_lowers_the_exit_conviction_below_the_rest` |
  | W sums the fresh models' weights rather than all configured weights | `hand::the_denominator_counts_configured_models_not_fresh_ones`, `properties::the_three_combined_figures_match_the_integer_oracle` |
  | The score is compared before rounding to 12 places | `hand::the_score_rounds_to_twelve_places_before_the_rule_compares_it` (MC-B13) |
  | Rounding is half-up instead of half-even | `num::a_weighted_ratio_is_one_rounding_of_the_exact_quotient`, `properties::the_three_combined_figures_match_the_integer_oracle` |
  | Freshness uses `as_of < now` instead of `as_of ≤ now`, or `now ≤ expires_at` instead of `now < expires_at` | `hand::an_output_expiring_exactly_now_is_not_fresh`, `hand::an_output_at_exactly_max_output_age_is_fresh_and_one_second_later_is_not`, `properties::freshness_matches_the_interval_oracle` |
  | The latest output per model breaks ties by the earlier journal position | `hand::two_outputs_with_one_as_of_take_the_later_journal_position` |
  | The content hash is not compared, only the id and version | `hand::an_unpinned_model_id_is_ignored` (variant asserting a hash mismatch counts as missing) |
  | The admission ceiling takes the **looser** of the two decisions | `hand::the_admission_ceiling_never_loosens_a_deny_rule`, `properties::the_admission_ceiling_is_monotone_in_strictness` |
  | The admission ceiling is applied before the rules rather than after | `hand::the_admission_ceiling_turns_auto_into_ask_for_a_new_instrument` (by reading `by`, which must be `admission_ceiling` only when the ceiling changed the decision) |
  | The rule walk takes the **last** match instead of the first | `hand::the_first_matching_rule_decides_and_a_later_one_is_not_read`, `properties::the_decision_matches_the_first_match_oracle` |
  | A reducing purpose is classified by the rules instead of the built-in AUTO | `hand::every_reducing_purpose_is_auto_by_the_builtin`, `properties::no_rule_set_ever_denies_or_asks_a_reducing_purpose` |
  | `two_approver_above_usd` compares with `≥` instead of `>` | `hand::two_approvers_above_the_threshold_and_one_at_it`, `properties::the_approver_count_is_two_exactly_above_the_threshold` |
  | Decimal condition values compare as text | `hand::a_decimal_condition_compares_numerically_not_lexically` |
  | MV uses the ask rather than the risk mark, so the position looks larger and a buy is smaller — or the reverse | `hand::a_working_opening_order_counts_toward_the_target`, `properties::delta_matches_the_independent_target_oracle` (MC-B18's 8.7 delta) |
  | The working opening orders' cost is left out of Delta | `hand::a_working_opening_order_counts_toward_the_target`, MC-B15 |
  | The rebalance band is compared against Delta only, not against the value after clipping | `hand::a_value_below_the_band_after_clipping_holds` (MC-B19) |
  | One of the four bounds is dropped from the minimum | `hand::each_bound_that_can_bind_binds_in_turn`, `properties::a_proposal_never_exceeds_any_of_the_four_bounds` |
  | The share count rounds instead of truncating to the increment | `num::shares_are_truncated_toward_zero_at_the_increment`, `properties::a_proposal_never_exceeds_any_of_the_four_bounds` |
  | The step-5 guard drops `n ≤ 0` and keeps only the minimum-order comparison | `hand::a_zero_quantity_never_becomes_a_buy_at_a_zero_minimum_and_zero_band`, `properties::every_proposed_quantity_is_strictly_positive` |
  | The flat-position check is tried before the `accumulate` check in step 2 | `hand::a_flat_accumulate_agent_below_the_exit_threshold_holds_exits_disabled` |
  | The accumulate remaining-quantity clip divides by 1 instead of β, so the asset fee is ignored | `hand::accumulate_with_fees_counts_the_spend_and_the_quantity_received`, `properties::an_accumulate_buy_never_breaks_a_goal_bound` |
  | The projected-average guard is skipped when the clip did not bind | `hand::a_projected_average_above_max_avg_price_holds` |
  | An `accumulate` goal still takes the exit branch | `hand::accumulate_never_sells_on_negative_conviction` (MC-B29) |
  | A `deny` dry run still requests an approval | `hand::a_gate_deny_skips_and_asks_nobody`, `properties::no_denied_proposal_ever_reaches_an_approval` (MC-B21) |
  | A `defer` verdict is turned into a deny or a skip | `hand::a_defer_verdict_stores_nothing_and_never_becomes_a_deny`, `properties::decide_returns_deferred_exactly_when_the_verdict_defers` (MC-B22) |
  | An equity exit in the close window is proposed as a plain limit, or a crypto exit as a marketable one | `hand::a_discretionary_exit_in_the_close_window_is_a_marketable_limit`, `hand::a_crypto_discretionary_exit_is_a_plain_limit_in_any_session` |

- **Crates in scope.** New `mandate-builder` (layer 5 per ADR-0001 ES-02 and `xtask/layers.toml`'s
  plan; `pure = true`; `safety_critical = true`, DEC-130 item 1; `allowed_external = ["thiserror"]`;
  a CODEOWNERS line beside the other safety-critical crates). It depends on `mandate-num`,
  `mandate-time` (`UtcNanos`, for freshness), and `mandate-accounting` (`InstrumentId`, `Side`,
  `AssetClass`). `mandate-num` gains the exact arithmetic of DEC-130 item 7 under a shared-crate
  claim, because ES-04 keeps exact arithmetic there.
- **Crates out of scope.** `mandate-risk` (stream G: the gate, the ladder, `trim_to_target`),
  `mandate-spec` (stream F: the document, validation, policy, classification, risk state, and the
  mandate harness), `mandate-refcases` (its `mandate` module is stream F's; this story adds the `A`
  and `B` families to it in a later PR), `mandate-accounting` and `mandate-sim` (read only),
  `mandate-cli`.
- **New dependencies allowed:** none. `thiserror` and `proptest` are registered; the
  `docs/dependencies.md` "Used by" cells gain `mandate-builder` in the tests PR.
- **Safety-critical:** yes (DEC-130 item 1). Autonomy is the boundary between what an agent may do
  alone and what the owner must approve, and the builder is what sizes a position. DEC-77 sequence:
  tests PR (crate, stubs, pending tests), implementation PR (test files change only by deleting
  `#[ignore = "pending E6-2"]` lines), then the harness-and-status PR once stream F's harness and
  stream G's gate exist.
- **Size budget:** 400 non-generated lines per PR (ES-13). The tests PR will exceed it for test
  code; it states its split, and the autonomy tests and the builder tests are separable if the
  coordinator wants two.

## Data shapes

The caller's view, written before any logic. These are the tests PR's stubs, in `mandate-builder`.
Every field is required: nothing is inferred from another field (DEC-85).

```rust
pub enum Purpose { Open, Increase, DiscretionaryExit, OwnerExit, RiskExit, Protective }
pub enum Session { PreMarket, Regular, AfterHours, Crypto }
pub enum Decision { Auto, Ask, Deny }

pub struct RuleId(String);
pub enum Field {
    Purpose, OrderUsd, CombinedScore, Instrument, AssetClass, Session,
    FirstTradeInInstrument, NewInstrument, ThesisConfidence, Drawdown, DailyPnlFraction,
    PositionUsdAfter, GrossUsdAfter, BoughtTodayUsd, PositionPnlFraction, UnusualInput,
}
pub enum Op { Eq, Ne, Gt, Gte, Lt, Lte, In, NotIn }
pub enum Condition {
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    Compare { field: Field, op: Op, value: Value },
}
pub enum Value { Flag(bool), Text(String), Money(Usd), Unit(Unit), Signed(Signed), Set(BTreeSet<String>) }
/// A value beyond the places its field's type holds is refused here, not at evaluation (item 8).
pub struct Rule { pub id: RuleId, pub when: Condition, pub then: Decision }
pub struct AutonomyPolicy {
    pub rules: Vec<Rule>,
    pub default: Decision,
    pub admission: Decision,
    pub two_approver_above_usd: Option<Usd>,
}

pub struct ActionContext {
    pub purpose: Purpose,
    pub order_usd: Usd,
    pub combined_score: Unit,
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub session: Session,
    pub first_trade_in_instrument: bool,
    pub new_instrument: bool,
    pub thesis_confidence: Unit,
    pub drawdown: Unit,
    pub daily_pnl_fraction: Signed,
    pub position_usd_after: Usd,
    pub gross_usd_after: Usd,
    pub bought_today_usd: Usd,
    pub position_pnl_fraction: Signed,
}

pub enum DecidedBy { BuiltinRiskReducing, Rule(RuleId), Default, AdmissionCeiling }
pub struct Approval { pub approvers_required: NonZeroU8, pub on_timeout: OnTimeout }
pub enum OnTimeout { Skip }
pub struct Autonomy { pub decision: Decision, pub by: DecidedBy, pub approval: Option<Approval> }

/// Mandate plus proposed action plus context to AUTO, ASK, or DENY, with the rule that decided.
pub fn classify(policy: &AutonomyPolicy, action: &ActionContext) -> Result<Autonomy, BuilderError>;
```

```rust
pub struct ModelId(String);
pub struct SignalModel {
    pub id: ModelId,
    pub version: ModelVersion,
    pub content_hash: ContentHash,
    pub weight: SizeFraction,
    pub max_output_age_s: u32,
}
pub enum Direction { Long }
pub struct ModelOutput {
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub content_hash: ContentHash,
    pub instrument: InstrumentId,
    pub as_of: UtcNanos,
    pub expires_at: UtcNanos,
    pub direction: Direction,
    pub conviction: Conviction,
    pub confidence: Unit,
}
/// §8.3 step 1. `exit_conviction`, `buy_conviction`, and `score` are each one `round₁₂`.
pub struct Combined {
    pub outputs_used: BTreeSet<ModelId>,
    pub exit_conviction: Conviction,
    pub buy_conviction: Conviction,
    pub score: Unit,
}
pub fn combine(
    models: &[SignalModel],
    outputs: &[ModelOutput],
    now: UtcNanos,
) -> Result<Combined, BuilderError>;

pub enum SizingMethod { ConvictionLinear }
pub struct Sizing {
    pub method: SizingMethod,
    pub entry_threshold: SizeFraction,
    pub exit_threshold: SizeFraction,
    pub rebalance_band: SizeFraction,
}
pub struct Limits {
    pub max_position_usd: Usd,
    pub max_position_fraction: SizeFraction,
    pub max_order_usd: Usd,
    pub max_gross_exposure_usd: Usd,
}
pub struct AccumulateGoal {
    pub instrument: InstrumentId,
    pub target_qty: Qty,
    pub max_avg_price: Option<Price>,
    pub max_spend_usd: Usd,
}
pub enum GoalKind { Continuous, ProfitStop, Accumulate(AccumulateGoal) }
/// The §8 view of the mandate: what the builder reads, not the whole document (stream F's).
pub struct BuilderMandate {
    pub models: Vec<SignalModel>,
    pub sizing: Sizing,
    pub limits: Limits,
    pub goal: GoalKind,
}

pub struct AccountSnapshot {
    pub agent_equity: Usd,
    pub position_qty: Qty,
    pub cost_basis: CostBasis,
    pub risk_mark: MarkPrice,
    pub gross_usd: Usd,
    pub working_opening_cost: Usd,
    pub goal_spent_usd: Usd,
}
pub struct Market {
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub session: Session,
    pub in_close_window: bool,
    pub bid: Price,
    pub ask: Price,
    pub increment: ShareIncrement,
    pub min_order_usd: Usd,
    pub fee_rate_cash: FeeRate,
    pub fee_rate_asset: FeeRate,
}
/// Risk state and thesis facts the builder reads but never computes (§5.2, §5.5, §8.4).
pub struct RiskContext {
    pub size_factor: SizeFraction,
    pub drawdown: Unit,
    pub daily_pnl_fraction: Signed,
    pub position_pnl_fraction: Signed,
    pub bought_today_usd: Usd,
    pub has_prior_fill: bool,
    pub new_instrument: bool,
    pub thesis_confidence: Unit,
}

/// Declaration order is not evaluation order; item 21 fixes the order the reasons are reached in.
pub enum HoldReason {
    NoFreshOutputs, NoPosition, DiscretionaryExitsDisabled, BetweenThresholds,
    AtOrAboveTarget, WithinRebalanceBand, BelowBandAfterClipping,
    WouldExceedMaxAvgPrice, BelowMinimumAfterClipping,
}
pub enum Clip { Limits, Goal }
pub enum OrderShape { Limit, MarketableLimit }
/// The reported figures of §8.3, exact. `cap` has no stated rounding (§5.2: comparisons are
/// exact); `target_value` and `delta` are reported at 12 places by the caller that journals them.
pub struct Sizes {
    pub cap: UsdExact,
    pub current_mv: Usd,
    pub target_value: Option<UsdExact>,
    pub delta: Option<UsdExact>,
}
pub enum Action {
    Hold { reason: HoldReason },
    Sell { purpose: Purpose, qty: Qty, limit_price: Price, order_usd: Usd, shape: OrderShape },
    Buy { purpose: Purpose, qty: Qty, limit_price: Price, order_usd: Usd, action: ActionContext },
}
pub struct Proposal {
    pub action: Action,
    pub combined: Combined,
    pub sizes: Sizes,
    pub clipped_by: BTreeSet<Clip>,
}
/// Signal outputs plus the mandate plus an account snapshot to a proposed order or no action,
/// with the combined score as an exact decimal. Pure: no clock, no I/O, no state between calls.
pub fn propose(
    mandate: &BuilderMandate,
    account: &AccountSnapshot,
    market: &Market,
    risk: &RiskContext,
    outputs: &[ModelOutput],
    now: UtcNanos,
) -> Result<Proposal, BuilderError>;

pub enum GateVerdict { Allow, Deny, Defer }
/// §6.2 step 2 then steps 3 to 6: the gate's verdict arrives as a **value**, so the builder never
/// calls the gate. A `Deny` is skipped and journaled and no approval is requested (DEC-05); a
/// `Defer` is the gate's, never derived here — `decide` takes no `Market` and so cannot see the
/// session (item 15).
pub enum Outcome { Skipped, Deferred, Classified(Autonomy) }
pub fn decide(
    policy: &AutonomyPolicy,
    proposal: &Proposal,
    verdict: GateVerdict,
) -> Result<Outcome, BuilderError>;
```

`BuilderError` names one cause each, with a stable `code()` (ES-09): `Unimplemented` (the tests PR's
stubs only), `ConditionTooDeep`, `ConditionTypeMismatch`, `ReservedField` (`unusual_input`, V-018),
`DuplicateRuleId`, `NoSignalModels`, `WeightSumZero`, `CrossedQuote` (item 17),
`AccumulateInstrumentMismatch`, and the numeric and time errors it wraps, whose codes
(`too_precise`, `overflow`, `not_canonical`) are how an input too wide to size exactly refuses the
proposal (item 8).

Three refusals an earlier draft of this brief carried are **not** in that list, because
`reference/mandate/ref.py` does not raise them and none needs an error at all. An output whose
`expires_at` is at or before its `as_of` is simply never fresh, which is the freshness test's own
answer; `Direction` has one variant, `Long`, and `SizingMethod` one, `ConvictionLinear`, so a
direction or a method v1 does not support is unrepresentable rather than rejected — the trust
ladder's first rung. `CrossedQuote` stays, and item 17 records it as the one refusal ref.py would not
produce.

## Exact arithmetic (DEC-89, ES-04, ES-21)

§5.2 says comparisons are exact and only reported ratios round, and §8.3 states exactly three
roundings (the `round₁₂` of step 1). So every other value in the chain is exact, and the crate's
job is to stay inside a fixed-width integer or say it cannot.

New in `mandate-num` (shared-crate claim on this story):

| Addition | Why |
|---|---|
| `SizeFraction`: non-negative, at most 1, at most **12** fractional places | the mandate's sizing and limit fractions — a signal model's `weight`, `max_position_fraction`, `entry_threshold`, `exit_threshold`, `rebalance_band`, and the ladder size factor. Twelve places is what keeps the exact sizing chain inside 256 bits (the budget below), so the bound is load-bearing rather than cosmetic. `Fraction` is not reused: its 9-place ceiling is part of the fill model's contract (it multiplies a 9-place volume, E4-1), and widening it would let a volume cap carry 12 places |
| `Unit`: non-negative, at most 1, at most **18** fractional places | a model output's `confidence`, the combined score, `thesis_confidence`, `drawdown`, and the unit-typed condition values. Eighteen, not twelve, because `MC-B13` already carries a 13-place confidence, and a confidence enters only the combine step, where the budget affords it |
| `Conviction`: signed, in [−1, 1], at most **18** places | a model output's conviction and the two combined convictions. The two combined values are `round₁₂` results, so they use 12 of the 18; their bound is a consequence of the formula (\|F\| ≤ W and F − M ≥ −W) and is therefore checked, not assumed |
| `Signed`: signed, at most **18** places | `daily_pnl_fraction` and `position_pnl_fraction`, which §5.2 defines at 12 places, and their condition values, which the schema does not bound |
| `Unit::weighted_ratio(&[(SizeFraction, Unit)], &[SizeFraction])` and `Conviction::weighted_ratio(&[(SizeFraction, Conviction, Unit)], &[SizeFraction], &[SizeFraction])` | the three `round₁₂` quotients of step 1 as **one** formula each on 256-bit intermediates: the numerator terms, the weights counted as −1 (the missing models of the buy conviction), and the denominator terms, summed exactly and divided once |
| `UsdExact`: a signed USD amount carried exactly on 256-bit intermediates, wider than `Usd`, converted to `Usd` only by an explicit rounding | cap, the target, and Delta. `Usd` holds 28 places on a 96-bit significand and `Ratio` 24, while cap alone reaches 33 (a 12-place fraction times an equity that holds 21, from a 9-place quantity at a 12-place mark), so neither existing type can carry them and rounding them would be a rounding the spec does not state |
| `UsdExact::{checked_add, checked_sub, min, times_size_fraction, times_conviction, is_positive, round}` and `UsdExact::shares_at(Price, ShareIncrement)`, the wide counterpart of the existing `Usd::shares_at` | the step 3 chain and its one truncation to the increment |

**Digit budget**, with `SizeFraction` at 12 places, `Unit` and `Conviction` at 18, marks at 12,
quantities and prices at 9 (trading spec §2.1), and a value ceiling of 10¹² USD. 256 bits holds about
10⁷⁷.

- **Combine (§8.3 step 1).** A numerator term is weight (12) × conviction (18) × confidence (18) = 48
  places, at most 10 of them, so the sum is under 10⁴⁹; W holds 12 places, under 10¹³. The one
  `round₁₂` division scales the denominator by 10³⁶, under 10⁴⁹. The score's terms are smaller still
  (12 + 18 = 30 places).
- **Size (§8.3 steps 2 and 3).** E holds 21 places (a 9-place quantity at a 12-place mark);
  cap = fraction × E holds 33, under 10⁴⁵; the band = `rebalance_band` × cap holds 45, under 10⁵⁷;
  T = b × cap × factor holds 57, under 10⁶⁹; Delta and each of the four clips hold 57. The one
  division, `shares_at`, is an integer division of that 10⁶⁹ value by a 9-place price scaled to 10⁵⁴.
- **Accumulate (§8.3 step 4).** The widest is the spend clip: a 28-place remainder over a 21-place
  per-unit cost, truncated to a 9-place increment, under 10⁷⁰. The projected-average guard is a
  cross-multiplied comparison at 30 places, so it takes no quotient at all.

This is why `SizeFraction` stops at 12: at 18 places the target would reach 69 and `shares_at` would
pass 10⁷⁷. `properties::no_input_within_the_stated_bounds_overflows`
is the test that keeps this true; anything outside returns `overflow` rather than an approximation.

## Interpretations (recorded as DEC-130)

1. **Crate and criticality.** `mandate-builder` is safety-critical and pure, at layer 5, and holds
   both the autonomy classification and the order builder, because a rule reads the combined score
   the builder produces and separating them would put one crate's output in the other's test
   fixtures. `xtask/layers.toml` and CODEOWNERS are founder-owned, so those two entries are the one
   item this decision cannot take alone.
2. **The builder proposes; the gate decides; never the reverse.** `mandate-builder` does not depend
   on `mandate-risk`: `decide` takes the dry run's verdict as a **value**. So no defect in the
   builder can turn a gate verdict into an allow, and the builder cannot be the reason a limit is
   not enforced (AGENTS.md rule 1). The gate assigns purpose from side and position (§6.1, trading
   §9.1); the builder's `Purpose` on a proposal is a label the gate re-derives and is never binding.
3. **The `trim_to_target` risk exit is not the builder's.** §6.2 step 1 has the risk engine propose
   it first; its quantity, its four guards, and the ladder state they read are §5.5, which is
   `mandate-risk`. The runtime (stream I) asks the risk engine first and calls the builder only when
   there is no trim to place, so the builder can never suppress a risk exit — a stronger property
   than sequencing the two inside the builder. `MC-B17`, `MC-B30`, `MC-B31`, and `MC-B32` therefore
   belong to stream G.
4. **`RC-08` and `RC-18` wait on the gate, not on the builder** (see Scope). Recorded here so the
   tracker's M2 row and this stream do not each wait for the other.
5. **The mandate arrives as a narrow view.** `propose` and `classify` take `BuilderMandate` and
   `AutonomyPolicy`, the §8 and §6 fields only, not the whole document. This keeps the story
   testable before stream F's `mandate-spec` exists, keeps the crate's inputs reviewable, and means
   `mandate-spec` later supplies a conversion without changing either signature. `Session` and
   `Purpose` are defined here for now; both belong in `mandate-domain` once stream F creates it,
   beside the `AssetClass` move the tracker already records.
6. **Market value is at the risk mark, not at the quote.** §8.3 step 3 says "MV (at the risk mark)",
   and trading spec §8.2 makes the risk mark the bid for a long, which is why the reference cases
   supply the bid and the mark as the same number. The crate takes `risk_mark` as its own input, so
   a stale or non-sane mark cannot be silently replaced by a quote, and MC-B18's 699.3 is the mark
   times the quantity.
7. **Exactness.** As above: three roundings, everything else exact, `SizeFraction` at 12 places and
   `Unit` and `Conviction` at 18 because `MC-B13` carries a 13-place confidence, `UsdExact` for the
   wide intermediates, and the digit budget stated per step and tested.
8. **An input too wide refuses the order.** A weight, fraction, threshold, conviction, confidence,
   or condition value beyond the places its type holds is `too_precise`, and an intermediate beyond
   256 bits is `overflow`; either way the builder proposes nothing. Refusing to propose adds no risk, which is what rule 3 and
   DEC-06 require of an ambiguous input, whereas an approximated size is an order nobody specified.
   The schema admits values these types cannot hold; see Decisions needed.
9. **Combined figures are computed before the hold is reported.** §8.3 step 1's "no fresh outputs:
   hold" still reports c = 0, b = −1, and s = 0 (MC-B20), because the three figures are what the
   journal and an approval screen show, and an outage must be visible as an outage rather than as a
   blank.
10. **Ties among outputs break by journal order.** §8.2's "ties by journal sequence" is implemented
    as the index in the `outputs` slice, which the caller supplies in `seq` order; the crate does not
    read a sequence number it cannot verify. `outputs_used` is a `BTreeSet<ModelId>`, so it is
    sorted, as the cases expect, and two runs cannot differ by iteration order (ES-21).
11. **A pinned triple is compared whole.** An output is ignored unless its id, version, **and**
    content hash all equal the pinned values (§8.1, DEC-67). An ignored output counts as missing, so
    it lowers the buy conviction and never raises it.
12. **Condition values are typed at load, not at evaluation.** `Value` is parsed into the field's own
    type when the policy is built, so V-023's type rules and V-017's depth limit are checked once and
    a comparison is a numeric comparison of two exact decimals, never a string comparison. A rule
    naming `unusual_input` is refused at load (V-018), so the field cannot be reached at all.
13. **`by` names what decided.** `DecidedBy::AdmissionCeiling` is reported only when the ceiling
    actually changed the decision (MC-A12 and MC-A14), and the matching rule's id otherwise
    (MC-A13's `rule:routine` under `admission: auto`, MC-A15's `rule:no_new`). The ceiling is applied
    after the rules and only tightens, so the owner's `deny` survives an `auto` admission setting.
14. **`skipped` and `deferred` are outcomes, not decisions.** The reference cases carry them in the
    `autonomy` field; in Rust they are `Outcome::Skipped` and `Outcome::Deferred`, so a gate verdict
    cannot be mistaken for an autonomy decision in a `match`, and the harness maps the two shapes.
15. **A discretionary exit is paced, never denied, and the two pacing rules have different owners.**
    The **session** rule is the gate's: an equity exit outside the regular session is verdict
    `defer` (trading spec §9.6, §9.1, DEC-48), and `decide` maps `GateVerdict::Defer` to
    `Outcome::Deferred` with nothing stored (§6.2 step 2). The builder does not derive it — `decide`
    takes no `Market`, so it cannot see the session, which is item 2 holding rather than an omission.
    `reference/mandate/ref.py` composes the two inside its `builder`, which is why MC-B22 reads as a
    builder case; in Rust the harness composes propose → gate → `decide`, and the gate is stream G's.
    The **close-window** rule is the builder's, because it changes the order it proposes rather than
    the verdict: `propose` has `Market`, and an equity discretionary exit with `in_close_window` set
    carries `OrderShape::MarketableLimit` (DEC-70). Crypto has no regular session, so
    `Session::Crypto` is never deferred.
16. **No approval is requested for an action the gate would deny**, and `decide` is the only path to
    an `Autonomy`, so that rule holds by construction rather than by discipline (§6.2 step 2).
17. **Limit prices are the quote, not a collared or tick-rounded price.** §8.3 prices a buy at the
    ask and an exit at the bid; the collar (trading §9.6) and the tick (§2.1) are the gate's and the
    executor's, and applying them here would price an order twice. A crossed quote (bid > ask) is
    `CrossedQuote`, a refusal `reference/mandate/ref.py` does not raise and no committed case
    reaches, taken because sizing off a crossed quote prices an order against a market that does not
    exist; it is the one refusal in `BuilderError` that ref.py would not produce.
18. **Protection is not the builder's.** `protection` is an envelope field the executor uses to place
    the bracket or crypto stop-limit (trading §5.4); the builder proposes the entry only.
19. **No defaults.** Every field the reference cases treat as optional (`size_factor`, `gross_usd`,
    `bought_today_usd`, `has_prior_fill`, `goal_spent_usd`, `cost_basis`) is required in Rust; in
    particular `has_prior_fill` is not inferred from the position, because a re-entry after a round
    trip is not a first trade (MC-B25) and inferring it would mislabel exactly that case (DEC-85).
20. **Journal events: none.** The crate is a pure function. The runtime appends
    `ModelOutputRecorded` (with the `ignored` reason for an output this crate rejected),
    `IntentProposed`, `GateDecided`, and the approval events; the skipped and deferred journaling of
    §6.2 step 2 is the runtime's (stream I).
21. **Two orderings the spec leaves implicit, fixed here.** In step 2 the `accumulate` check comes
    **before** the flat-position check, as `ref.py` has it, so a flat `accumulate` agent under
    c ≤ −`exit_threshold` holds `discretionary_exits_disabled`, not `no_position`: the goal disabled
    the exit, and reporting the position instead would read as though a position would have been
    sold. And the step-5 guard is `n ≤ 0` **or** the value below the minimum order, not the minimum
    alone, so a zero-quantity buy is impossible even where `min_order_usd` and `rebalance_band` are
    both zero — the one thing standing between a fully clipped budget and an order for nothing.

## What the tests PR settled, and what it found

Shapes the brief left open, fixed by the tests PR and reported in its body. None changes a rule; each
is a Rust detail or a finding about the spec's own arithmetic.

| Settled | Why |
|---|---|
| `AutonomyPolicy::new` is **implemented**, not stubbed: V-017's depth, V-018's reserved field, V-023's types, and unique rule ids are load-time checks | The `mandate-num` precedent (`Ratio::parse` real, the formulas stubbed): a constructor that validates is not logic under test, and implementing it makes the pending tests fail at their own assertion rather than at fixture construction. Its five tests therefore pass in the tests PR |
| `Field::kind()` and a public `Kind` | V-023's table has to live somewhere, and naming it lets the property test check the invariant "every rule that loads is type-correct" without re-deriving the table |
| `Outcome::NotProposed` | A hold has no order to gate, and `decide` is total rather than returning an `Option`, so no caller can forget the case |
| `Action::Buy` boxes its `ActionContext` | The variant is otherwise 200 bytes larger than `Hold`, which `clippy::large_enum_variant` rejects. No semantic change |
| `Conviction::of_fraction` | §8.3 step 2 compares a combined conviction against `entry_threshold` and `exit_threshold`, which are `SizeFraction`s; the conversion is what makes that comparison typed |
| `UsdExact::round` refuses a `scale` beyond the 28 places `Usd` stores | Clamping it silently would hand a caller fewer places than it asked for, in the one path that reports a figure to an owner |
| `UsdExact::truncated_quotient` returns a **signed** quantity | A bound already exceeded leaves a negative budget, which §8.3 step 5 holds on; a non-negative type would have to error or clamp, and both hide the case |
| `UnsupportedSizingMethod` dropped from `BuilderError` | The same rung-1 argument as `Direction`: `SizingMethod` has one variant, so there is nothing to reject, and `ref.py` does not check the method either. Round 1's item 4 asked for exactly this consistency |

**Two findings about §8.3 itself**, from writing the throwaway implementation:

1. **The cap headroom can never be the binding bound.** Step 3's third bound is
   `cap − MV − working`, and the target is `buy_conviction × cap × size factor` with both factors at
   most one, so `T ≤ cap` and therefore `delta ≤ cap − MV − working` always, with equality only at
   full conviction and no active rung. The bound is belt and braces rather than a clip, and a proposal
   cut to exactly the cap headroom is cut by the delta and reports no clip at all
   (`hand::the_cap_headroom_is_never_the_binding_bound`). Nothing in the spec is wrong; the brief's
   first reading that all four bounds bind was.
2. **The rebalance band is checked before the goal clips, the minimum order after.** A value above the
   band can therefore be clipped by an `accumulate` bound to well below the band and still be
   proposed, as long as it clears the minimum order — which is what `MC-B26` and the spend-clip case
   do. That asymmetry is the spec's own order of operations and is now pinned by tests rather than
   inferred.

## Dependencies

- **Stream F (`mandate-spec`, DEC-128).** Needed for the mandate document type and for
  `crates/mandate-refcases`'s `mandate` module. Not needed for this story's tests PR: item 5's narrow
  views are defined here, and the tests are hand-built inputs plus generated ones. What waits for F
  is the **harness-and-status PR** that wires the `A` and `B` families and moves them in
  `status.toml`, and the `B` family also waits on stream G's gate (Scope). If F's brief places `Session`, `Purpose`, `AssetClass`, or `InstrumentId` in
  `mandate-domain`, this crate takes them from there in the implementation PR instead of defining
  them, which changes no signature in this brief.
- **Stream G (`mandate-risk`, DEC-129).** Owns the gate whose verdict `decide` consumes — including
  the `defer` of item 15 — the `trim_to_target` proposal and its guards (item 3), and `RC-08` and
  `RC-18` (item 4). No code dependency in either direction: the verdict is a value. `GateVerdict` is
  defined here so the tests PR does not wait for G; if G defines the same enum, the implementation PR
  takes G's and deletes this one. The `B` family's harness does need G's gate, as Scope says.
- **Stream J (E17, DEC-132).** Supplies `new_instrument` and `thesis_confidence` (§8.4, §8.5) as
  inputs. §8.5 admission is J's, not this story's; this crate only reads the two flags.
- **Stream I (E6-1, DEC-131).** The runtime that sequences risk engine → builder → gate → autonomy
  (items 3 and 16) and does the journaling of item 20.
- **`mandate-num` shared-crate claim.** The additions of the arithmetic section. Stream E's E4-2
  tests PR (#118, merged) already added `Ratio` with `RATIO_SCALE = 24`, its statistics operations,
  `Usd::ratio_to`, `Usd::shares_at`, and `Price::on_tick` as stubs (DEC-127 item 14), which E4-2's
  implementation PR fills. The two sets do not overlap: `Ratio`'s 24-place bound is exactly why this
  story adds `UsdExact` rather than widening it, and `UsdExact::shares_at` mirrors the existing
  `Usd::shares_at` for a wide numerator. This story's tests PR must not edit E4-2's stubs, so it
  lands after that implementation PR or the coordinator sequences the two.

## Decisions needed

1. **The schema bounds no decimal's scale, so a valid mandate can be unsizeable.**
   `schemas/mandate.schema.json`'s `$defs/decimal`, `fraction`, `unit_positive`, and
   `positive_decimal` admit up to 28 integer and 27 fractional digits, and mandate spec §8.2 gives no
   scale for a model output's `conviction` or `confidence` (`MC-B13` already carries a 13-place
   confidence). ES-04's typed values hold a 96-bit significand at up to 28 places, and no
   fixed-width intermediate can carry an exact `b × cap × factor` at 27-place inputs.
   **Recommendation:** in the next spec window, bound every fraction-typed mandate field
   (`max_position_fraction`, `rebalance_band`, `entry_threshold`, `exit_threshold`, a ladder rung's
   `factor`, a signal model's `weight`, `max_daily_loss`, `max_drawdown`, `hysteresis`,
   `max_loss_from_allocation`) to **12 fractional places** as a V-rule and a schema pattern, bound a
   model output's `conviction` and `confidence` to **18** (`MC-B13` already carries 13, so 12 would
   invalidate a committed case), and bound a condition value to the places its field's type holds. Until then item 8's typed refusal
   stands, which is the conservative reading. `schemas/` and `docs/specs/` are protected paths
   (ES-22), so this brief cannot make the change; it is a spec-change PR of its own.
2. **`reference/mandate/ref.py` is not an exact oracle beyond the committed cases.** It sets
   `getcontext().prec = 60` (ref.py:8), so it is 60 significant digits, not unbounded: `budget ÷ ask`
   and the three accumulate quotients round to 60 digits *before* `trunc` truncates them to the
   increment, and a quotient that sits within 10⁻⁶⁰ of a multiple of the increment can therefore
   truncate one increment higher there than exact arithmetic gives. In the other direction, an input
   beyond recommendation 1's bounds gets a number from Python and `overflow` from Rust. Every one of
   the 298 committed cases is inside both, so nothing diverges today.
   **Recommendation:** the Rust crate, not ref.py, is the oracle for anything beyond the 298 cases,
   and where the two differ the crate's exact 256-bit result is the correct one. A fuzz is therefore
   run against this story's independent oracles (in Scope, above), never differentially against ref.py.
   Recommendation 1 removes the second divergence; the first is inherent to a 60-digit context and is
   why no differential fuzz is planned.
3. **`xtask/layers.toml` and CODEOWNERS** gain `mandate-builder` (layer 5, safety-critical, pure,
   `allowed_external = ["thiserror"]`). Both are founder-owned; the tests PR adds the two lines for
   the founder to confirm or veto, as DEC-127 item 1 did for `mandate-backtest`.
4. **`UsdExact` puts a 256-bit value in a public API, which ES-04's engineering note does not
   foresee.** That note says "typed domain values stay within a 96-bit significand and scale ≤ 28",
   and `Sizes` reports `cap`, `target_value`, and `delta` as `UsdExact` because cap reaches 33 places
   and the target 57 (the arithmetic section), while §5.2 forbids rounding a value a comparison uses.
   **Recommendation:** amend the ES-04 note at the next ADR touch to name one wide reported type in
   `mandate-num` alongside the storage newtypes, bounded to the 256-bit intermediates ES-04 already
   requires and convertible to `Usd` only by an explicit rounding. The alternative the independent
   review raised — keep the wide value crate-private and have `propose` return the three figures as
   rationals compared by cross-multiplication — keeps the note intact but leaves the journal and the
   approval screen with a numerator and a denominator where the reference cases show `1500`, so it
   trades a documented type for an undocumented presentation. `docs/adr/` is not a protected path, but
   amending an accepted ADR is the founder's call either way.
5. **E6-2's acceptance criteria name `MC-A01` to `MC-A11` and `MC-B01` to `MC-B29`**, but spec v0.6
   added `MC-A12` to `MC-A16` (the admission ceiling and thesis confidence) and `MC-B30` to `MC-B32`
   (the trim guards). This story takes all 16 `A` cases; the three new `B` cases go to stream G with
   `MC-B17` (item 3). Recommendation: the backlog line is updated to "`MC-A01` to `MC-A16` and the
   `MC-B` builder cases" in the harness-and-status PR, which is a backlog edit, not a spec edit.

## Not done

- The risk gate itself, US account rules, the eligibility floor, market-conduct controls, and the
  drawdown ladder: stream G (`mandate-risk`).
- The mandate document, its validation, the policy hierarchy, change classification, and risk state:
  stream F (`mandate-spec`).
- §8.4 and §8.5, the research agent's thesis contract and admission, and §8.6's lineages: stream J.
- The approval flow itself (content, delivery, step-up, two-approver resolution, quiet hours). This
  story reports the approver count and `on_timeout`; §6.4's screen and the `Approval*` events are
  E6-1 and M7's.
- Protective order prices and the tranche model (trading §5.4), exit pricing ladders (§5.6), and the
  collar and tick rounding of a submitted price (item 17).
- The mandate reference-case harness and the `status.toml` move: the harness-and-status PR after
  stream F's harness and, for the `B` family, stream G's gate.
- Sizing methods other than `conviction_linear`, and any calibration (DEC-47: there is none in v1).

## Commands

```bash
cargo xtask check
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo nextest run -p mandate-builder -p mandate-num
cargo xtask ci pending
cargo mutants -p mandate-builder
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong (the ones found so far are DEC-130 and Decisions needed);
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The 16 `MC-A` cases and the 28 `MC-B` cases named in Scope pass, and no case that passed
      before now fails (the harness-and-status PR).
- [ ] Tests came first; each invariant above has a test whose oracle is independent and was shown to
      fail on a planted bug (the table above).
- [ ] New state changes emit journal events (none: the crate is pure; item 20).
- [ ] Docs updated: the feature map gains an "Autonomy and the order builder" entry, and the
      tracker's M5 stream H, Stories, and Claims rows.
- [ ] `cargo xtask check` is green (summary in each PR).
- [ ] Each PR description is complete (see the PR template).
