# Mandate Spec (v1)

| | |
|---|---|
| **Status** | **Approved** v0.5 (founder sign-off 2026-09-25, [DEC-71](../project/04-decision-log.md#decisions)); requires a decision-log entry and founder approval to change (safety-critical) |
| **Implements** | PRD 6.3 (FR-3.1 to FR-3.8), 6.5 (FR-5.2 to FR-5.5), 6.6 (FR-6.1 to FR-6.6); backlog E6, E10 |
| **Schemas** | [mandate.schema.json](../../schemas/mandate.schema.json), [policy.schema.json](../../schemas/policy.schema.json) (structural rules) |
| **Reference cases** | [reference-cases/mandate.yaml](reference-cases/mandate.yaml) |
| **Related** | [Trading domain spec](trading-domain.md), [journal spec](journal.md), decisions [DEC-39 to DEC-70](../project/04-decision-log.md#decisions) |

A **mandate** is the binding specification the owner sets for an agent: its goal, instruments,
capital, signal models, sizing, protection, risk limits, autonomy rules, and notifications. This
spec defines its structure, validation, the policy hierarchy, the risk state and its limits, the
autonomy rule language, signal models and the order builder, versioning, change classification,
and the records kept.

## 1. Principles

1. **The mandate is binding** (DEC-03). The agent cannot act outside it; the risk gate enforces it
   independently of agent logic.
2. **The user supplies every judgment** (DEC-38, DEC-45). Every field except the system fields and
   the platform defaults listed in §7 is set and confirmed by the user. The compiler only extracts
   what the user stated.
3. **Reducing risk never needs approval and is never denied** (DEC-05, DEC-48). Discretionary exits
   are paced by market-conduct controls.
4. **Losses are bounded.** Daily loss, drawdown, and a lifetime loss floor each have a defined
   response; nothing resets the lifetime floor (DEC-44, DEC-55).
5. **Deterministic.** Every field is typed; conditions are structured; inputs are journaled events
   in sequence order; time comes from the risk clock; ratios are rounded by fixed rules.
6. **Versioned and hashed.** A mandate version is the SHA-256 of its canonical form (journal spec
   §4); deployments pin a version.

### 1.1 Invariants

Every rule in this spec must preserve these properties. The reference implementation asserts
them with property-based tests over random sequences of marks, fills, clock ticks, allocation
changes, acknowledgments, and version changes (AGENTS.md, "Getting it right the first time").

| ID | Invariant |
|---|---|
| MI-1 | Risk reduction is never denied by a mandate limit, conduct control, session rule, or instrument restriction: `risk_exit` and `protective` orders are allowed; `owner_exit` is allowed (outside the regular session, once the bid is confirmed); `discretionary_exit` is allowed or deferred. Exits may be held only by agent mode `paused` or `stopped`, an `Unknown` order, or the broker (trading spec principle 4) |
| MI-2 | An applied allocation change never triggers or lifts a limit, never lowers drawdown or the daily loss fraction, and never raises floor headroom or agent return |
| MI-3 | Latched limits lift only by their defined path (§5.8): drawdown by owner acknowledgment once flat; daily loss by a new risk day plus `daily_breach_min_s`; the lifetime floor only by a loosening version (§5.7) |
| MI-4 | The lifetime floor bounds cumulative loss: it latches once its breach has accumulated `breach_confirm_s` of breach time, or when 1.25 × its loss level holds on two sane quotes at least min(`breach_confirm_s`, 10 s) apart (checked against an independent oracle) |
| MI-5 | H ≥ E and 0 ≤ DD < 1 |
| MI-6 | The effective mode is the strictest active restriction; `AgentModeApplied` is journaled exactly when it changes |
| MI-7 | Allocation increases are rejected while any limit is latched |
| MI-8 | The same mandate and inputs give identical outputs |
| MI-9 | An order-builder proposal never fails a mandate size limit (position, order size, gross exposure) |
| MI-10 | A missing model output never increases the order builder's buy value |
| MI-11 | A version classified risk-reducing or neutral never makes any autonomy decision less strict |
| MI-12 | Every judgment field is user-sourced and confirmed; every `auto` is user-entered (V-020, V-022; checked by the semantic cases, not fuzzed) |
| MI-13 | Dropping clock ticks that emitted no events changes no other result, so only ticks with events need journaling (§5.2) |
| MI-14 | The loss carried to the connection at retirement is the net dollar loss (net contributed − E), whatever withdrawals came first (§5.7) |

## 2. Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Draft: plain-language description or form
    Draft --> Compiled: compiler extracts stated values (judgment fields left blank if unstated)
    Compiled --> Reviewed: user enters and confirms every judgment field
    Reviewed --> Validated: schema + V-rules + policy hierarchy pass; warnings acknowledged
    Validated --> Versioned: canonical hash = mandate_version
    Versioned --> Deployed: backtest and paper requirements met; owner approves (step-up)
    Deployed --> Deployed: new version applied (§2.2)
    Deployed --> Holding: goal complete or end date, on_complete = hold_protected or disarm_ladder
    Deployed --> Retired: on_complete = release; profit_stop reached and flat; agent stopped
    Holding --> Retired: owner releases or closes positions
    Retired --> [*]
```

### 2.1 Provenance and confirmation

Provenance is recorded per field (JSON Pointer) in `MandateVersionCreated`, not in the hashed
document:

| `source` | Meaning |
|---|---|
| `user_stated` | Extracted by the compiler from the user's words; the quoted source span is recorded |
| `user_entered` | Entered by the user in the form or YAML editor |
| `template_structure` | Present because a template included the field or rule (templates carry no values) |
| `platform_default` | Filled by the platform; allowed only for the fields and values in §7 |

Each field also has `confirmed`. `MandateConfirmed` binds the version hash to the list of confirmed
paths and the rendered confirmation screen (§10). Platform defaults are shown on that screen
marked "platform default".

### 2.2 Applying a new version

- **Risk-reducing and neutral versions apply immediately.** Pending approvals are canceled
  (`ApprovalCanceled`) and re-proposed under the new version; working opening orders that violate
  the new limits are canceled.
- **Risk-increasing versions apply at a safe point:** the next evaluation with no `Unknown` orders
  for the agent. Pending approvals are canceled when the version applies.
- **Latched limits are never cleared by a version change** (MI-3), except that raising
  `max_loss_from_allocation` can lift the lifetime floor as §5.7 allows. `environment` and
  `connection_id` never change (V-031).
- **Allocation changes** follow §5.1 and may be rejected at application.
- **Removed instruments** keep their claim (trading spec §7.1) until the agent is flat in them;
  until then the instrument is exits-only for the agent: protection stays, risk exits apply, and
  model outputs are still requested so discretionary exits work.
- **Policy changes** apply to running agents as an overlay (§4.3).
- Application is journaled as `MandateVersionApplied` (result `applied` or `rejected` with reason).

## 3. Structure

The schema defines structure; this table defines meaning. Money is USD. Fraction fields are
decimal strings; the schema gives each field's bounds.

| Path | Meaning |
|---|---|
| `mandate_schema_version`, `source_text_ref` | System fields: `1`; artifact hash of the user's description (or `null`) |
| `name` | Agent name (lowercase, hyphens) |
| `environment`, `connection_id` | `paper` or `live`; the broker connection. Immutable across versions |
| `capital.allocation_usd` | The agent's capital allocation A (§5.1) |
| `capital.max_loss_from_allocation` | Lifetime loss floor as a fraction of the capital base (§5.7) |
| `goal` | `continuous`, `accumulate`, or `profit_stop` (§3.1) |
| `universe.instruments[]` | Instruments the agent may open or increase (1–50), sorted by `asset_id` |
| `universe.leveraged_etps_enabled`, `leveraged_etp_disclosure_version` | Opt-in for complex ETPs (trading spec §3.2) and the accepted disclosure version |
| `behavior.description` | The user's description of the strategy; given to LLM signal models |
| `behavior.signal_models[]` | Selected signal models: id, version, content hash, parameters, fixed weight, `max_output_age_s` |
| `behavior.cadence` | Scheduled interval and the event sources that trigger evaluation |
| `behavior.sizing` | Sizing method, entry and exit thresholds, rebalance band (§8.3) |
| `protection` | Resting protection: stop and take-profit distances as fractions of entry price; crypto stop-limit offset as a fraction of the stop price |
| `risk.*` | Limits, timings, and `scale_action` (§5) |
| `autonomy.rules[]`, `default`, `approval` | Autonomy rules and approval settings (§6) |
| `notifications` | Channels and quiet hours |

**Set-like arrays are sorted and unique** (V-009) so that equal mandates hash equally:
instruments by `asset_id`, signal models by `id`, parameters by `key`, and `event_sources`,
`channels`, and `approvers` lexically. Rule order is significant (first match); rule ids are
unique.

### 3.1 Goals and stop conditions (DEC-46, DEC-59)

`end_date` is the last risk day of the goal (§5.4); the goal ends at 00:00 America/New_York after
it. `null` means no end.

| Type | Parameters | Behavior | Done when | Then |
|---|---|---|---|---|
| `continuous` | `end_date`, `on_complete` | Trades the universe | `end_date` passes | `on_complete` |
| `accumulate` | instrument, `target_qty`, `max_avg_price` (or null), `max_spend_usd`, `end_date`, `on_complete` | Buys only the goal instrument; the universe is exactly that instrument (V-003). **Discretionary exits are disabled**; risk exits and protection apply. Buys are clipped (§8.3) | Remaining quantity (`target_qty` − position) is below one increment or below the minimum order; remaining spend is below the minimum order; or `end_date` passes | `on_complete` |
| `profit_stop` | `profit_level`, `end_date` | Trades the universe | Agent return reaches the level, confirmed in the risk state by breach time per §5.6 with no hard trigger: E − C ≥ `profit_level` × C (C = capital base, §5.1); or `end_date` passes | Discretionary exit of every position, then Retired (`AgentStopped`, reason `profit_stop_reached` or `end_date`) |

- `profit_stop` is a stop condition, not a target: the UI shows `profit_level` as the level at
  which the agent stops, never as progress toward a goal.
- **Goal spend** is the sum of the agent's buy fills in the goal instrument, including fees; sales
  never reduce it.
- **`on_complete`** (chosen at confirmation, a judgment field):

| Value | Effect |
|---|---|
| `hold_protected` | **Holding:** restriction `goal_complete` (mode `exits_only`); protection and every risk limit stay armed |
| `disarm_ladder` | Holding, but the drawdown ladder and daily loss are disarmed; protection and the lifetime floor stay armed |
| `release` | The agent cancels its protective orders, the ledger records `PositionReleased`, the positions become the owner's external holdings, claims are released, and the agent retires |

- **Holding:** the owner is alerted when it starts. The owner can later release (step-up; journaled
  as `PositionReleased`, including the warning shown that the positions will be unprotected) or
  close (`owner_exit`, §6.1). **Release counts as closing the position for retention** (trading
  spec §13).

## 4. Validation

A mandate is valid when it passes the JSON Schema, every V-rule, and the policy hierarchy (§4.3).
Failures return all violated codes. Warnings (§4.2) do not block, but each must be acknowledged
and is recorded in `MandateConfirmed`.

### 4.1 V-rules

| Code | Rule |
|---|---|
| V-001 | `connection_id` belongs to the workspace and matches `environment` (paper or live account) |
| V-002 | Other active agents' allocations on the account + this allocation ≤ account equity. Checked at validation and again atomically when a version is applied |
| V-003 | `accumulate`: the universe is exactly the goal instrument |
| V-005 | `leveraged_etps_enabled = true` requires `leveraged_etp_disclosure_version`, a `DisclosureAccepted` by the owner (with step-up) for exactly that version, and policy allowing it (§4.3). A new disclosure version makes leveraged-ETP openings inactive until the owner accepts it |
| V-006 | No instrument's **instrument group** (trading spec §7.1) is claimed by another agent on the account |
| V-007 | Each signal model's id, version, and content hash are registered together; parameters are exactly the model's declared parameters and match its parameter schema |
| V-008 | If protection is enabled and the universe contains crypto, `crypto_stop_limit_offset` is set. If protection is disabled, `stop_distance`, `take_profit_distance`, and `crypto_stop_limit_offset` are null |
| V-009 | Set-like arrays are sorted and unique (§3); rule ids are unique |
| V-010 | Ladder `at` strictly increasing; actions in non-decreasing severity (`scale_sizes`, `exits_only`, `flatten_and_pause`); `factor` set for `scale_sizes` and null otherwise |
| V-011 | Exactly one `flatten_and_pause` rung, last, with `at = max_drawdown` |
| V-012 | `hysteresis` < the first rung's `at` |
| V-013 | `max_order_usd ≤ max_position_usd ≤ max_gross_exposure_usd ≤ allocation_usd` |
| V-014 | `max_loss_from_allocation ≥ max_drawdown` |
| V-015 | Dates are valid calendar dates |
| V-016 | Quiet hours `start ≠ end` |
| V-017 | Conditions nest at most 4 levels |
| V-018 | Rules do not use `unusual_input` until the input-drift detector ships (DEC-60) |
| V-020 | Every field except the system fields and the platform defaults of §7 is `user_stated` or `user_entered`, and confirmed. A `platform_default` is valid only on a §7 field with its listed value; `approvers` may be a platform default only in a single-user workspace |
| V-022 | Every `auto` (the default or a rule's `then`) is `user_entered` and confirmed; the compiler and templates never produce `auto` |
| V-023 | Condition values match the field type (§6.3): enums take listed values (`purpose` only `open` or `increase`); decimal fields take canonical decimal strings with `eq`, `ne`, `gt`, `gte`, `lt`, `lte`; `in`/`not_in` take non-empty arrays and only on enum and string fields; booleans take `eq`/`ne`; `combined_score` and `drawdown` values are in [0, 1] |
| V-024 | Approvers resolve to at least one user with the approver role; if `two_approver_above_usd` is set, to at least two distinct users |
| V-030 | `end_date`, if set, is not before the validation date |
| V-031 | `environment` and `connection_id` equal the previous version's |
| V-032 | The connection's loss carry (§5.7) is below `max_loss_from_allocation` × allocation; otherwise deployment is rejected |
| V-033 | `scale_action: trim_to_target` is not allowed with an `accumulate` goal (trims would consume `max_spend_usd` without adding units) |

### 4.2 Warnings and the confirmation screen

| Code | Warning |
|---|---|
| W-001 | An instrument fails the eligibility floor at validation time (the gate enforces at runtime) |
| W-002 | Worst-case loss of one full position at its stop exceeds the daily loss budget: min(`max_position_usd`, `max_position_fraction` × A) × (`stop_distance` + crypto offset if crypto) > `max_daily_loss` × A |
| W-003 | Protection is disabled: no resting protective orders at the broker |
| W-005 | A rule follows a catch-all rule (`purpose in [increase, open]`) and can never match |

The confirmation screen also shows, in dollars: one position's loss at its stop, the daily loss
budget, the loss at which the agent flattens (`max_drawdown` × A), and the lifetime floor loss.
It states that gaps and exit pricing can exceed each of them, and it describes `scale_action` in
plain language ("limits new buys only" or "sells down to the scaled size").

### 4.3 Policy hierarchy (DEC-51, DEC-61)

Platform → organization → workspace → mandate. **A child may only tighten.** Each level is checked
against every level above it. A violation reports the key, the violating level and value, and the
**nearest** ancestor whose value it breaks. Policies are documents validated by
[policy.schema.json](../../schemas/policy.schema.json); policy ceilings are shown to users as
limits, never pre-filled as values.

| Kind | Keys | Rule |
|---|---|---|
| Maximums | `allocation_usd`, `max_loss_from_allocation`, `max_position_usd`, `max_position_fraction`, `max_gross_exposure_usd`, `max_order_usd`, `max_orders_per_day`, `max_daily_loss`, `max_drawdown`, `breach_confirm_s`, `max_output_age_s` (every model), `exit_threshold`, `stop_distance_max`, `exits_only_at_max` (the first rung with action `exits_only` or stricter), `two_approver_above_usd` (the mandate must set one at or below it) | Child ≤ parent |
| Minimums | `entry_threshold`, `rebalance_band`, `hysteresis`, `cadence_interval_s`, `approval_timeout_s`, `reentry_cooldown_s`, `daily_breach_min_s`, `scale_lift_after_s` | Child ≥ parent |
| Permissions | `leveraged_etps_allowed`, `auto_allowed` (any `auto` in the mandate) | Child may be `true` only if every ancestor is `true` |
| Requirements | `protection_required`, `independent_approval_required` | Once `true` at a level, every child is `true` |
| Sets | `asset_classes`, `signal_model_types` (`fast`, `llm`, `quant`), `goal_types`, `channels` | Child ⊆ parent |

- **Platform base:** `max_loss_from_allocation ≤ 0.5`; `breach_confirm_s ≤ 300` (also in the
  schema).
- **Retail profile** (platform level, applied to retail workspaces; **all values pending counsel**,
  [compliance questions 20 to 30](../product/08-compliance-and-regulatory.md)): `auto_allowed:
  false`, `signal_model_types: [quant]`, `leveraged_etps_allowed: false`, `protection_required:
  true`, `approval_timeout_s ≥ 120`, `max_loss_from_allocation ≤ 0.2`. **A workspace is retail
  unless its owning organization is a verified entity other than an individual's personal
  investment vehicle, or meets the investor-status test counsel sets (question 4).** Deployment
  mode does not affect the profile; an unassigned workspace is treated as retail. Each assignment
  or change and its basis are journaled as `WorkspaceProfileAssigned` (DEC-68).
- `independent_approval_required` (maker-checker): deployment, risk-increasing changes,
  high-water-mark resets, and loosening a latched lifetime floor need approval by a user other than
  the requester.
- **Policy changes** are journaled as `PolicyChanged` (level, diff, author, step-up evidence,
  affected agents). They apply to running agents at the next evaluation as an **overlay**: the
  stricter value governs, and `auto` evaluates as `ask` when `auto_allowed` becomes false. Affected
  agents are flagged `policy_nonconforming` and their owners are alerted; a conforming version is
  required before any risk-increasing change.

## 5. Risk state and limits

The executor computes the risk state from the account ledger, so it is part of the **account
stream** fold (journal spec §2). It belongs to the agent and survives restarts and version changes.

### 5.1 Capital, equity, and allocation changes (DEC-39, DEC-50, DEC-53)

- **Allocation** A: Σ allocations on an account ≤ account equity (V-002). If withdrawals or other
  agents' losses make Σ allocations exceed account equity, the owner is alerted and allocation
  increases are rejected; account-level buying power still binds every order.
- **Capital base** C: starts at the initial allocation and scales with allocation changes (below).
- **Net contributed** N: the initial allocation plus every applied allocation change, in dollars
  (withdrawals are negative). It measures the dollar loss carried to the connection (§5.7).
- **Agent equity** E = A + agent realized P&L (net of fees) + agent income + agent market value at
  risk marks − cost basis. Cost basis follows trading spec §8.1 (reductions rounded half-even to 12
  places). The agent sub-ledger is the account ledger filtered to the agent's orders.
- **An allocation change** of Δ is applied when its version applies, **after** time has been
  settled at that instant (§5.2), with no time passing:
  1. **Rejected** if Δ > 0 while any limit is latched; if E + Δ ≤ 0; if E + Δ < the agent's gross
     exposure (Σ |MV| + working opening orders); or if, after scaling, any limit condition would be
     newly true (`would_trigger_limit`).
  2. Otherwise, with k = (E + Δ) ÷ E: A′ = A + Δ, N′ = N + Δ; H′, E₀′, C′, and the inherited loss L′
     are H, E₀, C, and L multiplied by k, rounded **up** to 12 places (so drawdown and loss fractions
     never fall). Drawdown, the daily loss fraction, floor headroom, and agent return are preserved,
     up to that conservative rounding (MI-2).

### 5.2 Inputs, the risk clock, and determinism

- **Inputs** are account-stream events in `seq` order: `MarkUpdated` (risk marks), `FillApplied`,
  `LateFillApplied`, `FeesCharged`, `CorporateActionApplied` (splits, and dividend receivables on
  the ex-date per trading spec §8.3), `CashInLieuPosted`, `CompensatingEvent`,
  `MandateVersionApplied`, `RiskDayStarted`, copied `ClockAdvanced`, and owner acknowledgments.
- **The risk clock** is the scheduler's `ClockAdvanced` time, in whole seconds and monotone. The
  scheduler ticks every second; the executor evaluates every tick but journals a copied
  `ClockAdvanced` in the account stream only when the evaluation emits an event (or the tick
  crosses midnight). **Every other risk input carries a required `risk_clock` field** (the latest
  tick the executor had seen); the journal rejects a `risk_clock` that decreases along `seq`.
  `event_time` is never used for risk timing. Durations are integer seconds.
- **Durations** (confirmation, lift delays, cooldowns, staleness) are sums over intervals between
  evaluations, each credited according to the state at the **start** of the interval. Conditions
  change only at inputs, never at ticks, so unjournaled ticks do not change any result (MI-13).
- **Risk marks** (trading spec §8.2: bid for longs). For **equities, only regular-session sane marks
  update E**; extended-hours quotes are ignored, so E₀ at 00:00 reflects the last regular-session
  mark. Crypto uses sane marks at all times.
- **Staleness is per instrument.** If a held instrument has no sane mark for the data profile's
  staleness limit (measured in regular-session time for equities), or a mark fails the checks, it
  gets restriction `stale_mark` (journaled as `InstrumentRestrictionChanged`): no opening or
  increasing orders in it until a sane mark arrives. A stale mark never triggers a flatten.
- **Comparisons are exact:** a rung is breached when H − E ≥ `at` × H and lifts when
  H − E < (`at` − `hysteresis`) × H. Daily loss: E − E₀ ≤ −`max_daily_loss` × E₀. Floor:
  E ≤ C × (1 − `max_loss_from_allocation`) + L.
- **Reported ratios** (DD, daily P&L fraction, `position_pnl_fraction`, and the `drawdown` and
  `daily_pnl_fraction` condition fields) are rounded half-even to 12 places.
- **Evaluation order per input:** settle time; apply the input; update E, then H = max(H, E); ladder
  rungs in ascending `at`; daily loss (trigger, renewal, or lift); lifetime floor; then the
  effective mode (§5.9). Journal events follow this order.

### 5.3 Position, exposure, order size, count, and cooldown

These limits apply to **opening and increasing orders only**; exits, protective orders, owner
exits, and the kill switch are never denied by them (MI-1).

| Limit | Definition | Gate check (trading spec §9.1) | Reason code |
|---|---|---|---|
| Per-instrument position | MV(instrument) + max cost of working opening orders in it + the proposed order ≤ min(`max_position_usd`, `max_position_fraction` × E) | 2 | `concentration_limit` |
| Order size | Limit price × quantity ≤ `max_order_usd` | 2 | `max_order_size` |
| Re-entry cooldown | No opening order in an **instrument group** until `reentry_cooldown_s` after the agent's last exit fill in any instrument of the group | 2 | `reentry_cooldown` |
| Orders per day | Opening and increasing orders submitted by the agent in the risk day (each `client_order_id` once, including rejected ones) + 1 ≤ `max_orders_per_day`. Denies the order; the platform rate limits of trading spec §9.7 are separate | 6 | `max_orders_per_day` |
| Agent gross exposure | Σ \|MV\| of the agent's positions + max cost of its working opening orders + the proposed order ≤ min(`max_gross_exposure_usd`, E) | 7 | `gross_exposure_limit` |

### 5.4 Daily loss (DEC-40, DEC-49, DEC-54)

- The **risk day** runs from 00:00 to 00:00 America/New_York (23 or 25 hours on daylight-saving
  change days). The executor journals `RiskDayStarted` when the copied `ClockAdvanced` crosses
  midnight, and sets E₀ = E.
- **Breach:** E − E₀ ≤ −`max_daily_loss` × E₀, confirmed per §5.6. **A breach still confirming at
  the rollover keeps confirming against the previous day's E₀** (at most `breach_confirm_s`, so at
  most 300 s): it latches (reason `resolved_at_rollover`) if it confirms, and is discarded if the
  condition stays false for `breach_confirm_s`. A flash print just before midnight therefore
  latches nothing. The action is `daily_loss_action`:
  - `exits_only`: restriction `daily_loss` (mode `exits_only`).
  - `flatten_and_pause`: the agent-scoped kill switch (§5.5), restriction `daily_loss` (mode
    `paused`). Once the agent is flat, owner acknowledgment (step-up) changes it to `exits_only`.
- **Lift:** automatically, once a new risk day has started, at least `daily_breach_min_s` has
  passed since the breach, and (for `flatten_and_pause`) the owner has acknowledged. During the
  lift delay the new day's loss is evaluated against the new E₀; a confirmed new-day breach renews
  the latch (reason `new_day_breach`).

### 5.5 Drawdown and the ladder (DEC-41, DEC-44, DEC-56)

- **High-water mark** H = the maximum E since deployment or the last reset (§5.8).
  **Drawdown** DD = (H − E) ÷ H.

| Action | Trigger | Effect | Lifts when |
|---|---|---|---|
| `scale_sizes` | Immediately | Size factor = product of active rungs' factors. `scale_action: limit_buys`: order-builder targets are multiplied by it. `trim_to_target`: also, a position with MV − factor × cap ≥ `rebalance_band` × cap is sold down to factor × cap as a `risk_exit` (quantity rounded up to the increment) at the next evaluation, only once the rung has been active for `breach_confirm_s`, only if the order meets the minimum, for equities only in the regular session, and never while Holding (DEC-65) | H − E < (`at` − `hysteresis`) × H for `scale_lift_after_s` of regular-session time (crypto: all time) |
| `exits_only` | Confirmed (§5.6) | Restriction `drawdown_exits_only` (mode `exits_only`) | Owner acknowledgment (§5.8) |
| `flatten_and_pause` | Confirmed (§5.6) | Agent-scoped kill switch; restriction `drawdown_flatten` (mode `paused`) | Owner acknowledgment once flat (§5.8) |

- **Agent-scoped kill switch** (trading spec §5.5): the final mode is applied first. The executor
  cancels only the agent's orders (by `client_order_id`, confirming each), never the broker's
  cancel-all, then sells exactly the agent's sub-ledger quantity, never the broker's
  close-position. **Automated** flattens (limits, lifetime floor) wait for the regular session to
  sell equities, with protection left in place; crypto sells go immediately. An owner kill switch
  follows §6.1 `owner_exit`.

### 5.6 Breach confirmation (DEC-49, DEC-54)

Applies to `exits_only` and `flatten_and_pause` rungs, daily loss, the lifetime floor, and
`profit_stop`.

- **Breach time** accumulates over every interval between inputs that starts with the condition
  true. The limit triggers at the first input where the condition is true and breach time
  ≥ `breach_confirm_s`.
- Breach time resets to 0 only after the condition has been false continuously for
  `breach_confirm_s`, so a brief bounce does not restart confirmation.
- **Hard trigger** (DEC-63): a loss of at least 1.25 × the limit's loss level (for a rung,
  H − E ≥ 1.25 × `at` × H; daily, E − E₀ ≤ −1.25 × `max_daily_loss` × E₀; floor,
  E ≤ C × (1 − 1.25 × `max_loss_from_allocation`) + L) on a sane quote immediately applies
  restriction `hard_breach` (mode `exits_only`, reason `hard_breach_pending`). The limit itself
  latches (reason `hard_trigger`) when the hard level holds on a second sane quote at least
  min(`breach_confirm_s`, 10 s) later. A sane quote below the hard level clears `hard_breach`
  (`hard_breach_cleared`), and normal confirmation continues. One bad print therefore never
  latches or flattens anything.
- Confirmation continues on clock ticks when no marks arrive (for example after the close).
  `breach_confirm_s` is at most 300 s; 0 triggers on the first breaching input.
- Limits with breach time accumulating are shown in the agent's state (`pending`); they are
  journaled when they trigger.

### 5.7 Lifetime loss floor (DEC-44, DEC-55)

- **Breach:** E ≤ C × (1 − `max_loss_from_allocation`) + L, confirmed per §5.6. The result is the
  agent-scoped kill switch and restriction `lifetime_floor` (mode `paused`).
- **It cannot be acknowledged or reset.** It lifts only when a version raising
  `max_loss_from_allocation` to f′ applies with E > C × (1 − f′) + L (strictly), journaled as
  `RiskLimitLifted` with reason `version_loosened`; confirmation then starts afresh. While the floor
  is latched, that version needs independent approval (a second user). In a single-user workspace
  it applies only once the first full risk day after the confirmation day has ended; earlier it is
  rejected (`waiting_period`), as is a version that leaves E at or below the new floor
  (`still_below_new_floor`) or does not loosen (`not_loosening`).
- **Inherited loss L.** Each connection keeps a **loss carry**: the sum, over agents retired on it
  in the last 90 days, of their **net dollar loss** max(0, N − E) at retirement (§5.1). Withdrawals
  lower N and E equally, so withdrawing before retiring cannot shrink the carry (MI-14). A new agent
  on the connection starts with L = the carry, so retiring and redeploying cannot reset the floor.
  Deployment is rejected if the carry ≥ `max_loss_from_allocation` × allocation (V-032).
- The platform caps `max_loss_from_allocation` (§4.3).

### 5.8 Acknowledgment and high-water-mark reset (DEC-44, DEC-57)

- **Latched limits:** `drawdown_exits_only`, `drawdown_flatten`, `daily_loss` (until its lift), and
  `lifetime_floor`.
- **Acknowledging the drawdown ladder** (owner, step-up; with `independent_approval_required`, by
  a user other than the requester):
  - is rejected with `flatten_in_progress` while a flatten has not finished (the agent is not
    flat);
  - otherwise resets H := E (`HighWaterMarkReset`), unlatches the `exits_only` and
    `flatten_and_pause` rungs, and sets every `scale_sizes` rung active.
- **After a reset the scale rungs lift one at a time,** highest `at` first, each after
  `scale_lift_after_s` of regular-session time (crypto: all time) counted from the reset or from the
  previous lift. Sizes therefore return in steps.
- Resets touch neither C nor L, so the lifetime floor bounds cumulative loss across any number of
  resets (MI-4).

### 5.9 Restrictions and the effective mode

Agent restrictions each have a mode and lift independently: `daily_loss`, `drawdown_exits_only`,
`drawdown_flatten`, `lifetime_floor`, `hard_breach`, `goal_complete`, and the trading spec's account,
external-activity, reconciliation, and rate-limit restrictions (§7.3, §7.4, §9.7). The **effective
mode** is the strictest (`normal` < `exits_only` < `paused` < `stopped`); `AgentModeApplied` is
journaled only when it changes (MI-6). On entering `exits_only` or stricter, the executor cancels
the agent's working opening orders and pending approvals. Instrument restrictions (`stale_mark`,
removed instruments) block opening and increasing orders in that instrument only.

### 5.10 Journal events

| Event | Stream | When | Payload |
|---|---|---|---|
| `MandateVersionApplied` | account | A version takes effect or is rejected (§2.2) | agent, old and new version, classification, step-up evidence, allocation change, result and reason |
| `RiskDayStarted` | account | 00:00 America/New_York | agent, E₀ |
| `RiskLimitTriggered`, `RiskLimitLifted` | account | A limit or rung changes state | agent, limit (`max_daily_loss`, `drawdown_ladder[i]`, `lifetime_floor`), action, reason (`hard_trigger`, `resolved_at_rollover`, `new_day_breach`, `after_reset`, `owner_acknowledged`), E, H, DD, E₀, C, L, breach time |
| `HighWaterMarkReset` | account | Owner acknowledgment (§5.8) | agent, old and new H, acknowledging user (opaque), step-up evidence |
| `AgentModeApplied` | account | The effective mode changes | agent, from, to, restrictions; copied by the agent runtime into the agent stream as `AgentModeChanged` |
| `KillSwitchActivated` | account | A flatten | scope, initiator, orders canceled, sells submitted or deferred |
| `InstrumentRestrictionChanged` | account | `stale_mark` set or cleared | agent, instrument, restriction, active |
| `GoalCompleted` | account | A goal completes (§3.1) | agent, reason (`profit_stop_reached`, `target_qty`, `max_spend`, `end_date`), `on_complete` applied |
| `AgentStopped` | workspace control | The agent retires | agent, reason, net dollar loss added to the connection's loss carry |
| `OwnerExitRequested` | agent | The owner closes a position or triggers a kill switch | instrument or scope, bid shown and confirmed, user (opaque), step-up evidence |
| `PositionReleased` | account | Release (§3.1) | agent, instrument, quantity, protective orders canceled, warning shown (artifact), user (opaque), step-up evidence |

## 6. Autonomy (DEC-42, DEC-48, DEC-58)

### 6.1 Purposes

The **gate assigns** each order's purpose from its side and the agent's position, not from the
proposer's label. A buy is `open` (no position) or `increase`. A sell of at most the position is an
exit, typed by its origin. A sell above the position is rejected (no short sales).

| Purpose | Origin | Approval | Market-conduct controls |
|---|---|---|---|
| `open`, `increase` | Order builder | Autonomy rules (§6.2) | Apply (trading spec §9.6) |
| `discretionary_exit` | Order builder (signal exit), goal completion, removed instruments | Built-in AUTO; never denied | **Paced, never denied:** price collar and participation caps; in the close window, marketable limit orders only (DEC-70); equities in the regular session only (verdict `defer` outside it) |
| `owner_exit` | The owner closes a position or triggers a kill switch | The owner's instruction (step-up); never denied | Participation caps pace it. Outside the regular session, equities sell through the exit price ladder once the owner has confirmed the displayed bid and bid size and a **floor price** (default: the confirmed bid × (1 − the exit ladder's maximum offset)); the ladder never prices below the floor, any remainder rests at the floor and then waits for the session, and the owner is alerted (DEC-66) |
| `risk_exit` | Risk engine: limits, flatten, `trim_to_target`, stop watchdog (trading spec §5.4) | Built-in AUTO; never denied | Exempt |
| `protective` | Executor: placing and re-placing protection | Built-in AUTO; never denied | Exempt |

### 6.2 Evaluation

1. The risk engine proposes any `trim_to_target` risk exit (§5.5); otherwise the order builder
   proposes an action already **clipped to the limits** (§8.3).
2. **Gate dry run.** `deny`: the action is skipped and journaled; no approval is ever requested for
   an order the gate would deny. `defer` (discretionary exits only): nothing is submitted and the
   deferral is journaled. **No deferred intent is stored.** The order builder proposes again at
   each evaluation, and an evaluation also runs at the regular-session open, so a deferred exit
   happens only if the signal still calls for it.
3. **Built-in:** purposes other than `open` and `increase` are AUTO.
4. Otherwise, evaluate `autonomy.rules` **in order**; the first matching rule decides (`auto`,
   `ask`, `deny`). If none matches, `autonomy.default`.
5. `auto` → submit (the gate runs again at submission). `deny` → skip. `ask` → approval (§6.4).

### 6.3 Condition language

Conditions are JSON objects: `{"all": [...]}`, `{"any": [...]}`, `{"not": {...}}`, or a comparison
`{"field", "op", "value"}`. Rules see only `open` and `increase` actions.

| Field | Type | Meaning |
|---|---|---|
| `purpose` | enum | `open`, `increase` |
| `order_usd` | decimal | Limit price × quantity |
| `combined_score` | decimal in [0, 1] | The order builder's combined model score (§8.3); **not a probability of profit** |
| `instrument` | string | Instrument `asset_id` |
| `asset_class` | enum | `us_equity`, `crypto` |
| `session` | enum | `pre_market`, `regular`, `after_hours`, `crypto` |
| `first_trade_in_instrument` | boolean | The agent has no prior fill in this instrument |
| `drawdown`, `daily_pnl_fraction` | decimal | §5.2 values (12 places) |
| `position_usd_after` | decimal | MV of the instrument + working opening orders in it + this order |
| `gross_usd_after` | decimal | Agent gross exposure including this order |
| `bought_today_usd` | decimal | Opening and increasing order value submitted in the risk day, including this order |
| `position_pnl_fraction` | decimal | (MV − cost basis) ÷ cost basis of the current position, 12 places; 0 if none |
| `unusual_input` | boolean | **Reserved:** not usable until the input-drift detector ships (V-018) |

Operators: `eq`, `ne`, `gt`, `gte`, `lt`, `lte` (decimals compare numerically), `in`, `not_in`
(arrays). Type rules are V-023. The exposure fields let users bound what order splitting could
otherwise evade (for example, `bought_today_usd gt 2000 → ask`).

### 6.4 Approvals

- **Content:**
  - the proposed action (instrument, side, quantity, limit price, order value), its purpose, the
    mandate version, and the rule that triggered it;
  - the combined score, labeled "combined model score, not a probability of profit";
  - the deadline, and "If you do nothing, this action is skipped".
  - Model outputs sit behind "View model output", labeled by author and "Output of software you
    selected; not a recommendation". They are never included in notifications.
  - Never platform-authored alternatives, persuasive language, or profit estimates.
- **Binding:** an approval binds the quantity, limit price, and mandate version. On approval the
  gate runs again; if it denies, or the version has changed, the action is skipped. No re-pricing
  in v1.
- **Step-up:** live approvals require step-up authentication within the 5 minutes before the
  response.
- **Two approvers:** an ASKed action with `order_usd` above `two_approver_above_usd` needs two
  distinct approvers; with `independent_approval_required`, neither may be the mandate's author.
  `deny` is never overridden.
- **Timeout:** `on_timeout` is always `skip`. Approval requests are not delivered during quiet
  hours, so they time out and are skipped. **Risk-limit alerts ignore quiet hours.**

## 7. Compiler (DEC-45)

- **Judgment fields are every field except:** the system fields (`mandate_schema_version`,
  `source_text_ref`) and these platform defaults:

| Field | Allowed platform default |
|---|---|
| `name`, `notifications` | Any (shown for confirmation) |
| `environment` | `paper` only; `live` is always user-entered |
| `autonomy.approval.on_timeout` | `skip` |
| `autonomy.approval.approvers` | Any, in a single-user workspace only; in a multi-user workspace the user must enter it |
| `autonomy.default` | `ask` |
| `universe.leveraged_etps_enabled`, `leveraged_etp_disclosure_version` | `false`, `null` |

- **Input:** the user's plain-language description (stored as an artifact), optional form fields,
  and optional templates. Templates set **which fields and rules are present, never values**.
- **For judgment fields the compiler only extracts values the user explicitly stated,** recording
  the quoted source span (`user_stated`). Unstated judgment fields are left empty and the user must
  enter them (`user_entered`). The compiler never proposes instruments, signal models, numbers, or
  `auto`.
- **Unenforced constraints:** if the description contains a constraint the mandate cannot express
  (for example, "avoid trading around macro releases"), the compiler flags it on the review screen
  as **not enforced**. It reaches LLM models only as description text.
- The compiler is a model invocation (`ModelInvocationRecorded`); its output must pass the schema
  before review. **Quality metric:** extraction accuracy on a maintained evaluation set.

## 8. Signal models and the order builder

### 8.1 Signal model contract (DEC-52)

- A signal model is a registered component with an id (`quant.`, `fast.`, or `llm.` prefix), a
  semantic version, and a **content hash** of its code, prompt, parameter schema, and underlying
  model identity (provider, model name, and version, or weights hash); the mandate pins all three
  (V-007). **Signal models never place orders.**
- **The model gateway never substitutes a model** (DEC-67): a fallback may route only to another
  endpoint serving the identical pinned model; otherwise the call fails and the output counts as
  missing (§8.3). The platform withdraws a model version only through a journaled
  `PlatformOperatorAction` (`model_withdrawn`, with reason); owners are alerted and the withdrawn
  model's outputs count as missing.
- **Parameter schemas have no defaults;** every parameter is set by the user. **Documentation
  describes methodology only,** with no performance claims, rankings, or "recommended" labels.
  Each model carries an authorship label (platform-authored; user-authored later).
- **LLM models** receive only `behavior.description`, the universe, and their market-data and news
  inputs. Agent memory does not feed signal models in v1 (DEC-62). Their output is limited to
  observations, evidence references, and invalidation conditions: no imperatives ("buy",
  "should"), no price targets, no statements of likely profit. Outputs that violate this are
  ignored and journaled.
- **Scorecards** use only the user's own journaled results. They are never aggregated across users,
  never shown in the model picker, never used in marketing, and never change weights.

### 8.2 Output

| Field | Type | Rule |
|---|---|---|
| `model_id`, `model_version`, `content_hash` | string | Must equal the pinned values; otherwise ignored |
| `instrument_id` | uuid | In the universe, or a held removed instrument (§2.2) |
| `as_of` | timestamp | Data cut-off used (no look-ahead) |
| `expires_at` | timestamp | |
| `conviction` | decimal in [−1, 1] | Positive favors holding; negative favors exiting (long-only in v1) |
| `confidence` | decimal in [0, 1] | The model's self-reported confidence (uncalibrated in v1) |
| `horizon_s` | integer | Intended holding horizon |
| `thesis_ref`, `evidence` | artifact hash; event IDs | Observations and references |
| `invalidation` | string | What would invalidate the output |

**Fresh** means `as_of ≤ now < expires_at` and `now − as_of ≤` that model's `max_output_age_s`.
Only the latest fresh output per model counts (latest `as_of`, ties by journal sequence). Outputs
are journaled (`ModelOutputRecorded`); ignored outputs are journaled with the reason.

### 8.3 Order builder: `conviction_linear` (DEC-47, DEC-60)

The user selects the sizing method (v1 has one), shown in plain language on the confirmation
screen. Weights are fixed user-set values; **there is no calibration in v1.** For each instrument at
each evaluation:

1. **Combine.** W = Σ weights of **all** configured models. With F = Σ over fresh outputs of
   wᵢ · convictionᵢ · confidenceᵢ, and M = Σ weights of models without a fresh output:
   - exit conviction c = round₁₂(F ÷ W): a missing model counts as 0, so an outage never forces
     sells;
   - buy conviction b = round₁₂((F − M) ÷ W): a missing model counts as fully bearish, so an outage
     never enlarges a buy (MI-10);
   - combined score s = round₁₂(Σ fresh wᵢ · confidenceᵢ ÷ W);
   - no fresh outputs: hold.
2. **Decide.**
   - c ≤ −`exit_threshold`: discretionary exit of the whole position (minus working exits) at the
     bid within the collar; working opening orders in the instrument are canceled first. Disabled
     for `accumulate`.
   - b ≥ `entry_threshold`: target T = b × cap × size factor, where
     cap = min(`max_position_usd`, `max_position_fraction` × E).
   - Otherwise hold (no new buys, no sells).
3. **Size.** Delta = T − MV (at the risk mark) − max cost of working opening orders.
   - Hold if Delta ≤ 0. There are no signal trims in v1; positions shrink by exits and by
     `trim_to_target`.
   - Hold if Delta < `rebalance_band` × cap.
   - Otherwise the buy value is min(Delta, `max_order_usd`, cap − MV − working,
     min(`max_gross_exposure_usd`, E) − gross exposure), at the ask, truncated to the increment.
     **Hold if this value is below `rebalance_band` × cap** (no tiny top-ups after clipping).
   - Then apply the goal clips.
4. **Accumulate clips,** with per-unit cost a = ask × (1 + cash fee rate) and quantity received per
   unit β = 1 − asset fee rate (crypto fees are taken in the asset). Truncate every bound to the
   increment:
   - n ≤ (`target_qty` − position) ÷ β;
   - n ≤ (`max_spend_usd` − goal spend) ÷ a;
   - if `max_avg_price` is set and a − `max_avg_price` × β > 0:
     n ≤ (`max_avg_price` × position − cost basis) ÷ (a − `max_avg_price` × β). Then hold if the
     projected average (cost basis + n × a) ÷ (position + n × β) would exceed `max_avg_price`.
5. Hold if the result is below the minimum order. Otherwise the proposal (purpose, quantity, limit
   price, s, outputs used, clips applied) goes to the gate dry run and autonomy (§6.2).

## 9. Versioning and change classification (DEC-43)

### 9.1 Version

`mandate_version` = `sha256:` + SHA-256 of the mandate's canonical JSON (journal spec §4).
Provenance is not part of the hashed document; `MandateConfirmed` binds it to the hash.

### 9.2 Classification

Each changed path is classified. The version is **risk-increasing if any path is**, otherwise
risk-reducing if any path is, otherwise neutral. Changing `environment` or `connection_id` is
invalid (V-031).

| Change | Classification |
|---|---|
| Maximums: allocation, `max_loss_from_allocation`, `max_*`, `breach_confirm_s`, goal quantity, spend, `max_avg_price`, `profit_level`, `stop_distance`, `exit_threshold` | Increasing if larger (null means unbounded); otherwise reducing |
| Minimums: `entry_threshold`, `rebalance_band`, `hysteresis`, `reentry_cooldown_s`, `daily_breach_min_s`, `scale_lift_after_s` | Increasing if smaller; otherwise reducing |
| Ladder | Increasing if the actions change, or any `at` or `factor` is larger; otherwise reducing |
| `scale_action` | `limit_buys` → `trim_to_target` reducing; the reverse increasing |
| Universe | Increasing if any instrument is added; reducing if only removed |
| `end_date` | Increasing if later or removed (null); reducing if earlier |
| `leveraged_etps_enabled` on, `protection.enabled` off | Increasing (the reverse is reducing) |
| Autonomy | Reducing only if every change is one of the following; anything else (reordering rules, or changing a field, operator, or compound condition, approvers, or the approval timeout) is increasing:<br>• a `then` or the `default` made stricter (auto → ask → deny);<br>• a rule added whose `then` is at least as strict as every later rule and the default;<br>• a rule removed when every later rule and the default are at least as strict as its `then`;<br>• in a single-comparison `auto` rule, one value changed so it matches less often;<br>• in a single-comparison `ask`/`deny` rule, one value changed so it matches more often, when no later rule and not the default is stricter;<br>• `two_approver_above_usd` set or lowered |
| Notifications | Removing a channel: increasing. Adding a channel or changing quiet hours: neutral |
| `name` | Neutral |
| Signal models (any change, including `max_output_age_s`), sizing method, `description`, cadence, `daily_loss_action`, `take_profit_distance`, goal type, `on_complete`, and every path not listed | Increasing (fail safe) |

**Risk-increasing versions require step-up authentication** (and independent approval where
policy requires it); reducing and neutral versions take effect on owner confirmation (§2.2). MI-11
is asserted by fuzzing random autonomy changes against random actions.

## 10. Records (DEC-45, DEC-51)

| Event | Stream | Contents |
|---|---|---|
| `MandateVersionCreated` | workspace control | Source text (artifact); compiled fields; provenance per path with quoted spans; template id and version; policy-set hashes in effect; validation results, warnings, and worst-case figures; classification against the previous version; diff |
| `MandateConfirmed` | workspace control | Version hash; confirmed paths; the rendered confirmation screen (artifact) and UI build; warnings acknowledged; step-up evidence; confirming user (opaque) |
| `DisclosureAccepted` | workspace control | Disclosure document and version hash; user (opaque); step-up evidence |
| `AgentDeployed` | workspace control | Mandate version; the rendered go-live screen (artifact) and UI build; the `BacktestRunRecorded` and paper-run IDs shown; the hypothetical-performance legend and disclosure versions shown; approving user(s), including the independent approver where policy requires one; step-up evidence |
| `PolicyChanged`, `WorkspaceProfileAssigned` | workspace control | Level, diff, author (opaque), step-up evidence, affected agents; profile, basis, assigning user |
| `MandateVersionApplied`, `HighWaterMarkReset`, `PositionReleased` | account | §5.10 |
| `OwnerExitRequested`, `ApprovalRequested` … `ApprovalCanceled` | agent | §5.10; content shown (artifact), bound quantity and price, approvers, step-up evidence |

A mandate version and its records are retained at least 6 years after the later of its
supersession and the closing (or release) of every position opened under it (trading spec §13).

## 11. Reference cases

[reference-cases/mandate.yaml](reference-cases/mandate.yaml) holds the base mandates, the
canonical-form hash vector, a signal-model registry, and 215 cases that implementations must
reproduce exactly. A case patches a base mandate with an RFC 6902 JSON Patch. They are produced by
the reference implementation in [reference/mandate](../../reference/mandate/ref.py):
`generate.py` writes the file, `check_cases.py` checks every case against the claim in its title,
`fuzz.py` asserts the invariants of §1.1 against independent oracles, and `mutants.py` confirms
the fuzz catches seeded bugs.

| Family | IDs | Covers |
|---|---|---|
| Schema | MC-S01 to MC-S23 | Structural rejects, including `on_complete`, the 300 s confirmation cap, per-model output age |
| Semantic | MC-V01 to MC-V51 | Every V-rule and warning, the closed platform-default list, loss carry |
| Policy | MC-P01 to MC-P13 | Nearest-level reporting, each key kind, the retail profile, platform maximums |
| Risk state | MC-R01 to MC-R23 | Ladder, time-in-breach confirmation, two-quote hard triggers and flash prints, clock ticks, rollover (confirmed and discarded), renewal, reset and stepwise lifts, the floor with carry and its loosening, allocation scaling and rejections, staleness, `on_complete`, `profit_stop`, dollar loss carry |
| Risk day | MC-T01 to MC-T05 | Daylight-saving boundaries |
| Gate | MC-G01 to MC-G13 | Position cap, order size, group cooldown, orders per day, gross exposure, exits exempt |
| Order builder | MC-B01 to MC-B32 | Exit and buy conviction, freshness, clipping, band, trim and its guards, deferral, averaging down, accumulate clips with fees |
| Autonomy | MC-A01 to MC-A11 | Built-in AUTO including `owner_exit`, rule order, thresholds, default, two approvers |
| Agent flatten | MC-F01 to MC-F04 | Shared account, session deferral, owner kill switch with a floor price, and without confirmation |
| Goal | MC-L01 to MC-L05 | `accumulate` completion, `on_complete`, end date (`profit_stop` is in the risk-state family) |
| Change | MC-C01 to MC-C35 | Every classification row, including rule addition, removal, and reordering |

## 12. Open questions

1. Evaluation set and target for extraction accuracy.
2. Input-drift detector for `unusual_input` (V-018).
3. Instrument-group source and maintenance (trading spec §7.1).
4. Calibration (post-v1): a user-selected, versioned method; each change journaled and classified
   risk-increasing (DEC-47).
5. Additional sizing methods (partial trims, volatility scaling) and rule fields (event windows,
   minutes to close, spread).
6. Rule fields for the close window and event windows (for example, minutes to close).
7. A crypto-spot disclosure, and an acceptable-use rule on material nonpublic information for
   user-supplied data feeds.
