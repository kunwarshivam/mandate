# Task: E7-2, E7-3, E7-4 the idempotent executor and the broker connector abstraction

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task normally
implements one story; this one implements three that cannot be separated, because the idempotency key
(E7-2), the reconciliation that proves it worked (E7-3), and the protective-order sequences that every
one of those orders is wrapped in (E7-4) are one state machine over one stream. Splitting them would
put the account stream's single writer in three crates.

This is **stream K**: milestone M6 pulled forward in parallel with M5, so that when stream I's runtime
hands an intent to `IntentSink` there is something on the other side of it, and so that the Phase 1
exit — an agent trading an Alpaca paper account unattended through a soak — is not gated on M5
finishing first.

## Story

- **Stories:** E7-2, E7-3, E7-4 ([backlog](../06-backlog-v1.md#e7-alpaca-connector-and-recovery))
- **Acceptance criteria (verbatim):**
  - E7-2 (Must): "As an owner, I want order intents journaled with idempotency keys so that crashes
    never duplicate orders." No *Accepted when* clause, so this brief sets the bar: **every order the
    broker ever sees is named by a client order id that is a pure function of journaled facts, and the
    journal carries that name before the request leaves the process.**
  - E7-3 (Must): "As an owner, I want the agent to reconcile with the exchange after a restart so that
    its state matches reality." *Accepted when:* "fault injection at every submission step yields zero
    duplicates and full reconciliation; mismatches pause the agent and alert."
  - E7-4 (Must): "As an owner, I want protective exits resting at the broker as OCO or bracket orders,
    with defined exit and kill-switch sequences, so that positions keep protection if the platform is
    down." *Accepted when:* "RC-14 passes; unprotected windows are journaled and alerted beyond the
    limit."
- **PRD / HLD / spec anchors:** [HLD](../../HLD.md) §5 (the executor and the execution gateway,
  "Durability": the write-ahead intent, event-sourced state, venue-side protection), §6.B (the decision
  cycle's last step), §6.D (crash recovery: replay, reconcile, resume, and pause on anything
  unaccounted for), §4 ("Shared data plane"), §11 (Rust for connectors);
  [trading domain spec](../../specs/trading-domain.md) §5.1 to §5.7 (the v1 order policy, the Alpaca
  capability matrix, the constraints enforced before submission, protective exits and the tranche
  model, the kill switch, exit pricing, and the order lifecycle with the broker status mapping), §6.1
  (the fill record), §7.1 to §7.4 (the account ledger, buying power, account restrictions, agent
  modes), §9.1 (the gate's evaluation order, which the executor runs as the binding gate), §9.6 and
  §9.7 (conduct controls and order-rate limits the executor paces against), §10 (paper mode and the
  shadow ledger), §11 (reconciliation), §12 (the account-stream events);
  [journal spec](../../specs/journal.md) §2 (streams, the account stream's single writer, copied
  cross-stream facts, `intent_id` equals the agent stream's `IntentProposed` `event_id`), §3 (the
  envelope), §5.1 (the append protocol, idempotency, fencing), §5.2 (write before acting: "the executor
  journals `OrderSubmitted` **before** the broker request", and recovery by `client_order_id`), §8
  (replay and `fold_version`), §9 (the account-stream catalogue), §12 (export);
  [ADR-0001](../../adr/0001-engineering-setup.md) ES-02 (`mandate-executor` at layer 6 and
  `mandate-alpaca` at layer 7, DEC-133 item 1 as amended, both safety-critical), ES-06 (`handle(&mut State, Input) -> Vec<Effect>`,
  one task per broker account owns its ledger, a priority channel for kill-switch and risk-exit
  commands, one fenced `StreamWriter` per stream, effects only after `Committed` or `AlreadyCommitted`,
  `IdGen` injected), ES-09 (typed errors with stable codes, `secrecy`, a log scan), ES-19 (injected
  transport and clock), ES-20 (`IntentSink` and `TimerSource`), ES-21 (determinism), ES-22 (spec
  anti-drift), ES-23 (vendor numbers never through `f64`, the paper/live boundary), ES-24 (the latency
  budget); [glossary](../../product/glossary.md) (executor, account ledger, intent, idempotency key,
  protective exit, reconciliation).
- **Decisions that apply:** DEC-07 (journal intent before sending any order; idempotency keys;
  event-sourced recovery — the decision this whole stream implements), DEC-23 (Alpaca is the first
  connector), DEC-26 (one serialized account ledger per broker account; one agent per instrument per
  account; external activity switches every agent to exits-only), DEC-28 and DEC-36 (protective exits
  as OCO and bracket orders, the tranche model, and the crypto stop-limit sequences), DEC-29 and DEC-37
  (limit-only openings within a collar; market orders only for risk-reducing exits in the regular
  session), DEC-34 and DEC-104 (buying power, the per-bucket ceiling, and reservations the executor
  holds), DEC-05 and DEC-06 (reducing risk needs no approval; ambiguity resolves safe), DEC-08 (one
  process per agent deployment, which is why the executor is a second process and a second writer),
  DEC-11 (notifications carry opaque IDs), DEC-17 through ES-20 (still `Proposed (founder)`; it touches
  this stream's shell and nothing in its core), DEC-72 (ADR-0001), DEC-77 (brief, tests PR,
  implementation PR), DEC-79 (agents land their own changes; the founder's reserved list), DEC-80 (no
  plain comments), DEC-83 (a tests PR holds stubs only, so the mutation gate skips the crate),
  DEC-85 (an uninterpreted input fails loudly), DEC-89 and ES-21 (exact arithmetic, ordered containers,
  no clock, no randomness), DEC-100 (the platform operator's actions), DEC-103 (the Phase 1 thin
  slice), DEC-105 (`buying_power` in the harness is honest only while no reservation is tracked — this
  stream is what makes reservations real), DEC-107 (the artifact store the raw broker exchanges are
  written to), DEC-110 (every pending test fails on the stubs), DEC-117 to DEC-126 (the mandate spec
  v0.6 answers, still `Proposed (founder)`), DEC-129 (stream G's gate, which this executor calls as the
  binding gate), DEC-131 (stream I's runtime: the `IntentSink` the shell's adapter implements on this
  executor's behalf, the effect
  shapes, outstanding intents, `Input::Started` recovery, and the executor as the binder for
  cancel-all), and **DEC-133** (this stream's interpretations, below).
- **Risks this stream is the mitigation for:** [R-03](../03-raid-log.md) (duplicate or orphaned orders
  after crashes: "write-ahead intents with idempotency keys; reconciliation on restart; fault-injection
  gate" — the fault-injection gate is this brief's test design), R-19 (breaking US account rules or
  triggering broker restrictions: the executor is where the restriction table is read from rejects),
  R-22 (paper is more optimistic than live: the shadow ledger), R-23 (positions unprotected during exit
  sequences, partial bracket fills, extended hours, gaps, or fractional shares: every unprotected
  interval journaled and bounded).

## Scope

### Reference cases

**None move from pending to passing in this stream's tests PR**, because the tests PR ships stubs and
a stub passes nothing (DEC-83). The harness steps and expectation keys this stream owns are already
attributed in `crates/mandate-refcases/src/trading_domain.rs`, and they move in a **status PR after
the implementation PR** (the DEC-105 and E4-1 precedent), not here:

| Harness step or key | Attributed to | Earned by |
|---|---|---|
| `broker_order_update` (step), `orders` (expectation) | E7-2 | the order state machine and the status mapping |
| `reconciliation`, `broker_position_update` (steps), `reconciliation` (expectation) | E7-3 | the reconciliation function |
| `corporate_action_prepare` (step), `actions`, `protective_sell_qty` (expectations) | E7-4 | the protective sequences and the action list |
| `initial.open_orders` | E7-4 | resting protective orders as initial state |

The cases those unlock, and which this stream must reproduce exactly once implemented:
`trading_domain::RC-14` and its `passive_exit_becomes_oco_take_profit`, `plain_add_blocked`,
`add_via_bracket`, and `kill_switch` variants (E7-4's own acceptance clause names RC-14); `RC-04`
(a 4-for-1 split: the OCO cancel, the broker-posting gate, and protection re-derived); `RC-06`'s
`protective_orders_kept_through_dividend` variant; `RC-07` and `RC-11` (unposted crypto fees and the
settlement calendar, whose reconciliation halves are this stream's and whose accounting halves are
already covered); `RC-20` (the crypto stop-limit add and exit sequences); `RC-21` (a bracket partly
filled: the OCO for the filled quantity and re-placement before expiry); `RC-24` (the exit price
ladder in extended hours and the presumed-halt variant); `RC-15` (restrictions detected from rejects
and status, and an external order — shared with E6-9); `RC-17` (the account ledger's instrument claims
and shared buying power — shared with E7-5); and the `broker_order_update` steps of `RC-08`, `RC-09`,
`RC-09B`, `RC-18`, `RC-22`, `RC-23`, and `RC-25`. Each case's owning story is named when the harness
runs it, so nothing here is silent.

`RC-14`'s `kill_switch` variant is worth one line now, because its harness action names read like a
contradiction of AGENTS.md rule 13: `cancel_all_orders: agent` and `close_all_positions: agent` are
**scoped** actions meaning "every order *of that agent*, by `client_order_id`, each confirmed" and
"sell exactly the agent's sub-ledger quantity". The broker's `cancel-all` and `close-position`
endpoints belong to the account and workspace scopes alone (trading §5.5), and interpretation 18
below makes that unrepresentable rather than merely forbidden.

### Stream boundaries

| Concern | Owner |
|---|---|
| The account stream, the intent protocol, idempotency keys, the order state machine, submission, cancel, replace, reconciliation, protective sequences, unprotected intervals, reservations, the shadow ledger, the broker client | **This stream (K)** |
| The agent stream, the runtime loop, decisions, approvals, mode application, `IntentProposed`, the agent-side kill switch | Stream I (`mandate-runtime`), brief [#126](https://github.com/kunwarshivam/mandate/pull/126) |
| The binding gate's rules: the eight §9.1 checks, US account rules, the eligibility floor, conduct controls, restrictions, the flatten plan | Stream G (`mandate-risk`), brief [#127](https://github.com/kunwarshivam/mandate/pull/127) |
| The mandate document, validation, policy, change classification, and the risk-state fold **as arithmetic** | Stream F (`mandate-spec`, `mandate-domain`), brief [#129](https://github.com/kunwarshivam/mandate/pull/129) |
| **Appending** the risk-state records and `UniverseChanged` on the account stream (`MandateVersionApplied`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, `GoalCompleted`, `RiskDayStarted`) | **This stream (K)**, as the stream's single writer; the values come from F's fold and J's admission (interpretation 3, Decisions needed 8) |
| Autonomy classification and order sizing | Stream H (`mandate-builder`), brief [#128](https://github.com/kunwarshivam/mandate/pull/128) |
| Theses, admission, and the working universe's content | Stream J (E17 thin slice), brief [#138](https://github.com/kunwarshivam/mandate/pull/138) |
| Positions, cash, fees, settlement, corporate actions as a fold | `mandate-accounting` (E3-1 to E3-3, merged), used unchanged |
| Drafts, canonical bytes, the append protocol, artifacts | `mandate-journal`, `mandate-journal-pg`, `mandate-artifacts-fs`, used unchanged |
| OAuth, connection scopes, key-permission checks | E7-1, **M8**, out of scope |
| Instrument-group claims, deployment admission, external-activity acknowledgment | E7-5, a later story; this stream ingests external activity and sets the restriction, and does not arbitrate claims |
| Escalation delivery, quiet hours, step-up evidence | M7 and DEC-16 |

The gate is the one boundary that is **not** a port. Stream I's runtime calls the gate as an injected
`GateDryRun` that "can only narrow"; the executor calls `mandate-risk` as a **direct, crate-private
dependency**, because a binding gate behind an injectable trait is a binding gate an agent's
configuration could replace with a permissive one (AGENTS.md rule 1: "Limits are enforced by the risk
gate, independent of agent logic"; journal §2: "the risk gate is a pure library it calls"). See
interpretation 4.

### Invariants touched

Each row gets a named test whose oracle computes the answer its own way.

| Clause or invariant | Test |
|---|---|
| Journal §5.2, AGENTS.md rule 5, DEC-07 journal before acting: every `Effect::Broker` that submits is preceded in the same effect list by the `OrderSubmitted` draft that names it | `properties::every_submit_effect_follows_the_order_submitted_draft_that_names_it`, `hand::a_submission_journals_before_the_request_leaves` |
| E7-2 the client order id is a pure function of journaled facts: the same intent derives the same id in every process, every epoch, and every restart | `properties::a_client_order_id_is_a_function_of_the_intent_id_alone`, `hand::two_processes_derive_one_client_order_id_for_one_intent`, `hand::a_resubmission_after_a_crash_reuses_the_same_client_order_id` |
| E7-2 no two intents ever share a client order id, across restarts and across agents on one account | `properties::distinct_intents_never_share_a_client_order_id`, `hand::a_new_intent_after_a_restart_derives_a_fresh_client_order_id` |
| E7-3 zero duplicates: a crash at any of the twelve injection points leaves the broker having seen at most one order per intent | `properties::no_crash_point_makes_the_broker_see_two_orders_for_one_intent`, the twelve `fault::crash_at_*` cases |
| Journal §5.2, trading §5.7 an `OrderSubmitted` with no acknowledgment is resolved by a query on `client_order_id`, never by a blind resubmit | `hand::an_unacknowledged_submission_queries_before_it_resubmits`, `hand::an_order_the_broker_confirms_present_is_adopted_not_resent`, `properties::no_recovery_submits_without_a_confirmed_absence` |
| Trading §5.7 `Unknown → Intent` only after N confirmed absences over T seconds, then the gate re-runs and an intent older than `max_intent_age` is abandoned | `hand::one_absent_lookup_does_not_resubmit`, `hand::an_absence_confirmed_over_the_window_resubmits_the_same_id`, `hand::a_stale_intent_is_abandoned_rather_than_resubmitted`, `hand::a_gate_denial_on_re_check_abandons_the_intent` |
| Trading §5.7 the age check guards **every** `Intent → Submitting` transition, not only a resubmission, so an intent re-handed after an outage is abandoned rather than submitted stale | `hand::a_stale_intent_is_abandoned_at_its_first_submission`, `properties::no_submission_carries_an_intent_older_than_its_maximum_age` |
| Trading §5.7 terminal states are final, and a fill for a terminal order is a `late_fill` that is still applied and triggers reconciliation | `hand::a_fill_after_a_terminal_state_is_applied_as_a_late_fill`, `hand::a_late_fill_triggers_a_reconciliation`, `properties::no_terminal_order_leaves_its_terminal_state` |
| Trading §5.7 filled quantity is non-decreasing, at most the order quantity, and equals the sum of unique fills | `properties::filled_quantity_equals_the_sum_of_unique_fills`, `hand::a_repeated_fill_id_changes_nothing` |
| Trading §5.7 an illegal transition is journaled and ignored, and fills in it are still applied | `hand::an_illegal_transition_is_journaled_and_ignored`, `hand::a_fill_inside_an_illegal_transition_is_still_applied` |
| Trading §5.7 the status mapping is total over the table and any other value pauses the agent and alerts | `hand::every_broker_status_maps_to_the_table_row`, `hand::an_unknown_broker_status_pauses_the_agent_and_alerts`, `properties::the_status_map_is_total_and_never_silently_ignores` |
| Trading §5.7, §5.3 rule 9 an `Unknown` order reserves its maximum cost, counts as filled for exposure, and blocks new orders in that instrument | `hand::an_unknown_order_reserves_its_maximum_cost`, `hand::an_unknown_order_blocks_new_orders_in_the_instrument`, `properties::a_reservation_is_never_released_before_a_terminal_state` |
| Trading §5.7 a reservation is released by every one of the six terminal states, `Abandoned` and `Replaced` included, and a `Replaced` order's reservation passes to the linked new order | `properties::every_terminal_state_releases_its_reservation`, `hand::an_abandoned_order_releases_its_reservation`, `hand::a_replaced_orders_reservation_passes_to_the_new_order` |
| Trading §11, §6 the broker is the source of truth for **open orders**: an order-state difference is adopted with a journaled compensating event, and a missing fill is ingested | `properties::every_order_difference_adopts_the_broker_with_a_compensating_event`, `hand::a_journal_only_order_is_reconciled_away_not_kept`, `hand::every_adoption_journals_a_compensating_event_with_the_difference` |
| Trading §11 a position, cash, or fee difference is **never adopted**: §11's on-mismatch column pauses, alerts, or refuses to adjust, so nothing outside the order set is silently overwritten either way | `properties::no_position_cash_or_fee_difference_is_ever_adopted`, `hand::a_position_difference_is_not_written_away_as_a_compensating_event`, `hand::a_fee_difference_is_alerted_and_never_adjusted` |
| E7-3 full reconciliation: after the run every broker order is matched or adopted, every missing fill is ingested, and every remaining position difference has paused its agents | `properties::a_reconciliation_leaves_nothing_unexplained_and_unpaused`, `hand::an_unexplained_position_pauses_the_agent_and_alerts` |
| Trading §11 fills and orders are reconciled before positions, so a missing fill explains a difference instead of pausing on it | `hand::a_missing_fill_explains_the_position_and_pauses_nothing`, `properties::the_reconciliation_order_is_orders_then_fills_then_positions_then_cash_then_fees` |
| Trading §11 the tolerances are the spec's: equity exact except `pending_corporate_action`, crypto net plus unposted asset fees until posting, cash within the stated band, fees exact once posted and never silently adjusted | `hand::an_equity_difference_of_one_share_is_a_mismatch`, `hand::a_pending_corporate_action_difference_is_not_a_mismatch`, `hand::unposted_crypto_asset_fees_explain_the_crypto_difference`, `hand::a_fee_difference_is_alerted_and_never_adjusted`, `properties::cash_within_the_band_never_pauses_and_outside_it_always_alerts` |
| Trading §11, §7.1 an order the broker has and we do not is external activity: it is ingested, journaled, and every agent on the account goes `exits_only` | `hand::an_unknown_broker_order_becomes_external_activity`, `hand::external_activity_switches_every_agent_to_exits_only`, `hand::a_reject_for_an_unknown_client_order_id_is_not_external_activity_without_a_fill` |
| Trading §11 the executor never lifts a reconciliation pause: only an owner acknowledgment with step-up does | `hand::the_executor_never_lifts_a_reconciliation_pause_itself`, `properties::no_input_but_an_acknowledged_owner_ack_clears_a_mismatch_pause` |
| Journal §2, DEC-131 item 13 a `ReconciliationRun` is never positioned after a submission it did not cover, so stream I's positional rule holds without reading the payload | `properties::no_reconciliation_run_is_appended_after_a_submission_it_did_not_cover`, `hand::a_submission_between_the_snapshot_and_the_run_recomputes_the_run`, `hand::a_startup_reconciliation_covers_every_submission_it_reports_on` |
| Trading §5.4 the tranche model: each protected entry is one GTC bracket, and Σ protective sell quantity never exceeds the position | `properties::protective_sell_quantity_never_exceeds_the_position`, `hand::an_add_is_a_new_bracket_not_a_replacement` |
| Trading §5.4 bracket legs are held until the entry is completely filled, and a partial entry becomes a GTC OCO for the filled quantity at the bracket's prices | `hand::a_partly_filled_bracket_becomes_an_oco_for_the_filled_quantity`, `hand::an_entry_unfinished_at_the_timeout_is_cancelled_then_oco_d`, `hand::a_terminal_partly_filled_entry_is_oco_d_at_once` |
| Trading §5.4, E7-4 every unprotected interval is journaled from start to end, bounded by `max_unprotected_s`, and alerted beyond the limit | `properties::every_unprotected_interval_has_a_journaled_start_and_end`, `hand::an_unprotected_interval_at_the_limit_cancels_re_places_and_alerts`, `properties::no_interval_exceeds_the_limit_without_an_alert` |
| Trading §5.4 the marketable exit sequence is cancel, confirm, re-run the gate on fresh state, submit, then re-place protection for the remainder | `hand::an_exit_follows_cancel_confirm_regate_submit_replace`, `hand::an_exit_never_submits_before_the_cancel_is_confirmed`, `properties::no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding` |
| Trading §5.4 an order submitted while protection is cancelled is marketable at submission | `hand::an_order_submitted_without_protection_is_marketable`, `properties::no_resting_order_is_submitted_inside_an_unprotected_interval` |
| Trading §5.4 a passive exit keeps the stop: it becomes the take-profit leg of a new OCO, and protection is never removed for it | `hand::a_passive_exit_becomes_a_new_oco_keeping_the_stop`, `hand::a_passive_exit_never_leaves_the_position_unprotected` |
| Trading §5.4 protection is re-placed before GTC expiry, at the first `TradingDayStarted` on or after the buffer | `hand::protection_is_re_placed_at_the_buffer_day`, `hand::protection_is_not_re_placed_early` |
| Trading §5.4 the triggered-stop watchdog cancels, confirms, and exits through the ladder as a `risk_exit`, and alerts | `hand::a_stop_at_its_trigger_price_without_a_fill_is_watchdogged`, `hand::the_watchdog_exit_is_a_risk_exit_through_the_ladder` |
| Trading §5.4 crypto: one GTC stop-limit for the whole position, limit = stop × (1 − offset), adds are limit IOC, exits marketable, re-placed for the new net quantity | `hand::a_crypto_position_carries_one_stop_limit_for_the_whole_position`, `hand::a_crypto_add_is_a_limit_ioc_inside_the_sequence`, `hand::a_crypto_stop_limit_is_re_placed_for_the_new_net_quantity` |
| Trading §5.4 only the whole-share part of a fractional position is protected, and the fraction is disclosed | `hand::a_fractional_position_protects_the_whole_shares_and_discloses_the_fraction` |
| Trading §5.6 the exit price ladder: the reference bid's fallback order, the step interval, and the floor at which the order rests and the owner is alerted | `hand::the_ladder_prices_from_a_fresh_sane_quote_first`, `hand::the_ladder_falls_back_to_the_last_sane_bid_then_the_last_trade`, `hand::the_ladder_steps_only_after_the_interval`, `hand::the_ladder_never_prices_below_the_floor`, `properties::every_ladder_price_is_within_the_tier_offsets` |
| Trading §5.5, AGENTS.md rule 13 the agent-scoped kill switch cancels only the agent's orders by client order id, each confirmed, and sells exactly the sub-ledger quantity | `hand::an_agent_kill_switch_cancels_only_that_agents_orders`, `hand::an_agent_kill_switch_sells_exactly_the_sub_ledger_quantity`, `properties::no_agent_scoped_effect_can_name_the_account_wide_endpoints` |
| Trading §5.5 the account and workspace scopes are the only callers of `cancel-all` and `close-position`, and they include `Unknown` orders | `hand::an_account_kill_switch_uses_cancel_all_and_close_position`, `hand::an_account_cancel_all_covers_unknown_orders` |
| Trading §5.5 the final mode is applied first, then cancel, then confirm, then close; automated sells wait for the regular session and crypto goes at once | `hand::a_kill_switch_applies_the_mode_before_it_cancels`, `hand::an_automated_flatten_defers_equity_sells_to_the_session`, `hand::an_automated_flatten_sells_crypto_at_once`, `properties::the_mode_draft_precedes_every_cancel_and_every_sell` |
| Trading §5.5, mandate §6.1 an owner exit outside the regular session needs the confirmed bid and the floor from `OwnerExitRequested`, and without it equity sells wait | `hand::an_owner_exit_outside_the_session_prices_from_the_confirmed_bid`, `hand::an_unconfirmed_owner_exit_waits_for_the_session` |
| AGENTS.md rule 13 risk reduction is never denied by conduct controls, eligibility, day-trade budgets, buying power, or opening-session rules | `properties::no_risk_reducing_submission_is_ever_denied_by_a_pacing_control`, `hand::a_risk_exit_submits_inside_the_close_window`, `hand::a_protective_order_submits_with_no_buying_power` |
| AGENTS.md rule 13 exits are held only by `paused`, `stopped`, an `Unknown` order in the instrument, or the broker | `properties::the_only_holds_on_an_exit_are_the_four_the_rule_names`, `hand::an_unknown_order_holds_an_exit_in_that_instrument_alone` |
| Mandate §5.9 on entering `exits_only` or stricter, the executor cancels the agent's working opening orders | `hand::entering_exits_only_cancels_the_working_opening_orders`, `hand::entering_exits_only_leaves_protective_orders_resting` |
| Trading §5.3 rule 5 before a risk-reducing sell the executor cancels the agent's own resting opening buys and waits for confirmation | `hand::a_reducing_sell_cancels_the_resting_opening_buys_first`, `hand::the_reducing_sell_waits_for_the_cancel_confirmation` |
| Trading §5.3 rule 5, AGENTS.md rules 3 and 13, DEC-160 (7) and (13) the wait is bounded by `unknown_absent_window_s` from the `GateDecided` allowing the exit, whatever state the opening is in, and every waiting exit is re-evaluated after every step, so no exit stays neither submitted nor denied past the bound plus one tick except under rule 13's holds | `protection::sequence_tests::no_exit_waits_past_the_bound_but_under_a_rule_13_hold`, `protection::sequence_tests::a_submitting_opening_never_acknowledged_is_queried_at_the_bound`, `protection::sequence_tests::an_opening_reported_gone_without_a_confirmation_releases_the_exit`, `protection::sequence_tests::an_opening_filled_while_its_cancel_is_outstanding_releases_the_exit` |
| Trading §7.3 restrictions are detected from rejects and statuses and stored as account state, evaluated before agent mode | `hand::a_closing_only_reject_sets_the_account_restricted`, `hand::n_consecutive_403s_without_a_known_cause_set_closing_only`, `hand::an_unknown_client_order_id_reject_counts_only_toward_the_threshold` |
| Trading §7.2, DEC-34, DEC-104 the gate's buying power is the lower of the model and the broker, reservations included, uncleared deposits excluded | `hand::buying_power_is_the_lower_of_model_and_broker`, `properties::a_reservation_lowers_buying_power_by_exactly_its_amount` |
| Trading §10, R-22 paper regulatory fees and dividends are booked in the shadow ledger, excluded from cash reconciliation, included in P&L and buying power | `hand::a_paper_fill_books_a_simulated_fee_in_the_shadow_ledger`, `hand::a_simulated_fee_is_excluded_from_cash_reconciliation_and_included_in_buying_power`, `properties::no_simulated_record_reaches_a_cash_comparison` |
| ES-23 a stream's `environment` is fixed at `StreamOpened`, and nothing in this stream can address a live host | `hand::an_append_with_another_environment_is_rejected`, `hand::the_client_refuses_a_path_that_is_not_a_paper_trading_endpoint`, `properties::every_request_the_client_can_build_targets_the_paper_host` |
| ES-23 broker numbers never pass through `f64`: every quantity, price, and amount is parsed from raw text into `mandate-num` | `hand::a_broker_decimal_with_nine_places_parses_exactly`, `hand::a_broker_number_in_exponent_form_is_rejected_with_its_code`, and the crate's lint header |
| AGENTS.md rule 7, ES-09 credentials never appear in a log, an error, a draft, a fixture, or `Debug` | `hand::credentials_are_redacted_in_debug_output`, `hand::a_transport_error_names_no_url_and_no_header`, `hand::recorded_fixtures_contain_no_credential`, `properties::no_draft_payload_holds_a_credential_or_an_account_number` |
| Journal §6.4, trading §13 the broker's account number is held by reference and never journaled or logged | `hand::the_account_ref_is_an_opaque_id_not_an_account_number`, `properties::no_draft_holds_a_broker_account_number` |
| Journal §9, §6.4 `BrokerExchangeRecorded` redacts the authorisation headers **and** every personal-data field in the body before the bytes are hashed or stored, by artifact reference when large | `hand::a_broker_exchange_is_recorded_with_its_credentials_redacted`, `hand::an_account_body_is_recorded_with_its_account_number_replaced_by_a_pii_ref`, `hand::a_large_exchange_is_recorded_by_artifact_reference`, `properties::no_recorded_exchange_holds_an_account_number_or_an_account_id` |
| AGENTS.md rule 6, DEC-11 alert payloads carry opaque IDs and generic text only | `hand::an_alert_carries_only_opaque_ids`, `properties::no_alert_payload_holds_an_instrument_a_price_or_a_quantity` |
| Journal §5.1 idempotency and fencing: a retry after `Unavailable` or `Ambiguous` derives the same `event_id`, and a `Fenced` append stops the process | `hand::a_retried_append_derives_the_same_event_id`, `hand::a_fenced_append_stops_the_executor`, `properties::a_derived_event_id_is_a_function_of_epoch_head_and_ordinal` |
| Journal §2 gapless `seq`, and the copied facts carry `causation_id` to their origin | `hand::a_gap_in_seq_fails_the_fold`, `hand::a_copied_agent_mode_points_at_the_originating_event`, `properties::every_copied_draft_cites_its_origin` |
| Journal §2 every account-stream risk input carries `risk_clock`, never decreasing | `properties::every_risk_input_draft_carries_a_non_decreasing_risk_clock`, `hand::a_risk_input_without_a_risk_clock_is_refused` |
| ES-21, journal §8 state is a fold: replaying the drafts a run journaled reproduces its state and emits nothing | `properties::folding_the_journaled_drafts_reproduces_the_live_state`, `properties::a_replay_emits_no_draft_and_no_broker_effect`, `hand::the_golden_journal_folds_to_the_committed_state` |
| ES-21 determinism: two runs of the same inputs give equal effect lists; ordered containers, no floats, no clock, no randomness | `properties::two_runs_of_the_same_inputs_give_equal_effects`, and the crate's lint header |
| ES-06 the priority channel: a kill-switch command is handled before a full queue of ordinary inputs, and the handling order is what gets journaled | `hand::a_kill_switch_jumps_a_full_queue`, `hand::the_journaled_order_is_the_handling_order` |
| DEC-131, ES-02 this crate declares no `IntentSink` and names no `mandate-runtime` type: an intent enters only as `Input::Intent`, and the adapter is the shell's | `hand::an_intent_enters_only_as_an_input`, and `cargo xtask layers` |
| DEC-85 an uninterpreted input fails loudly, naming the owning story | `hand::an_unknown_event_type_fails_the_fold`, `hand::an_uninterpreted_broker_field_names_its_story`, `properties::every_catalogue_event_is_interpreted_or_named` |
| ES-09 every error variant has a stable `code()` and the set is exhaustive | `hand::every_error_code_is_stable_and_unique` |

### Oracles

`crates/mandate-executor/tests/properties.rs` holds four implementations that share no code with the
crates under test:

1. **A broker-side counter.** The fake connector keeps a `BTreeMap<ClientOrderId, u32>` of how many
   distinct submissions it accepted and a `BTreeSet` of the order bodies it saw, accumulated
   independently of the executor's own state. "Zero duplicates" is read off that map, not off the
   journal, so an executor that journals one `OrderSubmitted` and sends twice still fails.
2. **A shadow order book.** It rebuilds each order's state, filled quantity, and reservation from the
   emitted drafts' canonical bytes alone, as a `BTreeMap` keyed by client order id, using a separately
   written transition table transcribed from trading §5.7. It is what
   `folding_the_journaled_drafts_reproduces_the_live_state` compares against, so an executor that keeps
   state the journal does not carry fails.
3. **A shadow position ledger.** Positions, cash, and accrued fees are re-accumulated from
   `FillApplied`, `LateFillApplied`, `FeesCharged`, and the corporate-action events with `i128`
   arithmetic, independently of `mandate-accounting`, so a reconciliation that compares the ledger
   against itself cannot pass.
4. **A protection accountant.** For each instrument it sums protective sell quantity and records every
   interval in which that sum is below the position, from the drafts alone, so "every unprotected
   interval is journaled and bounded" is checked against an interval set the executor did not build.

Every property first compares the **number** of emitted effects and of accepted submissions with the
oracle's, so no property can pass on an empty list (the E4-1 lesson). Each oracle is shown to fail on
a seeded bug before it is trusted; the planted-bug table below is that evidence.

### Crates

- **In scope, new: `mandate-executor`.** Recommended `layer = 6`, `pure = true`,
  `safety_critical = true`, `allowed_external = ["thiserror"]`, with a CODEOWNERS line. It depends on
  `mandate-num`, `mandate-time`, `mandate-canon`, `mandate-journal`, and `mandate-accounting` from the
  tests PR, and on three M5 crates from the implementation PR, once they exist: `mandate-domain`
  (layer 1) and `mandate-spec` (layer 3) for the mandate view `Ports` reads, and `mandate-risk`
  (layer 4) for the binding gate. Layer 6 is what ES-02 plans and what lets the executor see all three
  while sitting beside `mandate-runtime` rather than under it. It does **not** depend on
  `mandate-runtime`: they are the same layer, so the `IntentSink` adapter is the shell's (see Data
  shapes).
- **In scope, new: `mandate-alpaca`.** Recommended `layer = 7` (DEC-133 item 1 as amended by PR
  #152: an adapter over the core that implements the executor's trait sits above it), `pure = false`,
  `safety_critical = true`, `allowed_external = ["reqwest", "rustls", "secrecy", "serde", "serde_json", "thiserror", "tokio"]`,
  with a CODEOWNERS line. It depends on `mandate-num`, `mandate-time`, and `mandate-executor` (it
  implements the connector trait the executor declares); nothing depends on it except the shell and its
  own tests, so the executor's core never sees an HTTP type.
- **In scope, touched:** `Cargo.toml` workspace members (two lines, founder-owned),
  `xtask/layers.toml` and `CODEOWNERS` (two entries each, founder-owned, Decisions needed 1),
  `docs/dependencies.md` "Used by" cells for `reqwest`, `rustls`, `secrecy`, `serde`, `serde_json`,
  `thiserror`, `tokio`, and `proptest` (no new dependency, so no new row).
- **Out of scope:** `mandate-journal`, `mandate-journal-pg`, `mandate-artifacts-fs`,
  `mandate-accounting`, `mandate-marketdata`, `mandate-sim`, `mandate-backtest`, `mandate-cli`,
  `crates/mandate-refcases/` and `status.toml`, and every file under `docs/specs/`, `schemas/`,
  `reference/`, and `fixtures/`.
- **New dependencies:** none. Every crate named above is already registered in
  [docs/dependencies.md](../../dependencies.md).
- **Safety-critical:** yes, on six counts from the AGENTS.md list (accounting inputs, the account
  ledger and protective-exit sequencing, the executor and idempotency and reconciliation and crash
  recovery, broker connectors, credential handling, notification payloads). DEC-77 sequence: this
  brief, then the tests PR (two crate skeletons, stubs, pending tests, hand-built fixtures, a
  planted-bug report), then the implementation PR in which test files change only by deleting
  `#[ignore = "pending E7-2"]`, `#[ignore = "pending E7-3"]`, and `#[ignore = "pending E7-4"]` lines.
- **Size budget:** 400 non-generated lines per PR in safety-critical crates (ES-13). The tests PR may
  exceed it for test code and fixtures and says how it splits: (1) `mandate-executor`'s types, the
  intent protocol, and the order state machine; (2) reconciliation and the fault-injection suite;
  (3) the protective sequences and the ladder; (4) `mandate-alpaca`'s client, wire types, and
  fixtures. Each is its own PR against `main` if the first exceeds the budget on its own.
  The tests PR (#152) carries the stubs and vocabulary in `src` above the budget; the split
  governs the implementation PRs (#174's slices), not the tests PR (review round 2).

## Data shapes

### `mandate-executor`: the core

The same ES-06 shape as stream I, because the executor is also a single-writer state machine over one
stream, and two state machines built the same way are two state machines a reviewer can read at once.

```rust
/// Everything the executor knows about one broker account, derived from journaled events and
/// nothing else: orders by client order id, reservations, protection per instrument, unprotected
/// intervals, the account snapshot last observed, per-agent modes, and the reconciliation checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorState { /* every field private */ }

/// Replays one journaled event into the state. Total over the account-stream catalogue,
/// effect-free (journal spec section 8).
pub fn fold(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError>;

/// The live step, the only producer of effects (ES-06). `ports` carries the injected pure
/// collaborators: `IdGen`, the mandate view, and the instrument snapshot.
pub fn handle(
    state: &mut ExecutorState,
    input: Input,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, ExecutorError>;
```

Recovery is an input, not a third entry point, exactly as DEC-131 item 2 fixed it for the runtime:

```rust
pub enum Input {
    /// The process folded the account stream and took `writer_epoch`. The only input that may
    /// re-resolve an unacknowledged submission, and the input that opens the startup reconciliation.
    Started(WriterEpoch),
    /// An event tailed from a stream the executor follows, in `seq` order. The agent streams it
    /// follows are where `IntentProposed` arrives from.
    Journal(FoldedEvent),
    /// An intent the shell's adapter took from stream I's `IntentSink` (ES-20). Carries no authority:
    /// it is journaled as `IntentReceived` and then gated like any other. Both `IntentBody` variants
    /// arrive here, an `Order` and a `Flatten(FlattenPlan)`, so the agent-scoped kill switch's plan
    /// comes down the same path as an ordinary proposal.
    Intent(IntentHandoff),
    /// What the connector answered, or that it answered nothing. The only way a broker fact enters.
    Broker(BrokerOutcome),
    /// A broker-pushed order or account update the shell polled or streamed.
    BrokerUpdate(BrokerUpdate),
    /// A snapshot for reconciliation: open orders, positions, the account, and activities since the
    /// checkpoint, gathered by the shell in one pass.
    BrokerSnapshot(BrokerSnapshot),
    /// The scheduler's tick: a whole-second risk clock and nothing else. Drives every timeout in
    /// this crate (the ladder step, the bracket timeout, the unprotected bound, the watchdog).
    Tick(RiskClock),
    /// A quote or mark the shell observed, used for collar and ladder pricing, never for time.
    Market(MarketObservation),
    /// A command to the account's executor: a kill switch at any scope, or a reconciliation request.
    Command(Command),
}

pub enum Effect {
    /// Append to the account stream. The shell runs these first and in order.
    Journal(EventDraft),
    /// One broker request, described and not performed. Never emitted before the draft that records it.
    Broker(BrokerRequest),
    /// Arm or cancel one keyed timer through `TimerSource`.
    Timer(TimerRequest),
    /// An owner alert: opaque IDs and generic text only (AGENTS.md rule 6).
    Notify(NotificationRef),
}
```

The connector is described, never called by the core. `BrokerRequest` is a closed enum, and this is
where AGENTS.md rule 13 becomes unrepresentable rather than merely forbidden:

```rust
/// Every request this executor can make. `CancelAll` and `ClosePosition` take an
/// `AccountWideScope`, a type only the account and workspace kill-switch paths can construct, so no
/// agent-scoped code path can name them (trading domain spec section 5.5, AGENTS.md rule 13).
pub enum BrokerRequest {
    Submit(SubmitOrder),
    Cancel { client_order_id: ClientOrderId },
    /// Broker-initiated replacements only (section 5.1 forbids ours); carried so the new order can be linked.
    AcknowledgeReplace { replaced: ClientOrderId },
    GetOrderByClientId(ClientOrderId),
    ListOpenOrders,
    ListPositions,
    GetAccount,
    ListActivities { since: ActivityCursor },
    CancelAll(AccountWideScope),
    ClosePosition(AccountWideScope, InstrumentId),
}
```

The connector trait is shell-driven, in the same shape as stream I's shell-side ports (ES-20).
`mandate-alpaca` implements it; the core only builds the request and folds the answer:

```rust
/// One broker round trip. Implemented by `mandate-alpaca` and by the tests' fake connector. An
/// `Err` is not a rejection: it means the outcome is unknown and recovery must query.
pub trait BrokerConnector {
    fn call(&mut self, request: &BrokerRequest)
        -> impl Future<Output = Result<BrokerOutcome, BrokerUnknown>>;
}
```

**`IntentSink` is declared once, in `mandate-runtime`, and this crate does not re-declare it.**
`mandate-runtime` and `mandate-executor` are both layer 6, so neither can name the other's types and
neither can implement the other's trait: a second declaration here would be a second, incompatible
trait with the same name. What this crate exposes instead is `Input::Intent`, and **the adapter that
implements stream I's `IntentSink` by handing a proposal to this executor lives in the shell**, which
is the one place that may depend on both (review round 1, finding 4). That shell is `mandate-shell` at
**layer 8**, not layer 7: [DEC-138](../04-decision-log.md#decisions) amends DEC-133 item 1, because the
shell must also see `mandate-backtest` at layer 7 (the tracer's signal) and a crate may depend only on
strictly lower layers. Nothing else about this stream changes; the coordinator ruled this alignment onto
the tracer brief's PR (#173). In one process the
adapter is a direct call; across processes it is DEC-131 item 5's notify-and-tail, where the executor
reads the agent stream from its folded position and journals `IntentReceived` — the durable path being
the journal either way. Nothing about the executor changes between the two, which is why the shell owns
the choice and this brief does not.

Stream I's tests PR has merged ([#134](https://github.com/kunwarshivam/mandate/pull/134)), so the
contract is code rather than prose and this brief pins it to what landed:

```rust
// mandate_runtime::ports — stream I's, not re-declared here.
pub trait IntentSink { fn hand(&mut self, handoff: &IntentHandoff) -> Result<(), SinkError>; }
pub enum SinkError { Unavailable, Refused { reason: String } }

// mandate_runtime::types
pub struct IntentHandoff { pub intent_id: EventId, pub body: IntentBody }
pub enum IntentBody {
    Order { instrument: InstrumentId, side: Side, qty: Qty, limit: Price, purpose: Purpose },
    Flatten(FlattenPlan),
}
```

Three things follow, and each is a test in this stream rather than a note:

- **`intent_id` is already the `IntentProposed` `event_id`**, as the merged doc comment says, so the
  idempotency chain's first link needs nothing added — the fold lookup of interpretation 8 is the whole
  of the deduplication.
- **A `Flatten` handoff is an intent like any other on the way in**, and the agent-scoped rules apply on
  the way out: cancel by client order id with each confirmed, sells of exactly the sub-ledger quantity,
  and no path to the account-wide endpoints (interpretation 18).
- **`SinkError` never carries a trading verdict.** `mandate-runtime` maps *either* variant to
  `RuntimeError::NotInterpreted { story: "E7-2" }`, which is this story, so the adapter returns `Ok`
  once `IntentReceived` has committed and `Unavailable` when it could not, and **never** `Refused` for a
  gate denial — a denial is a journaled `GateDecided`, not a sink failure. An adapter that answered
  `Refused` on a deny would turn every denied proposal into a runtime error in stream I.

The idempotency key is a type, not a convention:

```rust
/// The broker's name for one of our orders, and our only idempotency key on the broker side.
/// Derived from journaled facts alone; there is no constructor that takes a free string except the
/// parser that reads one back from the broker, and that one validates the grammar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientOrderId(String);

impl ClientOrderId {
    /// `md-<26 chars of the intent id>` for an order submitted for an intent. A pure function of
    /// the intent id, so every process, epoch, and restart derives the same value (E7-2).
    pub fn for_intent(intent: IntentId) -> Self;
    /// `md-r-<origin>` for the order that replaces one the broker replaced: derived from the
    /// `OrderStateChanged` event that recorded the replacement, never from a counter. The id
    /// carries no intent segment, because the derivation takes the origin event alone; the
    /// replacement is attributable to its intent through the order it replaced, which the fold
    /// links (`replaced_by`), not from the id by itself.
    pub fn for_replacement(origin: EventId) -> Self;
    /// `md-p-<origin>` for a protective order we submit on its own (the OCO after a partial fill,
    /// a re-placement before expiry, a crypto stop-limit): derived from the `ProtectionChanged`
    /// draft that records it, and attributable to its position through that draft.
    pub fn for_protection(origin: EventId) -> Self;
    pub fn parse(raw: &str) -> Result<Self, ExecutorError>;
    pub fn as_str(&self) -> &str;
}
```

`Ports` carries only pure collaborators, so `handle` stays deterministic:

```rust
/// Deterministic event identity (ES-06, ES-21, DEC-131 item 6): same (epoch, head, ordinal) in,
/// same id out, so a retry after `Unavailable` or `Ambiguous` re-derives it.
pub trait IdGen { fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId; }

/// Stream F's mandate view and stream G's instrument snapshot, read-only. `MandateView` is stream F's
/// type once `mandate-spec` lands; until then each layer-6 crate declares the narrow view it reads
/// (stream I already does), and the two converge on F's type in the implementation PR rather than one
/// same-layer crate importing the other's.
pub struct Ports<'a> {
    pub ids: &'a dyn IdGen,
    pub mandates: &'a dyn MandateView,
    pub instruments: &'a dyn InstrumentSnapshot,
}
```

Everything crossing a boundary is exact: quantities, prices, and money are `mandate-num` types,
timestamps are `mandate-time`, drafts are `mandate_journal::Draft` over `mandate_canon::Value`, and
every collection is a `BTreeMap` or `BTreeSet` (ES-21). Errors are one `thiserror` enum with a stable
`code()` per variant (ES-09), including `Unimplemented { story }` — the body of every stub in the tests
PR — and `NotInterpreted { what, story }`, the loud failure DEC-85 requires.

### `mandate-alpaca`: the paper client

The `mandate-marketdata` shape, deliberately: an injected transport, an injected clock, recorded
fixtures, and no network in any test (ES-19, and the
[E2-1 brief](E2-1-download.md)'s pattern).

```rust
/// One request against the paper trading host. The path is checked against the endpoint allowlist
/// before anything is sent, so no caller can reach a host or an endpoint this crate does not name.
pub trait TradingTransport {
    fn send(&self, request: &HttpRequest) -> impl Future<Output = Result<Response, TransportError>>;
}

/// The paper account's trading credentials, read through an injected lookup so a test never touches
/// the process environment. Held as `SecretString`; `Debug` prints neither value (AGENTS.md rule 7).
pub struct Credentials { /* private */ }

impl Credentials {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, CredentialsError>;
    pub fn from_env() -> Result<Self, CredentialsError>;
}

/// The client: one method per `BrokerRequest` variant, each returning a `BrokerOutcome` the
/// executor folds. It holds no state a restart would lose.
pub struct TradingClient<T, P> { /* private */ }
```

The host is a single constant for `https://paper-api.alpaca.markets`, and there is **no live base URL
in the crate at all** (ES-23: "Only the paper trading and data hosts are compiled in"). The endpoint
allowlist is the seven paths this stream needs (`/v2/orders`, `/v2/orders/{id}`,
`/v2/orders:by_client_order_id`, `/v2/positions`, `/v2/positions/{symbol}`, `/v2/account`,
`/v2/account/activities`), matched the way `mandate_marketdata::http::is_market_data_path` matches, so
a crafted symbol or id cannot change the host, add a header, or reach another endpoint.

Numbers arrive as raw text and are parsed by `mandate-num` (ES-23); a field the broker sends in
exponent form, with more places than the instrument's increment, or as a JSON number that has already
been through a float is a typed error with a stable code, never a rounded value.

## The idempotency design (E7-2)

This is the heart of the stream, so it is stated as a chain with no gaps.

1. **The intent's identity comes from the agent stream.** `intent_id` *is* the `event_id` of the agent
   stream's `IntentProposed` (journal §2). Nothing here mints it, so two runtimes cannot mint the same
   one and one runtime cannot mint two for one decision.
2. **`IntentReceived` is journaled first.** The handoff is at-least-once by design (DEC-131: a restart
   re-hands what the fold left owed), so the first thing `handle` does with `Input::Intent` is look the
   intent id up in folded state. Already present → the effect list is empty and the handoff is
   acknowledged; that is the whole of the executor's deduplication, and it is a fold lookup rather than
   a set held in memory.
3. **The binding gate runs next**, against fresh folded state, and `GateDecided` is journaled with the
   verdict, the first failing check's reason code, and the whole `checks` list (journal §9). A deny
   ends the intent; nothing is sent.
4. **The client order id is derived, not assigned.** `ClientOrderId::for_intent(intent_id)` is a pure
   function. It is not a counter, not a ULID minted at submission, and not a function of the attempt
   number — because trading §5.7 requires a resubmission after a confirmed absence to carry the **same**
   id, and an id that depended on the attempt could not.
5. **`OrderSubmitted` is journaled before the request leaves.** The draft carries the client order id,
   the attempt number, the intent id, and the exact order fields, and it is the effect immediately
   before `Effect::Broker(Submit(..))` in the same list. The shell appends first and submits only on
   `Committed` or `AlreadyCommitted` (ES-06), so the ordering is structural: there is no code path that
   can submit without the draft, and a property asserts it over every input.
6. **The broker carries the key.** The submission sets Alpaca's `client_order_id` to ours. The broker
   rejects a duplicate `client_order_id`, so even a duplicate request that escaped every check above
   cannot become a second order; that rejection is folded as "already submitted", not as a failure.
7. **A crash between the journal and the request is the whole point.** On restart the fold sees
   `OrderSubmitted` with no acknowledgment. `Input::Started` does **not** resubmit. It emits a
   `GetOrderByClientId` for each such order:
   - the broker **has** it → adopt the broker's state with `OrderStateChanged` (and
     `CompensatingEvent` if it differs from what we assumed), apply any fills, and continue;
   - the broker **does not have it** → the order enters `Unknown`, and it stays there until absence is
     confirmed `unknown_absent_lookups` times over `unknown_absent_window_s` (trading §5.7's "confirmed
     absent after N lookups over T seconds"). Only then does it return to `Intent`, the gate re-runs,
     and it is resubmitted **with the same client order id** if the gate allows and the intent is younger
     than `max_intent_age`; otherwise `OrderAbandoned`.
   - while an order is `Unknown`, its maximum cost stays reserved, it counts as filled for exposure and
     concentration, and no new order is submitted in that instrument (§5.3 rule 9).
8. **A crash between the request and the response is the same case**, reached from `Input::Broker(Err)`
   instead of from a restart: the outcome is unknown, so the order goes `Unknown` and the same lookup
   discipline applies. The executor never treats a timeout as a rejection.
9. **A crash between the response and its journal entry is covered by the query too**, because the
   authority for an order's state is the broker, not our memory of what it said.
10. **Journal-level retries are separate and do not interact.** A `Unavailable` or `Ambiguous` append is
    retried with the same drafts, whose `event_id`s are derived from `(epoch, head, ordinal)` (DEC-131
    item 6), so the retry answers `AlreadyCommitted`. No new input is handled at an unresolved head. A
    `Fenced` append stops the process: a newer epoch owns the stream and this one is a ghost.
11. **Nothing else may name an order.** There is no `ClientOrderId::new(String)`. The three derivations
    and the validating parser are the only ways to obtain one, and each derivation's input is an event
    id, which the journal guarantees unique. That is what makes "a client order id reused across
    restarts" a planted bug rather than a possibility.

## Reconciliation (E7-3)

One pure function over a snapshot, the folded state, and the account ledger, returning drafts and a
verdict. It runs at startup, after any `Unknown` order, at each session boundary, after fee posting,
and on a schedule (trading §11). The order of the steps is part of the algorithm:

1. **Open orders, by client order id.** Compare the set and the state. The broker wins: every
   difference is adopted with `OrderStateChanged` plus a `CompensatingEvent` naming the difference and
   the corrected event ids. An order the broker has and we do not is **external activity**:
   `ExternalActivityIngested`, every agent on the account to `exits_only` until the owner acknowledges,
   and the instrument blocked from being claimed (§7.1). A reject for an unknown client order id counts
   only toward the 403 threshold and is not external activity unless it carries a fill.
2. **Fills since the checkpoint, by fill id.** Ingest every missing one as `FillApplied`, or
   `LateFillApplied` where its order is already terminal, which itself schedules another
   reconciliation. Fills are always applied to accounting (§5.7).
3. **Positions.** Compare the ledger's net position — *after* step 2, which is why the order matters —
   against the broker's: equities exact except an instrument under `pending_corporate_action`; crypto
   model net plus unposted asset fees until posting, exact after. A difference is a **mismatch**:
   `AgentModeApplied(paused)` for every agent holding that instrument, an alert, and the difference
   recorded. Comparing positions before ingesting the fills would pause an agent that was merely behind,
   which is planted bug 8.
4. **Cash.** Tolerance is 0.01 × fills since the last broker cash snapshot, plus accrued unposted fees;
   exact after posting. Above the threshold alerts; persistently above it pauses. Paper's simulated fees
   and dividends are `simulated = true` in the shadow ledger and are **excluded** from this comparison
   while remaining in P&L and buying power (§10).
5. **Fees.** Exact once posted. A difference alerts and is never silently adjusted.
6. **`ReconciliationRun` last**, carrying what was compared, every difference, the corrected event ids,
   and the checkpoint it advances to. Its **position** in the stream is what lifts stream I's startup
   hold (DEC-131 item 13), and this stream carries the whole of that contract: **the executor never
   appends a `ReconciliationRun` positioned after a submission it did not cover.** The run is appended
   with `expected_head` equal to the head the snapshot was taken at, so an `OrderSubmitted` that landed
   in between makes the append answer `HeadMismatch` and the reconciliation is recomputed against a
   fresh snapshot rather than published as covering something it never saw. The payload still records
   the high-water mark, as the audit record of what was compared, but **stream I does not have to read
   it**: positional is enough, which is why this needs no change on I's side (review round 1,
   finding 6).

**A mismatch pauses and alerts, and the executor never lifts it.** Resuming a paused agent requires an
owner acknowledgment with step-up authentication (§11), which arrives as a copied `OwnerAcknowledged`
on the account stream. Nothing in this crate clears a mismatch on its own, at any tick, on any later
reconciliation that happens to agree.

**Adoption is scoped to the order set, because that is the only row §11's on-mismatch column adopts.**
An order-state difference is adopted (with a compensating event) and a missing fill is ingested; a
position, cash, or fee difference is **never** written away. Positions pause the agents holding the
instrument, cash alerts above the band and pauses when persistent, and fees are alerted and never
silently adjusted. Writing a compensating event for a position difference would make the ledger agree
with the broker while destroying the evidence that they disagreed, which is the opposite of what §11
asks for (review round 1, finding 1).

**Adopting an order is never "trusting the broker over the journal" in the audit sense**: the journal
keeps both, the adoption is itself a journaled event with the difference, and a replay reproduces the
adoption rather than the original assumption. The failure mode this rules out is the opposite one —
keeping our own order state because it is ours — which is planted bug 3.

## Fault injection (E7-3's acceptance clause)

E7-3 is accepted when "fault injection at every submission step yields zero duplicates and full
reconciliation". "Every step" is enumerated so the claim is checkable rather than rhetorical. Each point
is a named position in one submission's effect pipeline; each has a `fault::crash_at_*` test that drops
the state at that point, replays the fold from the journal, runs `Input::Started`, drives the
reconciliation, and asserts the four properties below.

| # | Crash point | Test | What a wrong executor does here |
|---|---|---|---|
| 1 | Before the `IntentReceived` append | `fault::crash_at_before_intent_received` | Loses the intent silently, or re-journals it twice on the re-hand |
| 2 | After `IntentReceived`, before the binding gate | `fault::crash_at_intent_received_before_gate` | Submits on the re-hand without gating |
| 3 | After `GateDecided`, before the `OrderSubmitted` append | `fault::crash_at_gate_before_order_submitted` | Submits an order the journal never named |
| 4 | **After the `OrderSubmitted` append, before the request** | `fault::crash_at_journal_before_request` | Blindly resubmits: the duplicate window R-03 names |
| 5 | During the request, no response (timeout) | `fault::crash_at_request_no_response` | Treats the timeout as a rejection and submits again |
| 6 | During the request, an ambiguous response (5xx after the broker accepted) | `fault::crash_at_request_ambiguous_response` | Same, with a live order at the broker |
| 7 | After the response, before the `OrderStateChanged` append | `fault::crash_at_response_before_state_change` | Re-derives state from memory instead of querying |
| 8 | After `OrderStateChanged`, before the fill append | `fault::crash_at_state_change_before_fill` | Double-applies the fill, or loses it |
| 9 | After the protective cancel request, before its confirmation | `fault::crash_at_cancel_before_confirmation` | Submits the exit into a still-resting OCO (a self-cross reject) |
| 10 | Between the cancel confirmation and the exit submit | `fault::crash_at_confirmation_before_exit_submit` | Leaves the position unprotected with no journaled interval |
| 11 | Between an entry fill and the OCO for the filled quantity | `fault::crash_between_entry_fill_and_oco` | Leaves a partially filled tranche unprotected for ever |
| 12 | During reconciliation, between adopting an order and appending its compensating event | `fault::crash_mid_reconciliation_before_the_compensating_event` | Adopts twice, or leaves a mismatch unpaused |

Asserted at every point:

- **Zero duplicates.** The fake connector's independent counter shows at most one accepted submission
  per client order id, and the journal holds at most one `OrderSubmitted` per `(intent_id, attempt)`
  with no repeated `event_id`.
- **Full reconciliation.** After the run, every broker order is matched or adopted with a compensating
  event, every fill missing from the journal is ingested, and every remaining position difference has
  paused the agents holding that instrument, with the cash and fee rows alerted per §11. Nothing is
  unexplained and unpaused — and nothing outside the order set is adopted.
- **Protection is accounted for.** Every unprotected interval the protection accountant finds has a
  journaled start and end, and none exceeds `max_unprotected_s` without an alert.
- **The fold is faithful.** Replaying the drafts reproduces the state the run ended in, and the replay
  emits no draft and no broker request.

A property test then generates a random sequence of intents, fills, cancels, and restarts with a random
crash point and asserts the same four, with the submission multiset as its oracle. The fake connector
is scripted, in-process, and deterministic; no test touches a network (ES-19).

## Protective exits, exits, and kill switches (E7-4)

The spec gives the sequences; this stream's contribution is that each is one effect list from one
`handle` call, so a crash inside a sequence leaves the journal saying where it stopped.

- **The tranche model.** Each protected entry is one GTC bracket. An add is a new bracket, never a
  replacement, and Σ protective sell quantity ≤ position is a property, not a check.
- **Partial bracket entries.** Legs are held until the entry is complete. The unprotected interval
  starts at the first partial fill; at `bracket_partial_fill_timeout` (default 60 seconds, and always
  before the closing auction window) the entry remainder is cancelled, confirmed, and a GTC OCO for the
  filled quantity is submitted at the bracket's prices. The same OCO follows a terminal partly filled
  entry.
- **The marketable exit sequence.** Cancel every protective order in the instrument → await
  confirmation → re-run the gate on fresh state → submit the exit, priced per §5.6 → on a terminal
  state, re-place protection for the remainder. Nothing is submitted while an unconfirmed cancel is
  outstanding, and an order submitted while protection is cancelled is marketable at submission.
- **Passive exits keep the stop.** A sell limit above the bid becomes the take-profit leg of a new OCO
  that keeps the existing stop; protection is never removed for it.
- **Bounded unprotected intervals.** Every interval is journaled from start to end
  (`ProtectionChanged`). At `max_unprotected_s` (default 60 seconds) with the order unfilled, the order
  is cancelled, confirmed, protection re-placed for the held quantity, and the owner alerted.
- **Expiry.** GTC expires 90 calendar days after creation; protection is re-placed (cancel, confirm,
  new OCO) at the first `TradingDayStarted` on or after the day that is
  `protective_replace_buffer_trading_days` trading days before expiry.
- **The triggered-stop watchdog.** A sane risk mark at or below a resting stop's price for
  `stop_watchdog_s` in a session where the stop can trigger, with no fill, or below a stop-limit's limit
  price, cancels, confirms, exits the held quantity through the ladder as a `risk_exit`, and alerts.
- **Crypto.** One GTC stop-limit for the whole position, limit = stop × (1 − `crypto_stop_limit_offset`)
  from the mandate. Take-profit is the runtime's to watch. Adds are limit IOC inside the cancel →
  confirm → act → re-place sequence; exits are marketable. A gap that a stop-limit misses is disclosed,
  not compensated.
- **Fractional positions.** Only the whole-share part is protected; the fraction is unprotected and
  disclosed.
- **The exit price ladder (§5.6).** Reference bid from a fresh sane quote, else the last sane bid within
  five minutes, else the last trade. Step at `exit_step_s` (default 5 seconds) with the offset raised by
  `exit_offset_step`, repricing from the current reference. The offset never exceeds `max_exit_offset`;
  at the floor the order rests and the owner is alerted. The three tiers are data, transcribed with the
  fee configuration.
- **Kill switches (§5.5).** The mode first, then cancel, then confirm, then close, each step journaled.
  Account and workspace scopes use the broker's `cancel-all` (including `Unknown` orders) and
  `close-position`; the agent scope cancels only that agent's orders by client order id, confirming
  each, and sells exactly the agent's sub-ledger quantity. Automated (`risk_exit`) sells wait for the
  regular session for equities, leaving protection in place until then, and go at once for crypto. Owner
  (`owner_exit`) sells outside the session need the confirmed bid, bid size, and floor from the agent
  stream's `OwnerExitRequested`; without them equity sells wait for the session. Kill-switch orders are
  exempt from the agent's mode, never wait for an approval, and never depend on model state.
- **Risk reduction is never denied** by conduct controls, eligibility, day-trade budgets, buying power,
  or opening-session rules (AGENTS.md rule 13). Owner exits are paced only by participation caps;
  discretionary exits are paced by conduct controls and wait for the regular session for equities, but
  are never denied. The only holds on an exit are agent mode `paused` or `stopped`, an `Unknown` order in
  the same instrument, and the broker.

## The fixture plan

Fixtures follow `mandate-marketdata`'s recorded-scenario shape, **adapted for a write API**, so a real
recording can replace a hand-built body without touching a test:
`crates/mandate-alpaca/tests/fixtures/alpaca-trading/<scenario>/requests.txt` and `response-N.json`
beside it. Three differences from the market-data shape are deliberate and are what the adaptation is
for (review round 1, finding 8): a `requests.txt` line carries the **method and the canonical request
body** as well as the path and query, because a trading call is identified by what it sends and not only
by where it sends it; the response files are `response-N.json` rather than `page-N.json`, because these
endpoints answer with one object and not a page of a cursor walk; and the transport trait is
`TradingTransport::send` rather than `Transport::get`, for the same reason. The recording script is
therefore its own, `crates/mandate-alpaca/tests/fixtures/record.sh`, beside market data's rather than
shared with it, and it records the method and body lines the fake transport asserts against.

**In the tests PR, every fixture is hand-built** from the spec's own vocabulary — trading §5.2's
capability matrix, §5.7's status table, §7.2's account fields, §7.3's reject signals, §6.1's fill record
— and computed from the spec's rules rather than typed from memory. Scenarios:

| Scenario | What it pins |
|---|---|
| `submit_limit_accepted` | The canonical submission body, our `client_order_id` on the wire, and `new` → Accepted |
| `submit_bracket_accepted` | Bracket legs, one shared TIF, no extended hours |
| `submit_oco_accepted` | An OCO for a filled quantity at the bracket's prices |
| `submit_crypto_stop_limit` | One simple GTC stop-limit, limit derived from the stop |
| `submit_rejected` | `rejected` with a reject code, and the code's restriction mapping |
| `submit_duplicate_client_order_id` | The broker refusing our own id, folded as "already submitted" |
| `submit_timeout_then_found` | A transport error, then the query finding the order |
| `submit_timeout_then_absent` | A transport error, then N absences over the window |
| `order_by_client_id_found`, `order_by_client_id_absent` | The recovery query, both answers |
| `cancel_confirmed`, `cancel_rejected_already_filled` | `pending_cancel` → `canceled`, and the filled-first case |
| `replace_pending_then_replaced` | Broker-initiated replace: the old order live, the new one linked |
| `partial_then_filled` | Per-fill quantities and prices, cumulative agreement |
| `late_fill_after_terminal` | A fill for a terminal order |
| `open_orders_page`, `positions`, `account_active`, `account_blocked`, `activities_fills` | The reconciliation snapshot's four sources |
| `status_unrecognised` | A status outside the table, which must pause and alert rather than parse |
| `cancel_all_account_scope` | The account-wide `cancel-all`, `Unknown` orders included, with each cancellation confirmed |
| `close_position_account_scope` | The account-wide `close-position` per instrument, the only path that may use it |

Every fixture is asserted to contain no credential, no broker account number, and no personal data, the
way E2-1's `fixtures::recorded_fixtures_contain_no_credentials` does. The parser accepts unknown extra
fields (a later recording carries more than a hand-built body), while an unknown **status** value still
fails loudly (§5.7's last row), so the permissiveness is exactly where it is safe.

**The tests PR's contract is the hand-built fixtures**, computed from the spec, so the suite is
reviewable and runs with no network whatever the environment has. Real recordings then *replace* those
bodies scenario by scenario, and a test that fails at that point is a test that was wrong about the
broker, which is the whole reason for the shape.

The coordinator's round-1 review reports that the cloud environment now **does** have the paper keys and
egress to the paper trading host, so a recording pass is possible in the tests stage rather than only
later on the founder's machine (finding 8). This brief keeps it out of the tests PR's required scope and
records it as the coordinator's call (Decisions needed 7): the fixtures the tests PR must contain are
the hand-built ones, and a recording pass is an addition on top. Whenever it happens, the same rules
hold without exception — the credentials are read through the injected lookup, never printed, never
logged, never written to a file, and never committed; the recorded bodies have every account number and
account id replaced before they are saved; and `cargo xtask ci supply-chain` and the fixture scan are
what prove it (AGENTS.md rules 7 and 8, ES-23: paper only).

## Credentials and the paper boundary

Phase 1 connects with the account owner's **own paper credentials**, read from the process environment
([milestones](../02-milestones-and-wbs.md) M6: "API keys for the founder's own account in Phase 1;
OAuth arrives in M8"). This code therefore:

- reads them through an injected lookup (`from_lookup`), so no test reads the environment and a
  deployment can supply them from anywhere;
- holds them as `SecretString` and never implements a `Debug` that could print one (ES-09, AGENTS.md
  rule 7);
- **never stores, writes, journals, or logs them**: not in a draft, not in a `BrokerExchangeRecorded`
  payload (which redacts the authorisation headers before recording), not in an error message (a
  transport error carries no URL, header, or body, as `mandate-marketdata` already does), not in a
  fixture, and not in a file this code creates;
- names the broker's account number nowhere: the account stream's subject is an opaque `account_ref`
  ULID and the number lives in the personal-data vault (journal §6.4);
- compiles in the paper host only, with no live base URL present in the crate at all, and a `live`
  cargo feature is forbidden (ES-23). Live credentials would come only from the vault, never from the
  environment, and that path is M13's, not this stream's.

**Live accounts, OAuth, and fund movement are out of scope.** Nothing in this stream requests an OAuth
scope, stores a token, reaches a live endpoint, or moves money; there is no deposit, withdrawal, or
journal-transfer request in `BrokerRequest` and no code path that could build one (AGENTS.md rule 8,
[compliance](../../product/08-compliance-and-regulatory.md): no custody of funds).

## Interpretations (recorded as DEC-133)

Each item fixes how code realises a rule the specs already state. Item 1 is the only one that edits
founder-owned files; the items under "Decisions needed" that would add a number to a spec table stay
`Proposed (founder)`.

1. **Two crates, both safety-critical: `mandate-executor` at layer 6 and `mandate-alpaca` at layer 7**
   (amended by PR #152 review round 1). `mandate-executor` (`pure = true`,
   `allowed_external = ["thiserror"]`) and `mandate-alpaca` (`pure = false`,
   `allowed_external = ["reqwest", "rustls", "secrecy", "serde", "serde_json", "thiserror", "tokio"]`),
   each with a CODEOWNERS line, exactly as ES-02 plans them. The split is the one ES-02 already names,
   and it is load-bearing: the executor's decisions are reviewable without reading an HTTP client, and
   the client cannot see folded state. `xtask/layers.toml` and `CODEOWNERS` are founder-owned, so this
   item is the one the decision cannot take alone.
2. **Two entry points, and recovery is an input.** `fold` replays one account-stream event and produces
   no effects; `handle` is the only producer of effects (ES-06); recovery is `Input::Started`, which
   opens the startup reconciliation and resolves unacknowledged submissions by **query**, never by
   resubmission. This is DEC-131 item 2's shape, deliberately, so the two M5/M6 state machines read the
   same way.
3. **One writer, one stream, and the executor writes *everything* on the account stream.** The executor
   is the account stream's single writer (journal §2) and reads the agent streams it follows without
   writing to them. That makes it the appender of three groups, not one, and the earlier draft named
   only the first (review round 1, finding 7):
   - **Copied facts** — `AgentModeApplied` (from the agent stream's `AgentModeChanged`),
     `TradingDayStarted`, `ClockAdvanced`, and `OwnerAcknowledged` (from the control stream) — each with
     `causation_id` pointing at the original, and `RiskDayStarted` **derived** there from the copied
     `ClockAdvanced` crossing midnight America/New_York, as journal §2 requires.
   - **The risk-state records** — `MandateVersionApplied`, `RiskLimitTriggered`, `RiskLimitLifted`,
     `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, and `GoalCompleted`
     (journal §9's account table, mandate spec §5.10) — journaled by the executor as the account-stream
     record of a fold that is **stream F's**, called as a library exactly as the gate is. The executor
     owns the append; it does not own the arithmetic.
   - **`UniverseChanged`** — copied from the agent stream's `ThesisProposed` or `ThesisRevised`, or from
     a `MandateVersionApplied` that changed a pinned universe (journal §2, mandate spec §2.3). The
     admission decision is **stream J's**; the append is the executor's.

   Every risk input it appends carries the `risk_clock` the stream requires, never decreasing. Which of
   the three groups this stream's tests PR interprets, and which arrive with F and J, is Decisions
   needed 8; until then `fold` returns `NotInterpreted` naming the owning story for any of them it does
   not yet interpret, which is item 27 doing its job rather than a gap.
4. **The binding gate is a dependency, not a port.** The executor calls `mandate-risk` directly. It is
   not injected, not behind a trait, and not replaceable by configuration, because a binding gate that
   a caller can substitute is not independent of agent logic (AGENTS.md rule 1; journal §2: "the risk
   gate is a pure library it calls"). Stream I's injected `GateDryRun` is the *advisory* call and can
   only narrow (DEC-131 item 4); this one decides. Until stream G's crate exists the call site is a
   crate-private function returning `Unimplemented`, and the dependency is added in the implementation
   PR — a workspace crate, so `docs/dependencies.md` is unaffected.
5. **The client order id is derived from the intent id alone.** Not from a counter, not from a fresh
   ULID, and **not from the attempt number**, because trading §5.7 requires a resubmission after a
   confirmed absence to carry the same id. The attempt number is journaled for the audit trail and
   changes nothing the broker sees.
6. **There are only three derivations and one validating parser.** `for_intent`, `for_replacement` (from
   the `OrderStateChanged` that recorded a broker-initiated replacement), and `for_protection` (from the
   `ProtectionChanged` that records a protective order we submit on its own). There is no
   `ClientOrderId::new(String)`. Every derivation's input is an event id, which the journal guarantees
   unique, so an id reused across restarts is unrepresentable rather than merely forbidden.
7. **Write-before-acting is structural, not a convention.** Every `Effect::Broker` that submits follows
   the `OrderSubmitted` draft that names it in the same effect list, and the shell submits only on
   `Committed` or `AlreadyCommitted` (ES-06). A property asserts the ordering over every input, and the
   effect list is the only way a request can be made.
8. **Deduplication is a fold lookup.** An intent whose id the fold already carries produces an empty
   effect list and an acknowledged handoff, so `IntentSink`'s at-least-once delivery (DEC-131 item 22's
   re-hands) costs nothing. No in-memory set, no time window, nothing a restart could lose.
9. **An unacknowledged submission is resolved by query, with absence confirmed over a window.** The
   order enters `Unknown` and returns to `Intent` only after `unknown_absent_lookups` consecutive
   absences spanning at least `unknown_absent_window_s` (trading §5.7's "N lookups over T seconds"),
   then the gate re-runs. One absent answer never resubmits: a broker whose read replica is behind would
   otherwise be enough to double an order.
10. **A timeout is never a rejection.** An unknown outcome is `Unknown`, with its reservation held, its
    quantity counted as filled for exposure and concentration, and no new order in that instrument
    (§5.3 rule 9) until it resolves. Treating silence as a rejection is how a duplicate is born.
11. **`Abandoned` is exactly the spec's two cases, and the age check guards every submission.** §5.7
    abandons on a **gate re-check** denial or an intent that is too old. The re-check wording matters:
    a *first-pass* deny never produces an order in the `Intent` state at all, it is a journaled
    `GateDecided(deny)` with nothing sent and nothing to abandon (review round 2, nit 4). The age half
    is checked at **every** `Intent → Submitting` transition, not only at a resubmission: §5.7's
    diagram states `Intent --> Abandoned: intent too old` unqualified, and an intent re-handed after a
    long outage would otherwise be gated and sent at its *first* submission however stale, since crash
    points 1 and 2 leave it with no submission behind it (review round 1, finding 2). Nothing else
    abandons an intent, and an abandoned intent is never re-sent: `OrderAbandoned` is terminal, and a
    later handoff of the same intent id hits item 8's lookup and produces nothing.
12. **Reconciliation's step order is part of the algorithm:** orders, then fills, then positions, then
    cash, then fees, then `ReconciliationRun`. Positions are compared only after the missing fills are
    ingested, because a position difference that a missing fill explains is not a mismatch, and pausing
    on it would pause a healthy agent at every restart.
13. **Adoption is scoped to the order set, which is the only row §11 adopts.** An order-state
    difference resolves to the broker's value through `OrderStateChanged` plus a `CompensatingEvent`
    naming what changed and which event ids were corrected, and a missing fill is ingested. A
    **position, cash, or fee difference is never adopted**: §11's on-mismatch column pauses the agents
    holding the instrument, alerts above the cash band and pauses when persistent, and refuses to
    adjust a fee silently. Writing a compensating event for a position difference would make the ledger
    agree with the broker while destroying the evidence that they disagreed (review round 1,
    finding 1). Within the order set, our own state is never kept because it is ours; the journal keeps
    both, and a replay reproduces the adoption.
14. **A mismatch pauses and alerts, and this crate never lifts it.** `AgentModeApplied(paused)` plus an
    alert, lifted only by a copied `OwnerAcknowledged` carrying step-up evidence (§11). No tick, no
    later agreeing reconciliation, and no restart clears it.
15. **A `ReconciliationRun` is never positioned after a submission it did not cover**, so stream I's
    hold stays positional (a run at or after the last submission by `seq`, DEC-131 item 13) and stream I
    needs no change and no payload read. The run is appended with `expected_head` equal to the head its
    snapshot was taken at, so an `OrderSubmitted` that landed in between answers `HeadMismatch` and the
    reconciliation is recomputed against a fresh snapshot instead of being published as covering
    something it never saw. The payload still records the high-water mark, as the audit record of what
    was compared — not as a field another stream must read (review round 1, finding 6).
16. **External activity is defined by the broker's side.** An order or fill at the broker with no
    `client_order_id` of ours is external: `ExternalActivityIngested`, every agent on the account to
    `exits_only` until the owner acknowledges, and the instrument blocked from being claimed (§7.1). A
    reject naming a `client_order_id` we do not know counts only toward the 403 threshold and is not
    external activity unless it carries a fill (§7.3's own sentence).
17. **Account restrictions originate here.** `AccountStateObserved`, `RejectObserved`, and
    `AccountRestrictionChanged` are account-stream events this crate writes from statuses and rejects
    (§7.3), and the agent effect is journaled as `AgentModeApplied`, which the runtime then copies. The
    executor never infers a mode from a single broker push without the restriction it implies.
18. **The account-wide endpoints are unreachable from agent-scoped code.** `CancelAll` and
    `ClosePosition` take an `AccountWideScope` that only the account and workspace kill-switch paths can
    construct, so AGENTS.md rule 13 is a type rule rather than a review rule. `RC-14`'s
    `cancel_all_orders: agent` and `close_all_positions: agent` are scoped actions — every order of that
    agent by client order id, each confirmed, and exactly the agent's sub-ledger quantity — not those
    endpoints.
19. **The executor's half of mandate §5.9 is the orders.** On entering `exits_only` or stricter it
    cancels the agent's working **opening** orders and leaves protective orders resting; the pending
    approvals are the runtime's half, because the agent stream has a single writer (DEC-131 item 23).
    Neither side does both, and this brief and #126 agree on the split in writing.
20. **A sequence is one effect list.** Cancel, confirmation, gate re-run, submit, and re-placement are
    emitted as one ordered list from one `handle` call, so a crash inside a sequence leaves the journal
    saying exactly where it stopped, and `Input::Started` resumes from that point rather than restarting
    the sequence.
21. **Every unprotected interval is an interval, not a flag.** `ProtectionChanged` records its start and
    its end; the bound is enforced at the tick that reaches `max_unprotected_s`; and the alert is part of
    the same effect list as the cancel and the re-placement, so an interval cannot be closed by
    forgetting about it.
22. **The parameters the spec names without a value are read from configuration, not hard-coded**, and
    this brief proposes the values (Decisions needed 3). The executor reads them from the effective-dated
    configuration the gate already reads, so changing one is a configuration change with a content hash
    in `config_refs`, not a release.
23. **Broker numbers never pass through a float** (ES-23): every quantity, price, and amount is read as
    raw text and parsed by `mandate-num`. A value in exponent form, with more places than the
    instrument's increment, or already rounded by a JSON float is a typed error with a stable code.
24. **`BrokerExchangeRecorded` is redacted before it is recorded, for credentials *and* for personal
    data**, inline while small and by `sha256:` artifact reference above the inline limit (journal §6.3,
    DEC-107). Trading §13 keeps raw broker requests and responses as records, and AGENTS.md rule 7 keeps
    credentials out of them — but the raw body of `/v2/account` also carries the broker's
    `account_number` and account `id`, which journal §6.4 keeps in the vault and holds by reference
    (review round 1, finding 3). So the redaction pass runs over the body as well as the headers: the
    authorisation headers are removed, and every personal-data field is replaced by an opaque `pii_refs`
    entry, **before** the bytes are hashed or stored, so nothing that reaches the journal or the
    artifact store has ever held either. The pass is a total function over the wire types rather than a
    denylist of field names — only a field the wire type names is recorded at all — so a field the
    broker adds later cannot slip through unredacted.
25. **Paper's simulated fees and dividends are marked, not hidden.** They are booked with
    `simulated = true`, excluded from the cash and fee comparisons in reconciliation, and included in
    reported P&L and in buying power (§10, R-22). A simulated record that reached a cash comparison would
    make every paper reconciliation fail; one that was left out of buying power would make paper flatter
    than live, which is the risk R-22 names.
26. **Reservations are folded state, released by §5.7's whole terminal set.** An `Unknown` order
    reserves its maximum cost; a reservation is released by any of `Filled`, `Canceled`, `Rejected`,
    `Expired`, `Replaced`, and `Abandoned` — the six terminal states §5.7's diagram names, not the four
    broker statuses the earlier draft listed. `Abandoned` and `Replaced` matter most: an `Unknown` order
    confirmed absent and then abandoned, and an order the broker replaced, would otherwise hold their
    reservation for ever and starve the account's buying power (review round 1, finding 5). A
    `Replaced` order's reservation passes to the linked new order rather than vanishing, so the pair
    never double-reserves and never under-reserves. Nothing else releases a reservation: not a timeout,
    not a restart, not a reconciliation that merely disagrees. Buying power is the lower of the model
    and the broker (§7.2, DEC-34, DEC-104).
27. **An uninterpreted input fails loudly** (DEC-85): an event type, a payload field, a broker status, or
    a command this crate does not interpret returns `NotInterpreted` naming the owning story. A broker
    status outside §5.7's table pauses the agent and alerts, which is the spec's own last row.
28. **Alerts carry opaque IDs and a message key only** (AGENTS.md rule 6, DEC-11), and a test walks every
    payload this crate can build.
29. **Determinism is asserted, not assumed** (ES-21): `BTreeMap` and `BTreeSet` only, no floats, no clock,
    no randomness, exact `mandate-num` arithmetic, `fold_version` 1 with a committed golden journal, and a
    property that two runs of the same inputs give equal effect lists. The only time the core knows is the
    risk-clock second on its input.
30. **The priority channel is the shell's, and the journaled order is the handling order** (ES-06): a
    kill-switch or risk-exit command is read before a full queue of ordinary inputs, and what gets
    journaled is the order in which `handle` saw them, so a replay reproduces the priority.
31. **ES-24's budget is measured over the gate-and-draft step alone** — from the input to the order bytes
    being ready, excluding the append and the broker round trip. The implementation PR reports the
    measurement; the tests PR does not benchmark.

## Planted bugs

Twenty-one, each to be seeded alone in a throwaway implementation of the stubs (kept out of the tests PR
per DEC-83), run, and reverted. The last five come from review round 1's findings, which is what makes
those fixes testable rather than merely written down. The tests PR reports the result for each; a row whose bug is not caught
means the test is wrong, not the bug.

| Planted bug | Must be caught by |
|---|---|
| A restart resubmits an order whose `OrderSubmitted` committed, without querying the broker first, so a crash in the submission window doubles the order | `fault::crash_at_journal_before_request`, `hand::an_unacknowledged_submission_queries_before_it_resubmits`, `properties::no_crash_point_makes_the_broker_see_two_orders_for_one_intent` |
| The intent is journaled after the connector call rather than before, so a crash between them sends an order the journal never recorded | `properties::every_submit_effect_follows_the_order_submitted_draft_that_names_it`, `hand::a_submission_journals_before_the_request_leaves`, `fault::crash_at_request_no_response` |
| Reconciliation keeps our own order state when it differs from the broker's, journaling nothing, so the journal and reality diverge silently | `properties::every_order_difference_adopts_the_broker_with_a_compensating_event`, `hand::a_journal_only_order_is_reconciled_away_not_kept`, `hand::every_adoption_journals_a_compensating_event_with_the_difference` |
| A bracket entry that fills partly keeps waiting for the rest, so the filled quantity is left with no OCO and no journaled interval | `hand::a_partly_filled_bracket_becomes_an_oco_for_the_filled_quantity`, `fault::crash_between_entry_fill_and_oco`, `properties::every_unprotected_interval_has_a_journaled_start_and_end` |
| A cancel is treated as done when the request is accepted rather than when the broker confirms, so the exit is submitted into a still-resting OCO | `hand::an_exit_never_submits_before_the_cancel_is_confirmed`, `properties::no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding`, `fault::crash_at_cancel_before_confirmation` |
| An abandoned intent is re-sent when the runtime re-hands it after a restart, so an order the gate refused reaches the broker | `hand::an_abandoned_intent_is_never_re_sent`, `hand::a_gate_denial_on_re_check_abandons_the_intent`, `properties::distinct_intents_never_share_a_client_order_id` |
| The client order id is derived from the intent id **and the attempt**, so a resubmission after a confirmed absence carries a new id and the broker ends up with two orders for one intent | `hand::a_resubmission_after_a_crash_reuses_the_same_client_order_id`, `properties::a_client_order_id_is_a_function_of_the_intent_id_alone` |
| The client order id is a per-process counter, so a restart reuses an id for a different intent and the broker rejects the second order for ever | `hand::two_processes_derive_one_client_order_id_for_one_intent`, `hand::a_new_intent_after_a_restart_derives_a_fresh_client_order_id` |
| A position difference is recorded in `ReconciliationRun` but the agent is left running, so an agent trades against a position it does not have | `hand::an_unexplained_position_pauses_the_agent_and_alerts`, `properties::a_reconciliation_leaves_nothing_unexplained_and_unpaused` |
| Positions are compared before the missing fills are ingested, so every restart pauses a healthy agent and the real mismatch is lost in the noise | `hand::a_missing_fill_explains_the_position_and_pauses_nothing`, `properties::the_reconciliation_order_is_orders_then_fills_then_positions_then_cash_then_fees` |
| A transport timeout is folded as a rejection, so the reservation is released and the order is resubmitted while the first one is live at the broker | `hand::a_timeout_is_not_a_rejection`, `fault::crash_at_request_ambiguous_response`, `properties::a_reservation_is_never_released_before_a_terminal_state` |
| The agent-scoped kill switch reaches the broker's `cancel-all`, so another agent's orders and the owner's unattributed shares are cancelled | `hand::an_agent_kill_switch_cancels_only_that_agents_orders`, `properties::no_agent_scoped_effect_can_name_the_account_wide_endpoints` |
| The kill switch cancels and sells before applying the final mode, so an intent handled in the same batch is gated under the old mode | `hand::a_kill_switch_applies_the_mode_before_it_cancels`, `properties::the_mode_draft_precedes_every_cancel_and_every_sell` |
| An unprotected interval's start is journaled but the `max_unprotected_s` bound is never checked, so a stuck exit leaves a position unprotected indefinitely with no alert | `hand::an_unprotected_interval_at_the_limit_cancels_re_places_and_alerts`, `properties::no_interval_exceeds_the_limit_without_an_alert` |
| A broker status outside §5.7's table folds as a no-op, so a suspended or unrecognised order silently stays Accepted | `hand::an_unknown_broker_status_pauses_the_agent_and_alerts`, `properties::the_status_map_is_total_and_never_silently_ignores` |
| A fill is applied by arrival rather than by fill id, so a reconciliation that re-ingests a fill double-counts the position | `properties::filled_quantity_equals_the_sum_of_unique_fills`, `hand::a_repeated_fill_id_changes_nothing`, `fault::crash_at_state_change_before_fill` |
| A `ReconciliationRun` is appended at the current head rather than the snapshot's, so a submission that landed in between is positioned before a run that never saw it and stream I's startup hold lifts on unverified state | `properties::no_reconciliation_run_is_appended_after_a_submission_it_did_not_cover`, `hand::a_submission_between_the_snapshot_and_the_run_recomputes_the_run` |
| A position difference is written away as a `CompensatingEvent`, so the ledger agrees with the broker and the evidence that they disagreed is gone — and the agent is never paused | `properties::no_position_cash_or_fee_difference_is_ever_adopted`, `hand::a_position_difference_is_not_written_away_as_a_compensating_event`, `hand::an_unexplained_position_pauses_the_agent_and_alerts` |
| The age check runs only on a resubmission, so an intent re-handed after a long outage is submitted at its first attempt however stale | `hand::a_stale_intent_is_abandoned_at_its_first_submission`, `properties::no_submission_carries_an_intent_older_than_its_maximum_age` |
| A reservation is released only on the four broker statuses, so an `Unknown` order confirmed absent and abandoned holds its reservation for ever and the account's buying power drains | `properties::every_terminal_state_releases_its_reservation`, `hand::an_abandoned_order_releases_its_reservation` |
| `BrokerExchangeRecorded` redacts the authorisation headers but records the account body as sent, so the broker's account number reaches the journal and the artifact store | `hand::an_account_body_is_recorded_with_its_account_number_replaced_by_a_pii_ref`, `properties::no_recorded_exchange_holds_an_account_number_or_an_account_id` |

## Decisions needed

1. **The `mandate-executor` and `mandate-alpaca` entries in `xtask/layers.toml` and `CODEOWNERS`**
   (founder-owned files). Recommendation as interpretation 1 states them: `mandate-executor` at
   `layer = 6` and `mandate-alpaca` at `layer = 7` (DEC-133 item 1 as amended), both
   `safety_critical = true`; `mandate-executor` `pure = true` with `allowed_external = ["thiserror"]`;
   `mandate-alpaca` `pure = false` with
   `allowed_external = ["reqwest", "rustls", "secrecy", "serde", "serde_json", "thiserror", "tokio"]`;
   plus `/crates/mandate-executor/ @kunwarshivam` and `/crates/mandate-alpaca/ @kunwarshivam`. ES-02
   names both crates as safety-critical; the layer of `mandate-alpaca` moved to 7 because it implements
   a trait the layer-6 executor declares, so the files, which are the founder's, need that one change. **Proposed (founder).**
2. **The five parameters trading-domain spec §5.4 and §5.7 name without a value.** They govern how long
   an order may stay unresolved and how early protection is re-placed, so the numbers belong in the spec
   table beside the ones that already have defaults. Adding them edits `docs/specs/`, which ES-22 says
   may not change in the same PR as code and needs a DEC ID; the recommendation is therefore a
   **spec-only PR citing DEC-133**, landed before or with this stream's implementation PR, and this
   brief's proposed values in the meantime:

   | Parameter | Proposed default | Why this value is the conservative one |
   |---|---|---|
   | `max_intent_age` | 120 seconds | Long enough that a restart inside a submission still completes the owner's intent; short enough that a price two minutes stale is not acted on. An intent older than this is abandoned rather than resubmitted |
   | `unknown_absent_lookups` | 3 | One absent answer can be a lagging read replica; three cannot |
   | `unknown_absent_window_s` | 15 seconds | The absences must span a real interval, so three fast polls inside one replica lag do not count as confirmation |
   | `protective_replace_buffer_trading_days` | 5 trading days | A week of trading days before a 90-day GTC expiry, so a holiday or a halt cannot let protection lapse |
   | `restriction_403_threshold` | 3 consecutive | §7.3's "N consecutive 403 rejects without a known order-level cause"; three is the same reasoning as the absence count, and a false `closing_only` only ever reduces risk |

   Every value fails safe if it is wrong in either direction, which is why the brief proposes rather than
   blocks. **Proposed (founder).**
3. **Where paper's shadow ledger is folded.** §10 needs simulated fees and dividends in P&L and buying
   power but out of the cash comparison. The clean shape is a `simulated` marker on the accounting input
   so `mandate-accounting` folds them and reports them separately — which is a change to a
   safety-critical crate this stream does not own. Recommendation: a minimal `mandate-accounting`
   addition under its own shared-crate claim, taken in the implementation PR, rather than a second
   parallel ledger inside the executor (two ledgers would disagree, and the one the owner reads would be
   the wrong one). This is a coordination question for the merge coordinator, not a founder decision.
4. **Confirm the E7-5 boundary.** This stream ingests external activity and sets the `exits_only`
   restriction, and does **not** arbitrate instrument-group claims, deployment admission, or the
   related-accounts coordinator (§7.1, §9.6). Those stay with E7-5, which means `RC-17` turns green with
   E7-5 rather than here. Recommendation: confirm as stated.
5. **`mandate-alpaca` duplicates the transport, credential, and retry shape from `mandate-marketdata`
   rather than sharing a crate with it.** Two hosts, two endpoint allowlists, two credential scopes, and
   one of them is on the order path: a shared crate would let a market-data change reach the trading
   path, and the shared surface is about 150 lines. Recommendation: duplicate now, and revisit when a
   third connector (Kraken, DEC-23) arrives. Accepted as an engineering decision under DEC-79 unless the
   founder prefers otherwise.
6. **Sequencing with stream G.** The binding gate is a direct dependency (interpretation 4), and
   `mandate-risk` does not exist until stream G's tests PR merges. The tests PR therefore stubs the gate
   call site as a crate-private function and the implementation PR adds the dependency. The coordinator
   may prefer to hold this stream's implementation PR until stream G's implementation merges; the tests
   PR does not need it.

7. **Whether a real fixture recording happens in the tests stage.** The round-1 review reports that this
   cloud environment now has the paper keys and egress to the paper trading host, which the brief had
   assumed it did not. Recommendation: the tests PR's required contract stays the **hand-built**
   fixtures — they are what makes the suite reviewable and network-free — and a recording pass is an
   addition the coordinator may ask for in the same stage, under the rules in "The fixture plan"
   (credentials only through the injected lookup, never printed, logged, written, or committed; account
   numbers and ids replaced before a body is saved). The coordinator's call, not the founder's.
8. **Which account-stream events this stream's tests PR interprets, and which arrive with F and J.**
   Interpretation 3 settles *who appends* the risk-state records and `UniverseChanged` — the executor,
   because journal §2 gives the account stream one writer — but not *when* this stream interprets them,
   since the values come from stream F's risk-state fold and stream J's admission. Recommendation: the
   tests PR interprets the copied facts and `RiskDayStarted` (they need nothing from F or J) and leaves
   the risk-state records and `UniverseChanged` returning `NotInterpreted` with their owning story named,
   so the loud failure of DEC-85 is what covers the gap until F and J land. A coordination question for
   the merge coordinator.

## The integration story: the first paper order (not done here)

The story this stream exists to make possible, recorded now with its acceptance criteria so that it is a
story and not an aspiration. **It is not built in this stream**; it needs stream I's runtime, stream G's
gate, and the founder's paper credentials and egress, none of which this brief delivers.

**The story.** The E4-2 baseline strategy (the moving-average crossover, already specified and under
implementation) drives stream I's runtime, whose proposals pass stream G's binding gate inside this
stream's executor and reach an Alpaca **paper** account: one instrument, the smallest quantity the
instrument allows, every order journaled.

**Accepted when:**

- one order for one instrument, sized at or below the mandate's minimum, is proposed by the runtime,
  allowed by the binding gate, journaled as `IntentReceived`, `GateDecided`, and `OrderSubmitted`, and
  acknowledged by Alpaca paper with **our** `client_order_id` on the wire;
- its bracket or OCO protection rests at the broker after the entry fills, and the unprotected interval
  between the first partial fill and the protection is journaled with a start and an end;
- the process is killed at each of the twelve injection points during the run and, after each restart,
  the broker has exactly one order for that intent and reconciliation reports everything accounted for;
- an agent-scoped kill switch cancels that order by client order id, confirms it, sells exactly the
  agent's sub-ledger quantity, and leaves every other order on the account untouched;
- the journal exports and verifies (`mandate journal verify`) over the whole run, and the hash chain
  covers every event;
- the run uses paper credentials read from the environment, and a scan of the journal, the logs, and the
  artifact store finds no credential, no account number, and no personal data;
- the environment on every event of the run is `paper`, and no live host appears in any recorded
  exchange.

**Out of it:** live money, OAuth, more than one instrument, more than the minimum size, and the soak
itself (the Phase 1 exit, M6 to M7).

## Not done

- **No OAuth and no connection management.** E7-1 is M8: no scope request, no token, no vault write, no
  key-permission check. Phase 1 uses the owner's own paper credentials from the environment.
- **No live account, no live host, no fund movement.** There is no live base URL in the crate, no
  deposit, withdrawal, or transfer request in `BrokerRequest`, and no code path that could build one
  (AGENTS.md rule 8).
- **No real recordings.** The tests PR's fixtures are hand-built in the shape a recording lands in;
  recording against the founder's paper account is a later step outside this environment.
- **No shell.** No tokio task, no Postgres `LISTEN`/`NOTIFY`, no polling loop, no process supervision.
  The tests PR ships the two crates and an in-memory driver used only by tests; the shell that binds the
  runtime, the executor, and the connector arrives with the integration story.
- **No gate rules, no mandate validation, no order sizing, no autonomy classification, no research.**
  Streams F, G, H, and J own them; the executor calls the gate and reads the mandate view.
- **No instrument-group claims, no deployment admission, no related-accounts coordinator.** E7-5.
- **No escalation delivery.** M7 and DEC-16.
- **No surveillance report, no readiness report, no wash-sale flagging.** E6-8's report, the
  paper-to-live readiness report, and §9.8 are their own stories; this stream produces the records they
  read.
- **No reference case moves and no `status.toml` change** in the tests PR (see Scope); the cases move in
  a status PR after the implementation PR.
- **No benchmark in the tests PR.** ES-24's p99 is measured and reported by the implementation PR.
- **No snapshots.** Replay runs from `seq` 1; journal §8's snapshots are a later optimisation that
  changes no result.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-executor -p mandate-alpaca
cargo xtask ci pending
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo mutants -p mandate-executor -p mandate-alpaca
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision;
- anything would need a live credential, a live endpoint, or a real order.

## Definition of done

- [ ] Every order the broker sees is named by a client order id that is a pure function of journaled
      facts, and the journal carries that name before the request leaves the process (E7-2).
- [ ] Fault injection at each of the twelve submission steps yields zero duplicates and full
      reconciliation, and a mismatch pauses the agent and alerts (E7-3's acceptance clause).
- [ ] Protective exits rest at the broker as OCO or bracket orders; `RC-14` and its four variants pass;
      every unprotected interval is journaled and alerted beyond the limit (E7-4's acceptance clause).
- [ ] Tests came first; every invariant above has a named test whose oracle computes the answer its own
      way and was shown to fail on a planted bug.
- [ ] New state changes emit journal events: `IntentReceived`, `GateDecided`, `OrderSubmitted`,
      `OrderStateChanged`, `OrderAbandoned`, `BrokerExchangeRecorded`, `FillApplied`, `LateFillApplied`,
      `ProtectionChanged`, `BrokerPositionObserved`, `ReconciliationRun`, `CompensatingEvent`,
      `AccountSnapshotRecorded`, `AccountStateObserved`, `RejectObserved`, `AccountRestrictionChanged`,
      `ExternalActivityIngested`, `AgentModeApplied`, and `KillSwitchActivated`, all on the account
      stream (journal §9).
- [ ] No credential, broker account number, or personal datum appears in any draft, log, error, fixture,
      or file this code writes.
- [ ] Docs updated: this brief, DEC-133, the feature map's "Idempotent executor and broker connector"
      entry, and the tracker's M6, Stories, Claims, and work-graph rows.
- [ ] `cargo xtask check` is green (summary in each PR), and every pending test fails on the stubs.
- [ ] Each PR description is complete (see the PR template).
